//! 教材サーバー（HTTP API）の統合テスト:
//! 認証（ペアリング・セッション・CSRF）、dispatch経由のCRUD、
//! 楽観的ロック競合、Webからの禁止コマンド、パストラバーサル防御、
//! AI出力のスキーマ検証・セキュリティスキャン

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use kyozai_kobo_lib::commands::dispatch::{dispatch, Origin};
use kyozai_kobo_lib::server::build_router;
use kyozai_kobo_lib::state::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::util::ServiceExt;

fn make_state() -> (tempdir::TempDir, Arc<AppState>) {
    let dir = tempdir::TempDir::new("kyozai-server-test").unwrap();
    let conn = kyozai_kobo_lib::db::open_db(dir.path()).unwrap();
    kyozai_kobo_lib::commands::templates::seed_default_template(&conn).ok();
    let state = Arc::new(AppState::new(conn, dir.path().to_path_buf()));
    *state.server.pairing_code.lock().unwrap() = "12345678".to_string();
    (dir, state)
}

fn insert_chat_test_problem(state: &Arc<AppState>) -> i64 {
    let conn = state.conn.lock().unwrap();
    conn.execute("INSERT INTO subjects(name,sort_order) VALUES ('数学',1)", [])
        .unwrap();
    let subject_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO fields(subject_id,name,sort_order) VALUES (?1,'数学I',1)",
        [subject_id],
    )
    .unwrap();
    let field_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO units(field_id,name,sort_order) VALUES (?1,'二次関数',1)",
        [field_id],
    )
    .unwrap();
    let unit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO problems
         (unit_id,title,statement_latex,statement_latex_two_column,answer_latex,explanation_latex,
          answer_completed,explanation_completed,difficulty,is_required,memo,created_at,updated_at,version)
         VALUES (?1,'元の題名','\\(x^2=1\\)','', '解答済み','解説済み',1,1,'標準',0,'','2026-08-19','2026-08-19',1)",
        [unit_id],
    )
    .unwrap();
    conn.last_insert_rowid()
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn post_json(uri: &str, body: Value, cookie: Option<&str>, csrf: bool) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "127.0.0.1:8760");
    if csrf {
        b = b.header("x-requested-with", "kyozai-kobo");
    }
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn query_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// ペアリングしてセッションCookieを得る
async fn pair(router: &axum::Router) -> String {
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/auth/pair",
            json!({"code": "12345678", "deviceName": "テストiPad"}),
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "ペアリングが成功すること");
    let set_cookie = res
        .headers()
        .get(header::SET_COOKIE)
        .expect("Set-Cookieがあること")
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.contains("HttpOnly"), "HttpOnlyが付くこと");
    assert!(set_cookie.contains("SameSite=Lax"), "SameSite=Laxが付くこと");
    set_cookie.split(';').next().unwrap().to_string()
}

#[tokio::test]
async fn health_is_public() {
    let (_dir, state) = make_state();
    let router = build_router(state.clone());
    let res = router
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v = body_json(res).await;
    assert_eq!(v["ok"], json!(true));
}

#[tokio::test]
async fn write_api_requires_auth_and_csrf() {
    let (_dir, state) = make_state();
    let router = build_router(state);

    // 未認証 + CSRFヘッダー無し → 403（CSRF検証が先）
    let res = router
        .clone()
        .oneshot(post_json("/api/invoke/get_tree", json!({}), None, false))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // CSRFヘッダーはあるが未認証 → 401
    let res = router
        .clone()
        .oneshot(post_json("/api/invoke/get_tree", json!({}), None, true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 間違ったペアリングコード → 401
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/auth/pair",
            json!({"code": "00000000", "deviceName": "x"}),
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn paired_session_can_crud_and_detect_conflicts() {
    let (_dir, state) = make_state();
    let router = build_router(state.clone());
    let cookie = pair(&router).await;

    // ツリー作成
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/add_tree_node",
            json!({"kind": "subject", "parentId": null, "name": "数学"}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let subject_id = body_json(res).await.as_i64().unwrap();
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/add_tree_node",
            json!({"kind": "field", "parentId": subject_id, "name": "数I"}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    let field_id = body_json(res).await.as_i64().unwrap();
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/add_tree_node",
            json!({"kind": "unit", "parentId": field_id, "name": "二次関数"}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    let unit_id = body_json(res).await.as_i64().unwrap();

    // 問題作成・取得
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/create_problem",
            json!({"unitId": unit_id, "title": "テスト問題"}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    let problem_id = body_json(res).await.as_i64().unwrap();

    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/get_problem",
            json!({"id": problem_id}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    let problem = body_json(res).await;
    assert_eq!(problem["version"], json!(1));

    // 保存（version 1 → 2）
    let payload = json!({
        "payload": {
            "id": problem_id,
            "unit_id": unit_id,
            "title": "テスト問題",
            "statement_latex": "端末Aの本文",
            "statement_latex_two_column": "端末Aの二段組本文",
            "answer_latex": "",
            "explanation_latex": "",
            "difficulty": "標準",
            "difficulty_rank": null,
            "is_required": false,
            "memo": "",
            "tags": [],
            "expected_version": 1
        }
    });
    let res = router
        .clone()
        .oneshot(post_json("/api/invoke/update_problem", payload.clone(), Some(&cookie), true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await.as_i64(), Some(2));
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/get_problem",
            json!({"id": problem_id}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    let updated_problem = body_json(res).await;
    assert_eq!(
        updated_problem["statement_latex_two_column"],
        json!("端末Aの二段組本文")
    );

    // 同じ expected_version=1 で再保存 → 409 CONFLICT
    let res = router
        .clone()
        .oneshot(post_json("/api/invoke/update_problem", payload, Some(&cookie), true))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let err = body_json(res).await;
    assert!(err["error"].as_str().unwrap().starts_with("CONFLICT:2"));
}

#[tokio::test]
async fn web_blocked_commands_and_traversal_are_rejected() {
    let (_dir, state) = make_state();
    let router = build_router(state.clone());
    let cookie = pair(&router).await;

    // ローカルパスを扱うコマンドはWebから禁止
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/import_bank",
            json!({"path": "C:\\Windows\\win.ini"}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let err = body_json(res).await;
    assert!(err["error"].as_str().unwrap().contains("ブラウザからは利用できません"));

    // ファイル配信のパストラバーサル
    let res = router
        .clone()
        .oneshot(
            Request::get("/api/files/attachment/..%2F..%2Fkyozai-kobo.db")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(res.status(), StatusCode::OK, "トラバーサルは失敗すること");

    // 許可されていない絶対パスの成果物取得
    let res = router
        .clone()
        .oneshot(
            Request::get("/api/files/build?path=C:%5CWindows%5Cwin.ini")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 正常なプレビューPDFはキャッシュ回避クエリ付きでも取得できる。
    let build_root = std::env::temp_dir().join("kyozai-kobo-build");
    std::fs::create_dir_all(&build_root).unwrap();
    let preview_dir = tempdir::TempDir::new_in(&build_root, "preview-http-test").unwrap();
    let preview_pdf = preview_dir.path().join("kyozai.pdf");
    std::fs::write(&preview_pdf, b"%PDF-1.4\n%%EOF\n").unwrap();
    let preview_uri = format!(
        "/api/files/build?path={}&t=123456789",
        query_encode(&preview_pdf.to_string_lossy())
    );
    let res = router
        .clone()
        .oneshot(
            Request::get(preview_uri)
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/pdf"
    );
    assert!(
        res.headers().get(header::CONTENT_DISPOSITION).is_none(),
        "プレビュー表示ではattachmentを付けないこと"
    );

    // スマホ向けの直接ダウンロードではattachmentとUTF-8ファイル名を返す。
    let download_pdf = preview_dir.path().join("数学教材_解答.pdf");
    std::fs::write(&download_pdf, b"%PDF-1.4\n%%EOF\n").unwrap();
    let download_uri = format!(
        "/api/files/build?path={}&download=1",
        query_encode(&download_pdf.to_string_lossy())
    );
    let res = router
        .clone()
        .oneshot(
            Request::get(download_uri)
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/pdf"
    );
    let disposition = res
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(disposition.starts_with("attachment;"));
    assert!(disposition.contains("filename=\"kyozai.pdf\""));
    assert!(disposition.contains(
        "filename*=UTF-8''%E6%95%B0%E5%AD%A6%E6%95%99%E6%9D%90_%E8%A7%A3%E7%AD%94.pdf"
    ));
    assert_eq!(
        res.headers().get(header::CACHE_CONTROL).unwrap(),
        "private, no-store"
    );

    // UNCはcanonicalize等でネットワークへ触れる前に拒否
    let res = router
        .clone()
        .oneshot(
            Request::get("/api/files/build?path=%5C%5Cattacker.invalid%5Cshare%5Cx.pdf")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Webからホストの実行ファイル・出力先設定を変更できない
    let res = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/set_settings",
            json!({"settings": {"uplatex_path": "\\\\attacker.invalid\\share\\evil.exe"}}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let settings = dispatch(&state, "get_settings", json!({}), Origin::Desktop).unwrap();
    assert!(settings.get("uplatex_path").is_none());

    // Web向け設定取得にはローカルdata_dirを含めない
    let web_settings = dispatch(&state, "get_settings", json!({}), Origin::Web).unwrap();
    assert!(web_settings.get("data_dir").is_none());
}

#[test]
fn ai_model_setting_is_validated_persisted_and_available_from_web() {
    let (_dir, state) = make_state();

    dispatch(
        &state,
        "codex_set_model",
        json!({"model": "gpt-5.4-mini"}),
        Origin::Web,
    )
    .expect("ブラウザから安全なモデル選択を保存できること");
    assert_eq!(
        kyozai_kobo_lib::codex::selected_model(&state).unwrap(),
        Some("gpt-5.4-mini".into())
    );

    let invalid = dispatch(
        &state,
        "codex_set_model",
        json!({"model": "gpt-5.4; unsafe"}),
        Origin::Web,
    );
    assert!(invalid.is_err(), "不正なモデル名を拒否すること");
    assert_eq!(
        kyozai_kobo_lib::codex::selected_model(&state).unwrap(),
        Some("gpt-5.4-mini".into()),
        "不正入力で保存済み設定を変更しないこと"
    );

    dispatch(
        &state,
        "codex_set_model",
        json!({"model": ""}),
        Origin::Desktop,
    )
    .expect("空欄でCodexの既定モデルへ戻せること");
    assert_eq!(
        kyozai_kobo_lib::codex::selected_model(&state).unwrap(),
        None
    );
}

#[tokio::test]
async fn static_ui_has_security_headers() {
    let (_dir, state) = make_state();
    let router = build_router(state);
    let res = router
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        res.headers()
            .get(header::X_FRAME_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("DENY")
    );
    assert!(res.headers().contains_key(header::CONTENT_SECURITY_POLICY));
}

#[tokio::test]
async fn upload_rejects_fake_images() {
    let (_dir, state) = make_state();
    let router = build_router(state.clone());
    let cookie = pair(&router).await;

    // 問題を用意
    {
        let conn = state.conn.lock().unwrap();
        conn.execute_batch(
            "INSERT INTO subjects (name, sort_order) VALUES ('s',1);
             INSERT INTO fields (subject_id, name, sort_order) VALUES (1,'f',1);
             INSERT INTO units (field_id, name, sort_order) VALUES (1,'u',1);
             INSERT INTO problems (unit_id, title, created_at, updated_at) VALUES (1,'p','2026-01-01','2026-01-01');",
        )
        .unwrap();
    }

    // 拡張子偽装（.pngだが中身はテキスト）
    let boundary = "XBOUNDARYX";
    let body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"evil.png\"\r\nContent-Type: image/png\r\n\r\nこれは画像ではありません\r\n--{b}--\r\n",
        b = boundary
    );
    let res = router
        .clone()
        .oneshot(
            Request::post("/api/uploads/attachment?problemId=1")
                .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={}", boundary))
                .header("x-requested-with", "kyozai-kobo")
                .header(header::HOST, "127.0.0.1:8760")
                .header(header::COOKIE, &cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let err = body_json(res).await;
    assert!(err["error"].as_str().unwrap().contains("PNG"), "形式エラーの説明があること");
}

#[test]
fn dispatch_desktop_allows_but_web_blocks_path_commands() {
    let (_dir, state) = make_state();
    // Desktopでは analyze_tex_file が呼べる（存在しないパスはIOエラーになるが、禁止はされない）
    let r = dispatch(
        &state,
        "analyze_tex_file",
        json!({"path": "Z:\\not\\exist.tex"}),
        Origin::Desktop,
    );
    assert!(r.is_err());
    assert!(!r.unwrap_err().contains("ブラウザ"));

    let r = dispatch(
        &state,
        "analyze_tex_file",
        json!({"path": "Z:\\not\\exist.tex"}),
        Origin::Web,
    );
    assert_eq!(r.unwrap_err(), "このコマンドはブラウザからは利用できません");
}

#[test]
fn dispatch_emits_change_events() {
    let (_dir, state) = make_state();
    let mut rx = state.events.subscribe();
    let id = dispatch(
        &state,
        "add_tree_node",
        json!({"kind": "subject", "parentId": null, "name": "英語"}),
        Origin::Desktop,
    )
    .unwrap();
    assert!(id.as_i64().unwrap() > 0);
    let ev = rx.try_recv().expect("変更イベントが発火すること");
    assert_eq!(ev.kind, "tree");
    assert_eq!(ev.cmd, "add_tree_node");
}

// ---- AI出力の検証 ----

#[test]
fn ai_output_validation() {
    use kyozai_kobo_lib::ai::{
        scan_explanation_structure, scan_latex_security, scan_limit_formula_structure,
        scan_solution_layout, scan_solution_notation, scan_tikz_monochrome, validate_output,
        SOLUTION_FIXED_INSTRUCTIONS,
    };

    let valid = json!({
        "schemaVersion": 1,
        "detectedType": "problem",
        "latex": "\\noindent 次の問いに答えよ。",
        "plainText": "次の問いに答えよ。",
        "requiredPackages": ["amsmath"],
        "warnings": [{"code": "UNCLEAR_SYMBOL", "severity": "warning", "message": "指数が不鮮明"}],
        "uncertainFragments": [{"id": "u1", "description": "第2式の指数", "candidates": ["2", "3"]}],
        "segments": [{"order": 1, "kind": "text", "latex": "次の問いに答えよ。"}],
        "suggestedInsertTarget": "problem_body",
        "problems": []
    })
    .to_string();
    let r = validate_output(&valid).expect("正しいJSONは通ること");
    assert_eq!(r.detected_type, "problem");
    assert_eq!(r.uncertain_fragments.len(), 1);

    // コードフェンス付きでも防御的に受理
    let fenced = format!("```json\n{}\n```", valid);
    assert!(validate_output(&fenced).is_ok());

    // 不正JSON
    assert!(validate_output("これはJSONではない").is_err());
    // 必須欠落
    assert!(validate_output(r#"{"schemaVersion":1}"#).is_err());
    // 追加プロパティは禁止
    let extra = valid.replacen("{", r#"{"unexpected":true,"#, 1);
    assert!(validate_output(&extra).is_err());
    // 列挙外のseverityは禁止
    let bad_severity = valid.replace("\"severity\":\"warning\"", "\"severity\":\"critical\"");
    assert!(validate_output(&bad_severity).is_err());
    // 不正なdetectedType
    let bad_type = valid.replace("\"problem\"", "\"hacking\"");
    assert!(validate_output(&bad_type).is_err());
    // 未対応schemaVersion
    let bad_ver = valid.replace("\"schemaVersion\":1", "\"schemaVersion\":99");
    assert_ne!(bad_ver, valid, "置換が行われていること");
    assert!(validate_output(&bad_ver).is_err());

    // 危険コマンドのスキャン
    let warnings = scan_latex_security("\\write18{del *.*} \\includegraphics{C:/secret/x.png}");
    assert!(warnings.iter().any(|w| w.message.contains("\\write18")));
    assert!(warnings.iter().any(|w| w.code == "UNSAFE_IMAGE_PATH"));
    assert!(warnings.iter().all(|w| w.severity == "error"));
    assert!(scan_latex_security("$x^2+1$").is_empty());

    let safe_figure = "\\noindent\\includegraphics[width=0.65\\linewidth,height=0.28\\textheight,keepaspectratio]{figure.pdf}\\par";
    assert!(scan_solution_layout(safe_figure, "two_column").is_empty());
    let unsafe_layout = "\\begin{center}\\includegraphics[width=\\textwidth]{figure.pdf}\\end{center}";
    let layout_warnings = scan_solution_layout(unsafe_layout, "two_column");
    assert!(layout_warnings.iter().any(|warning| warning.code == "TWO_COLUMN_LAYOUT"));
    assert!(layout_warnings.iter().any(|warning| warning.code == "FIGURE_SIZE"));
    assert!(layout_warnings.iter().all(|warning| warning.severity == "error"));
    let wide_fixed_figure =
        "\\noindent\\includegraphics[width=12cm,keepaspectratio]{figure.pdf}\\par";
    assert!(scan_solution_layout(wide_fixed_figure, "single_column").is_empty());
    assert!(scan_solution_layout(wide_fixed_figure, "two_column")
        .iter()
        .any(|warning| warning.code == "FIGURE_SIZE"));

    let wide_equivalence = r#"\begin{aligned}
C(x,y)\in R
&\Longleftrightarrow
\begin{gathered}
\text{「点 }A(a,0),B(0,b)\text{ が条件を満たし，}\\
\text{実数 }a,b\text{ が存在する」}
\end{gathered}\\
&\Longleftrightarrow \frac{x^2}{16}+\frac{y^2}{4}=1
\end{aligned}"#;
    assert!(scan_solution_layout(wide_equivalence, "two_column")
        .iter()
        .any(|warning| warning.code == "TWO_COLUMN_EQUIVALENCE_WIDTH"));
    assert!(!scan_solution_layout(wide_equivalence, "single_column")
        .iter()
        .any(|warning| warning.code == "TWO_COLUMN_EQUIVALENCE_WIDTH"));

    let wrapped_equivalence = r#"\begin{aligned}
&C(x,y)\in R\\
&\Longleftrightarrow
\begin{gathered}
\text{「点 }A(a,0),B(0,b)\text{ が条件を満たし，}\\
\text{実数 }a,b\text{ が存在する」}
\end{gathered}\\
&\Longleftrightarrow \frac{x^2}{16}+\frac{y^2}{4}=1
\end{aligned}"#;
    assert!(!scan_solution_layout(wrapped_equivalence, "two_column")
        .iter()
        .any(|warning| warning.code == "TWO_COLUMN_EQUIVALENCE_WIDTH"));

    let monochrome_tikz = r#"\begin{tikzpicture}
  \fill[gray!15] (0,0)--(1,0)--(0,1)--cycle;
  \draw[thick,solid] (0,0)--(1,1);
  \draw[thick,densely dashed] (0,1)--(1,0);
\end{tikzpicture}"#;
    assert!(scan_tikz_monochrome(monochrome_tikz).is_empty());
    assert!(scan_solution_layout(monochrome_tikz, "two_column").is_empty());
    let colored_tikz = monochrome_tikz.replace("thick,solid", "thick,red");
    assert!(scan_tikz_monochrome(&colored_tikz)
        .iter()
        .any(|warning| warning.code == "TIKZ_COLOR_NOT_MONOCHROME"));
    assert!(scan_solution_layout(&colored_tikz, "two_column")
        .iter()
        .any(|warning| warning.code == "TIKZ_COLOR_NOT_MONOCHROME"));

    let unexplained_notation = scan_solution_notation("$a\\mid b$, $\\max\\{a,b\\}$");
    assert_eq!(unexplained_notation.len(), 2);
    assert!(unexplained_notation
        .iter()
        .all(|warning| warning.code == "UNEXPLAINED_NOTATION" && warning.severity == "error"));
    assert!(scan_solution_notation(
        "$a\\mid b$は$b$が$a$で割り切れることを表し、$\\max\\{a,b\\}$は$a,b$のうち大きい方を表す。"
    )
    .is_empty());
    assert!(scan_solution_notation(
        "$a\\equiv b\\pmod m$は、$a,b$を$m$で割った余りが等しいことを表す。"
    )
    .is_empty());
    for non_high_school_term in [
        "この2次方程式は異なる2実根をもつ。",
        "方程式の根は$x=1$である。",
        "この方程式の2つの根を求める。",
        "解の公式により重根をもつ。",
        "根と係数の関係を用いる。",
        "関数$f$の零点を求める。",
    ] {
        let warnings = scan_solution_notation(non_high_school_term);
        assert!(warnings.iter().any(|warning| {
            warning.code == "NON_HIGH_SCHOOL_SOLUTION_TERM" && warning.severity == "error"
        }));
    }
    for standard_term in [
        "この2次方程式は異なる2つの実数解をもつ。",
        "解の公式と解と係数の関係を用いる。",
        "平方根を求め、根号を外す根拠を示す。",
    ] {
        assert!(scan_solution_notation(standard_term).is_empty());
    }
    let inverse_trig = scan_solution_notation("$y=\\arcsin x$");
    assert!(inverse_trig.iter().any(|warning| {
        warning.code == "OUT_OF_SCOPE_INVERSE_TRIG" && warning.severity == "error"
    }));
    let direct_inverse_derivative =
        scan_solution_notation("$\\dfrac{d}{dx}\\left(\\sin^{-1}x\\right)$");
    assert!(direct_inverse_derivative.iter().any(|warning| {
        warning.code == "DIRECT_INVERSE_TRIG_DERIVATIVE" && warning.severity == "error"
    }));
    assert!(scan_solution_notation(
        "$y=\\sin^{-1}x$とおくと$x=\\sin y$であるから、$1=\\cos y\\dfrac{dy}{dx}$である。"
    )
    .is_empty());
    for forbidden in ["$x\\leq 1$", "$x\\ge 0$", "$a\\leqslant b$", "$x≤1$"] {
        let warnings = scan_solution_notation(forbidden);
        assert!(warnings.iter().any(|warning| {
            warning.code == "INEQUALITY_SYMBOL_STYLE" && warning.severity == "error"
        }));
    }
    assert!(scan_solution_notation("$0\\leqq x<1$かつ$y\\geqq 2$").is_empty());
    for quantified in [
        "$\\exists t\\in\\mathbb{R}$",
        "$\\forall x\\in\\mathbb{R}$",
    ] {
        let warnings = scan_solution_notation(quantified);
        assert!(warnings.iter().any(|warning| {
            warning.code == "QUANTIFIER_NOTATION_STYLE" && warning.severity == "error"
        }));
    }
    assert!(scan_solution_notation("条件を満たす実数$t$が存在する。").is_empty());
    for decorated in ["$\\boxed{x=1}$", "\\fbox{$x=1$}", "$x=1$（答）"] {
        let warnings = scan_solution_notation(decorated);
        assert!(warnings.iter().any(|warning| {
            warning.code == "ANSWER_DECORATION" && warning.severity == "error"
        }));
    }
    for bold_vector in [
        "$\\mathbf{a}$",
        "$\\boldsymbol{v}$",
        "$\\bm{x}$",
        "$\\pmb{AB}$",
    ] {
        let warnings = scan_solution_notation(bold_vector);
        assert!(warnings.iter().any(|warning| {
            warning.code == "VECTOR_NOTATION_STYLE" && warning.severity == "error"
        }));
    }
    assert!(scan_solution_notation(
        "$\\vec{a}$、$\\overrightarrow{AB}$、$\\vec{0}$を考える。"
    )
    .is_empty());
    for point_with_equals in ["$M=(x,y)$とする。", "$A = \\left(1,2\\right)$とする。"] {
        let warnings = scan_solution_notation(point_with_equals);
        assert!(warnings.iter().any(|warning| {
            warning.code == "POINT_COORDINATE_NOTATION" && warning.severity == "error"
        }));
    }
    assert!(scan_solution_notation("$AB$の中点を$M(x,y)$とする。").is_empty());
    assert!(scan_solution_notation("点$A(1,2)$を通る直線を考える。").is_empty());
    for braced_with_commas in [
        r#"\left\{\begin{aligned}x&=1,\\y&=2\end{aligned}\right."#,
        r#"\left\{\begin{aligned}x&=1\\y&=2,\end{aligned}\right."#,
        r#"\begin{cases}x=1,\\y=2\end{cases}"#,
    ] {
        let warnings = scan_solution_notation(braced_with_commas);
        assert!(warnings.iter().any(|warning| {
            warning.code == "BRACED_SYSTEM_COMMA" && warning.severity == "error"
        }));
    }
    assert!(scan_solution_notation(
        r#"\left\{\begin{aligned}x&=1\\y&=2\end{aligned}\right."#
    )
    .is_empty());
    for term in ["臨界点", "臨界値", "critical point", "critical value"] {
        let warnings = scan_solution_notation(term);
        assert!(warnings.iter().any(|warning| {
            warning.code == "NON_HIGH_SCHOOL_CRITICAL_TERM" && warning.severity == "error"
        }));
    }
    let difference_quotient_term =
        scan_solution_notation(r#"差商$\dfrac{f(a+h)-f(a)}{h}$の極限を考える。"#);
    assert!(difference_quotient_term.iter().any(|warning| {
        warning.code == "NON_HIGH_SCHOOL_DIFFERENCE_QUOTIENT_TERM"
            && warning.severity == "error"
    }));
    for standard_derivative_term in [
        r#"$\dfrac{f(a+h)-f(a)}{h}$は、$x=a$から$x=a+h$までの平均変化率である。"#,
        r#"平均変化率の極限$\lim_{h\to0}\dfrac{f(a+h)-f(a)}{h}$が、$x=a$における微分係数である。"#,
        r#"微分係数を定義する式$\lim_{h\to0}\dfrac{f(a+h)-f(a)}{h}$を用いる。"#,
    ] {
        assert!(scan_solution_notation(standard_derivative_term)
            .iter()
            .all(|warning| warning.code != "NON_HIGH_SCHOOL_DIFFERENCE_QUOTIENT_TERM"));
    }
    for non_high_school_binomial in [
        r#"$\binom{n}{r}$"#,
        r#"$\dbinom{8}{3}$"#,
        r#"$\tbinom{n}{2}$"#,
        r#"${n\choose r}$"#,
        r#"$\mathrm{C}(n,r)$"#,
        r#"${}^{n}\mathrm{C}_{r}$"#,
    ] {
        let warnings = scan_solution_notation(non_high_school_binomial);
        assert!(warnings.iter().any(|warning| {
            warning.code == "NON_HIGH_SCHOOL_BINOMIAL_NOTATION"
                && warning.severity == "error"
        }));
    }
    for high_school_binomial in [
        r#"${}_n\mathrm{C}_r$"#,
        r#"${}_5\mathrm{C}_2=10$"#,
        r#"${}_n\mathrm{C}_r=\dfrac{n!}{r!(n-r)!}$"#,
        r#"点$C(1,2)$を通る。"#,
    ] {
        assert!(scan_solution_notation(high_school_binomial)
            .iter()
            .all(|warning| warning.code != "NON_HIGH_SCHOOL_BINOMIAL_NOTATION"));
    }
    let derivative_range_without_table = r#"$f'(x)=x-1$であり、$f'(x)>0$と$f'(x)<0$となる区間を調べると、値域が求まる。"#;
    assert!(scan_solution_notation(derivative_range_without_table)
        .iter()
        .any(|warning| warning.code == "MISSING_VARIATION_TABLE"));
    let differentiated_piecewise_quadratic = r#"
最大値を求めるため
\[
G(y)=\begin{cases}
y^2+4y & (0\leqq y\leqq1)\\
y^2-8y+12 & (1\leqq y\leqq2)
\end{cases}
\]
とおく。$0\leqq y\leqq1$では$G'(y)=2y+4>0$であり、
$1\leqq y\leqq2$では$G'(y)=2y-8<0$である。"#;
    let differentiated_quadratic_warnings =
        scan_solution_notation(differentiated_piecewise_quadratic);
    assert!(differentiated_quadratic_warnings.iter().any(|warning| {
        warning.code == "UNNECESSARY_QUADRATIC_DIFFERENTIATION"
            && warning.severity == "error"
    }));
    assert!(differentiated_quadratic_warnings
        .iter()
        .all(|warning| warning.code != "MISSING_VARIATION_TABLE"));
    let completed_square_quadratic = r#"
最大値を求める。$0\leqq y\leqq1$では
$y^2+4y=(y+2)^2-4$であり、軸$y=-2$は区間の外にあるから端点を比べる。
$1\leqq y\leqq2$では$y^2-8y+12=(y-4)^2-4$であり、軸$y=4$は区間の外にあるから端点を比べる。"#;
    assert!(scan_solution_notation(completed_square_quadratic)
        .iter()
        .all(|warning| warning.code != "UNNECESSARY_QUADRATIC_DIFFERENTIATION"));
    let incomplete_variation_table = r#"$f'(x)=x-1$の正負から値域を求める。
\[\begin{array}{c|ccc}x&0&1&2\\ \hline f'(x)&-&0&+\end{array}\]"#;
    assert!(scan_solution_notation(incomplete_variation_table)
        .iter()
        .any(|warning| warning.code == "INCOMPLETE_VARIATION_TABLE"));
    assert!(scan_solution_notation(
        r#"$f'(x)=1>0$であるから定義域全体で増加し、値域は$0<y<1$である。"#
    )
    .iter()
    .all(|warning| warning.code != "MISSING_VARIATION_TABLE"));
    assert!(scan_solution_notation("したがって、$x=1$である。").is_empty());
    for punctuated in [
        "$x=1$.",
        r#"\[x=1.\]"#,
        r#"\begin{align*}x&=1.\\y&=2\end{align*}"#,
        r#"\begin{align*}x&=1\end{align*}."#,
    ] {
        let warnings = scan_solution_notation(punctuated);
        assert!(warnings.iter().any(|warning| {
            warning.code == "FORMULA_TRAILING_PERIOD" && warning.severity == "error"
        }));
    }
    assert!(scan_solution_notation("$x=1.5$である。").is_empty());
    assert!(scan_explanation_structure(
        "着眼点を示す。\\par\\textbf{【定石】}平方完成の形に着目する。"
    )
    .is_empty());
    let missing_standard_method = scan_explanation_structure("着眼点と方針だけを示す。");
    assert!(missing_standard_method.iter().any(|warning| {
        warning.code == "MISSING_STANDARD_METHOD" && warning.severity == "error"
    }));
    assert!(scan_solution_notation("$x^2+1=0$").is_empty());

    let piecewise_differentiable_problem = r#"関数
\[
f(x)=
\begin{cases}
x^3+\alpha x & (x\geqq2)\\
\beta x^2-\alpha x & (x<2)
\end{cases}
\]
が$x=2$で微分可能となるように定数$\alpha,\beta$を定めよ。"#;
    let omitted_limit_answer = r#"微分可能であるためには連続でなければならないので、左右の値から
\[
4\beta-2\alpha=8+2\alpha
\]
また、左右の微分係数が等しいことから
\[
4\beta-\alpha=12+\alpha
\]"#;
    let omitted_limit_warnings =
        scan_limit_formula_structure(piecewise_differentiable_problem, omitted_limit_answer);
    assert!(omitted_limit_warnings
        .iter()
        .any(|warning| warning.code == "ONE_SIDED_LIMIT_FORMULA_MISSING"));
    assert!(omitted_limit_warnings.iter().any(|warning| {
        warning.code == "ONE_SIDED_DERIVATIVE_LIMIT_FORMULA_MISSING"
    }));

    let explicit_limit_answer = r#"連続であるための条件は
\[
\lim_{x\to2-0}f(x)=4\beta-2\alpha,
\qquad
f(2)=\lim_{x\to2+0}f(x)=8+2\alpha
\]
より、$4\beta-2\alpha=8+2\alpha$である。また、
\[
\lim_{x\to2-0}f'(x)=4\beta-\alpha,
\qquad
\lim_{x\to2+0}f'(x)=12+\alpha
\]
であるから、微分可能であるための条件は$4\beta-\alpha=12+\alpha$である。"#;
    assert!(scan_limit_formula_structure(
        piecewise_differentiable_problem,
        explicit_limit_answer
    )
    .is_empty());
    assert!(scan_limit_formula_structure(
        r#"$n\to\infty$のときの数列の極限を求めよ。"#,
        "極限値は$2$である。"
    )
    .iter()
    .any(|warning| warning.code == "LIMIT_FORMULA_MISSING"));
    assert!(scan_limit_formula_structure(
        r#"$n\to\infty$のときの数列の極限を求めよ。"#,
        r#"$\lim_{n\to\infty}a_n=2$である。"#
    )
    .is_empty());
    let continuous_range_with_system = r#"線分が動いてできる立体の体積を求めよ。
解答では、$f_x$は区間$[x,1]$で連続であることを用いる。
\[
X(x,z)\in D\Longleftrightarrow
\left\{
\begin{aligned}
0&<x<1\\
0&\leqq z\leqq f_x(s)
\end{aligned}
\right.
\]"#;
    assert!(scan_limit_formula_structure(
        continuous_range_with_system,
        continuous_range_with_system
    )
    .iter()
    .all(|warning| warning.code != "ONE_SIDED_LIMIT_FORMULA_MISSING"),
    "連続な関数の値域と連立条件を区分関数の接続条件として誤検出しないこと");
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("\\lim_{h\\to-0}"));
}

#[test]
fn constrained_two_variable_extremum_prompt_and_structure_regression() {
    use kyozai_kobo_lib::ai::{
        is_constrained_two_variable_extremum_problem,
        scan_constrained_two_variable_extremum_structure,
        scan_tikz_monochrome,
        should_attach_constrained_two_variable_extremum_instructions,
        CONSTRAINED_TWO_VARIABLE_EXTREMUM_INSTRUCTIONS,
    };

    let circle_problem =
        "$x^2+y^2=1$のとき、$4x+3y$の最大値と最小値を求めよ。";
    let triangle_problem = r#"$x,y$が$x\geqq0$, $y\geqq0$, $x+y\leqq1$を満たすとき、$x^2+y^2$の最大値・最小値を求めよ。"#;
    assert!(is_constrained_two_variable_extremum_problem(circle_problem));
    assert!(is_constrained_two_variable_extremum_problem(triangle_problem));
    assert!(!is_constrained_two_variable_extremum_problem(
        r#"$0\leqq x\leqq1$のとき、関数$f(x)=x^2-x$の最大値を求めよ。"#
    ));
    assert!(!is_constrained_two_variable_extremum_problem(
        "$x+y=1$を満たす点の軌跡を求めよ。"
    ));
    assert!(should_attach_constrained_two_variable_extremum_instructions(
        "text",
        circle_problem
    ));
    assert!(!should_attach_constrained_two_variable_extremum_instructions(
        "text",
        "関数$f(x)$の最大値を求めよ。"
    ));
    assert!(should_attach_constrained_two_variable_extremum_instructions(
        "image", ""
    ));

    for required in [
        "F(x,y)=k",
        "共有点をもつ",
        "条件領域と共有点をもつ限界の位置",
        "端点、頂点、境界同士の交点",
        "TikZ",
        "偏微分、勾配、ラグランジュ",
        "問題が値域を求めていない限り",
        "この2次方程式は重解をもつ",
        "候補が複数残る場合だけ",
        "【図から極値位置が明らかな場合のfew-shot】",
        "\\clipを使用してはいけません",
        "極値計算に不要な中間位置",
        "すべてのnodeが切れず",
        "帰着した1変数関数が二次関数なら微分してはいけません",
        "【二次関数へ帰着する別解のfew-shot】",
        "平方完成、軸と区間の位置関係、端点・継ぎ目の比較",
    ] {
        assert!(
            CONSTRAINED_TWO_VARIABLE_EXTREMUM_INSTRUCTIONS.contains(required),
            "専用指示に不足: {required}"
        );
    }

    let geometric_answer = r#"
制約条件は単位円である。
求める式を$k$とおくと
\[
4x+3y=k
\]
は直線を表す。$k$を増加させると、この直線は右上へ平行移動する。
\begin{tikzpicture}[scale=0.8]
  \draw[->] (-1.5,0)--(1.5,0) node[right] {$x$};
  \draw[->] (0,-1.5)--(0,1.5) node[above] {$y$};
  \draw (0,0) circle (1);
  \draw[dashed] (-1.3,-0.23)--(0.4,2.03);
\end{tikzpicture}
$k$が目的式のとり得る値であることは、直線$4x+3y=k$と単位円が共有点をもつことと同値である。
図より、直線と円が最初に共有点をもつ限界では$k=-5$、最後に共有点をもつ限界では$k=5$である。
したがって、最大値は$5$で、そのとき$(x,y)=(4/5,3/5)$、最小値は$-5$で、そのとき$(x,y)=(-4/5,-3/5)$である。
"#;
    let warnings =
        scan_constrained_two_variable_extremum_structure(circle_problem, geometric_answer);
    assert!(
        warnings.is_empty(),
        "正しい図形移動法に警告: {:?}",
        warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );

    let parabola_problem = r#"3つの不等式$x+2y\geqq0$、$x-y\leqq0$、$x-4y+6\geqq0$を満たす$x,y$に対して、$y^2-2x$の最大値と最小値を求めよ。また、そのときの$x,y$の値を求めよ。"#;
    let parabola_answer = r#"
条件領域は、頂点$A(0,0)$、$B(-2,1)$、$C(2,2)$の三角形である。
求める式を$k$と置くと
\[
y^2-2x=k
\]
であり、$x=\dfrac12y^2-\dfrac{k}{2}$より、これは右向きに開く放物線である。
$k$を増加させると、放物線は左へ平行移動する。
\noindent\resizebox{0.70\linewidth}{!}{%
\begin{tikzpicture}[x=1.05cm,y=1.05cm,>=stealth,font=\small]
  \fill[gray!15] (0,0)--(-2,1)--(2,2)--cycle;
  \draw[->] (-3.15,0)--(3.15,0) node[below right] {$x$};
  \draw[->] (0,-0.35)--(0,2.70) node[above left] {$y$};
  \draw[thick] (0,0)--(-2,1)--(2,2)--cycle;
  \draw[thick,solid,domain=-0.1:2.1,samples=70,smooth,variable=\t]
    plot ({0.5*\t*\t+0.5},{\t});
  \draw[thick,densely dashed,domain=-0.1:2.2,samples=70,smooth,variable=\t]
    plot ({0.5*\t*\t-2.5},{\t});
  \fill (0,0) circle (1.5pt) node[below right] {$A$};
  \fill (-2,1) circle (1.5pt) node[above left] {$B$};
  \fill (2,2) circle (1.5pt) node[above right] {$C$};
  \fill (1,1) circle (1.5pt) node[below right] {$(1,1)$};
  \node[right] at (2.45,1.45) {$k=-1$};
  \node[left] at (-2.55,0.55) {$k=5$};
  \draw[->] (2.75,2.48)--(1.55,2.48)
    node[midway,above] {$k\text{ の増加}$};
\end{tikzpicture}%
}\par\smallskip
図より、放物線が領域と初めて共有点をもつのは、辺$AC$を含む直線$x=y$に接するときである。連立すると
\[
y^2-2y-k=0
\]
を得る。接するとき、この2次方程式は重解をもつから
\[
D=4+4k=0
\]
より、$k=-1$である。このとき$x=1$、$y=1$である。
また図より、放物線が領域と最後に共有点をもつのは頂点$B(-2,1)$を通るときであるから
\[
k=1^2-2(-2)=5
\]
である。
したがって、最大値は$5$で、そのとき$(x,y)=(-2,1)$である。最小値は$-1$で、そのとき$(x,y)=(1,1)$である。
"#;
    let parabola_warnings =
        scan_constrained_two_variable_extremum_structure(parabola_problem, parabola_answer);
    assert!(
        parabola_warnings.is_empty(),
        "放物線を使う正しい図形移動法に警告: {:?}",
        parabola_warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );
    assert!(!parabola_answer.contains("\\clip"));
    assert!(scan_tikz_monochrome(parabola_answer).is_empty());
    assert!(!parabola_answer.contains("red"));
    assert!(!parabola_answer.contains("blue"));
    let colored_figure_answer = parabola_answer.replacen("thick,solid", "thick,red", 1);
    assert!(scan_tikz_monochrome(&colored_figure_answer)
        .iter()
        .any(|warning| warning.code == "TIKZ_COLOR_NOT_MONOCHROME"));
    let clipped_figure_answer = parabola_answer.replacen(
        "\\begin{tikzpicture}[x=1.05cm,y=1.05cm,>=stealth,font=\\small]",
        "\\begin{tikzpicture}[x=1.05cm,y=1.05cm,>=stealth,font=\\small]\n  \\clip (-3,-0.4) rectangle (3,2.6);",
        1,
    );
    assert!(
        scan_constrained_two_variable_extremum_structure(
            parabola_problem,
            &clipped_figure_answer
        )
        .iter()
        .any(|warning| warning.code == "CONSTRAINED_EXTREMUM_TIKZ_CLIPPING")
    );
    let excess_level_curves = parabola_answer.replacen(
        "\\end{tikzpicture}",
        "  \\node at (0.4,2.2) {$k=2$};\n\\end{tikzpicture}",
        1,
    );
    assert!(
        scan_constrained_two_variable_extremum_structure(
            parabola_problem,
            &excess_level_curves
        )
        .iter()
        .any(|warning| warning.code == "CONSTRAINED_EXTREMUM_EXCESS_LEVEL_CURVES")
    );
    for unnecessary in [
        r#"y^2-2x\geqq-1"#,
        r#"k\leqq5"#,
        r#"-1\leqq k\leqq5"#,
        "共有点をもつ範囲",
    ] {
        assert!(
            !parabola_answer.contains(unnecessary),
            "図から明らかな極値に不要な再証明を含めないこと: {unnecessary}"
        );
    }
    assert!(parabola_answer.contains("D=4+4k=0"));
    assert!(!parabola_answer.contains("\\Delta"));

    let unnecessary_range_answer = format!(
        "{parabola_answer}\nさらに、この放物線と領域が共有点をもつ$k$の範囲は$-1\\leqq k\\leqq5$である。"
    );
    assert!(
        scan_constrained_two_variable_extremum_structure(
            parabola_problem,
            &unnecessary_range_answer
        )
        .iter()
        .any(|warning| warning.code == "CONSTRAINED_EXTREMUM_UNNECESSARY_VALUE_RANGE")
    );
    let value_range_problem = format!(
        "{} また、$y^2-2x$のとり得る値の範囲も求めよ。",
        parabola_problem
    );
    assert!(
        scan_constrained_two_variable_extremum_structure(
            &value_range_problem,
            &unnecessary_range_answer
        )
        .iter()
        .all(|warning| warning.code != "CONSTRAINED_EXTREMUM_UNNECESSARY_VALUE_RANGE")
    );

    let redundant_global_proof = format!(
        "{parabola_answer}\nさらに、領域全体で$y^2-2x\\geqq-1$かつ$y^2-2x\\leqq5$を示す。"
    );
    assert!(
        scan_constrained_two_variable_extremum_structure(
            parabola_problem,
            &redundant_global_proof
        )
        .iter()
        .any(|warning| warning.code == "CONSTRAINED_EXTREMUM_REDUNDANT_GLOBAL_PROOF")
    );

    let comparison_needed_answer = r#"
求める式を$k$と置くと$xy=k$であり、これは双曲線を表す。$k$の変化に伴って双曲線の形と位置が変化する。
\begin{tikzpicture}
  \draw[->] (-2,0)--(2,0);
  \draw[->] (0,-2)--(0,2);
\end{tikzpicture}
$xy=k$と条件領域が共有点をもつ限界の候補が複数残るため、各接点と頂点を求める。候補をすべて目的式へ代入して比較し、最大値と最小値を定める。
"#;
    let comparison_problem =
        "$x,y$が複数の不等式を満たす条件のもとで、$xy$の最大値と最小値を求めよ。";
    assert!(scan_constrained_two_variable_extremum_structure(
        comparison_problem,
        comparison_needed_answer
    )
    .is_empty());
    let missing_comparison = comparison_needed_answer.replace(
        "候補をすべて目的式へ代入して比較し、",
        "候補の一部だけを調べ、",
    );
    assert!(
        scan_constrained_two_variable_extremum_structure(
            comparison_problem,
            &missing_comparison
        )
        .iter()
        .any(|warning| {
            warning.code == "CONSTRAINED_EXTREMUM_CANDIDATE_COMPARISON_MISSING"
        })
    );

    let few_shot_output = CONSTRAINED_TWO_VARIABLE_EXTREMUM_INSTRUCTIONS
        .split("【few-shot出力例】")
        .nth(1)
        .and_then(|text| text.split("【few-shot出力例ここまで】").next())
        .expect("最大・最小用few-shotの出力例が存在すること");
    for forbidden in [
        r#"y^2-2x\geqq-1"#,
        r#"k\leqq5"#,
        r#"-1\leqq k\leqq5"#,
    ] {
        assert!(!few_shot_output.contains(forbidden));
    }

    let direct_answer = r#"$y$を固定して計算すると最大値は$5$、最小値は$-5$である。"#;
    let direct_warnings =
        scan_constrained_two_variable_extremum_structure(circle_problem, direct_answer);
    for expected_code in [
        "CONSTRAINED_EXTREMUM_LEVEL_SET_MISSING",
        "CONSTRAINED_EXTREMUM_SHARED_POINT_MISSING",
        "CONSTRAINED_EXTREMUM_MOVEMENT_MISSING",
        "CONSTRAINED_EXTREMUM_DIAGRAM_MISSING",
    ] {
        assert!(
            direct_warnings
                .iter()
                .any(|warning| warning.code == expected_code),
            "検出できない構造違反: {expected_code}"
        );
    }

    let out_of_scope = format!("{geometric_answer}\n偏微分と勾配を用いる。");
    assert!(
        scan_constrained_two_variable_extremum_structure(circle_problem, &out_of_scope)
            .iter()
            .any(|warning| warning.code == "CONSTRAINED_EXTREMUM_OUT_OF_SCOPE_METHOD")
    );
    assert!(scan_constrained_two_variable_extremum_structure(
        "関数$f(x)$の最大値を求めよ。",
        direct_answer
    )
    .is_empty());
}

#[test]
fn trajectory_region_prompt_and_structure_regression() {
    use kyozai_kobo_lib::ai::{
        is_compound_trajectory_region_problem, is_moving_figure_region_problem,
        is_trajectory_region_problem, prefers_swept_region_membership_structure,
        requires_strict_point_locus_structure, scan_solution_notation,
        scan_condition_quote_structure, scan_trajectory_solution_structure,
        should_attach_trajectory_instructions, trajectory_target_point_name,
    };

    let hyperbola_problem = "双曲線$x^2-y^2=2$と直線$y=3x+k$が異なる2点$A,B$で交わるとき、線分$AB$の中点$M$の軌跡を求めよ。";
    assert!(is_trajectory_region_problem(hyperbola_problem));
    assert!(requires_strict_point_locus_structure(hyperbola_problem));
    assert_eq!(trajectory_target_point_name(hyperbola_problem), Some('M'));

    let classification_cases = [
        "媒介変数$t$で表された動点$P$の軌跡を求めよ。",
        "線分上を動く点$P$の軌跡を求めよ。",
        "点$Q$までの距離が一定となる点$M$の軌跡を求めよ。",
        "2点からの距離の和が一定以下となる領域を求めよ。",
        "境界を含む領域を図示せよ。",
        "境界を含まない領域を求めよ。",
        "点$Q$が円弧上を動くときの軌跡を求めよ。",
        "点$C$が動くときの軌跡を求めよ。",
        "円$R$上の動点$Q$の軌跡を求めよ。",
    ];
    for problem in classification_cases {
        assert!(is_trajectory_region_problem(problem), "分類できない問題: {problem}");
    }
    assert!(!is_trajectory_region_problem("関数$y=x^2$の最大値を求めよ。"));
    assert!(should_attach_trajectory_instructions("text", hyperbola_problem));
    assert!(!should_attach_trajectory_instructions(
        "text",
        "関数$y=x^2$の最大値を求めよ。"
    ));
    assert!(should_attach_trajectory_instructions("image", ""));

    let moving_segment_volume_problem = r#"実数$\theta$が動くとき、動点$P(0,\sin\theta)$および$Q(8\cos\theta,0)$を考える。$0\leqq\theta\leqq\frac{\pi}{2}$のとき、平面内で線分$PQ$が通過する部分を$D$とする。$D$を$x$軸のまわりに1回転してできる立体の体積$V$を求めよ。"#;
    assert!(is_trajectory_region_problem(moving_segment_volume_problem));
    assert!(is_moving_figure_region_problem(
        moving_segment_volume_problem
    ));
    assert!(is_compound_trajectory_region_problem(
        moving_segment_volume_problem
    ));
    assert!(prefers_swept_region_membership_structure(
        moving_segment_volume_problem
    ));
    assert!(!requires_strict_point_locus_structure(
        moving_segment_volume_problem
    ));
    assert!(!prefers_swept_region_membership_structure(
        "曲線族の包絡線が囲む領域を求めよ。"
    ));
    let maximum_as_condition =
        "関数の最大値が1となるとき、動点$P$の軌跡を求めよ。";
    assert!(is_trajectory_region_problem(maximum_as_condition));
    assert!(!is_compound_trajectory_region_problem(maximum_as_condition));
    assert!(requires_strict_point_locus_structure(maximum_as_condition));

    let defined_d_point_region =
        "動点$P$の動く範囲を$D$とする。領域$D$を求めよ。";
    let defined_d_answer = r#"求める領域を$D$とし、動点$P$の座標を$P(x,y)$とする。
\[
P(x,y)\in D
\Longleftrightarrow
\left\{
\begin{aligned}
x&\geqq0\\
y&\geqq0
\end{aligned}
\right.
\]
"#;
    assert!(requires_strict_point_locus_structure(
        defined_d_point_region
    ));
    let defined_d_warnings =
        scan_trajectory_solution_structure(defined_d_point_region, defined_d_answer);
    assert!(
        defined_d_warnings.is_empty(),
        "問題文で定義済みの領域Dに警告: {:?}",
        defined_d_warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );
    let unnecessarily_renamed = defined_d_answer.replace("$D$", "$R$").replace("\\in D", "\\in R");
    assert!(scan_trajectory_solution_structure(defined_d_point_region, &unnecessarily_renamed)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_DEFINED_REGION_SYMBOL"));

    for (problem, expected) in [
        ("線分$AB$の中点$M$の軌跡を求めよ。", 'M'),
        ("動点$P$の軌跡を求めよ。", 'P'),
        ("点$Q$の軌跡を求めよ。", 'Q'),
        ("点$C$が動くときの軌跡を求めよ。", 'C'),
    ] {
        assert_eq!(trajectory_target_point_name(problem), Some(expected));
    }

    // 問題文で P(p,q) と定義済みなら、検査側も p,q を保持して受理する。
    let named_coordinate_problem = r#"点$P(p,q)$から楕円$\dfrac{x^2}{4}+y^2=1$に引いた2本の接線が直交するとき、点$P$の軌跡を求めよ。"#;
    let named_coordinate_answer = r#"求める軌跡を$R$とする。
\[
\begin{aligned}
P(p,q)\in R
&\Longleftrightarrow
\text{「問題文の条件」}\\
&\Longleftrightarrow
p^2+q^2=5
\end{aligned}
\]
"#;
    let named_coordinate_warnings =
        scan_trajectory_solution_structure(named_coordinate_problem, named_coordinate_answer);
    assert!(
        !named_coordinate_warnings.iter().any(|warning| matches!(
            warning.code.as_str(),
            "TRAJECTORY_POINT_NAME" | "TRAJECTORY_MISSING_COORDINATE_SETUP"
        )),
        "問題文どおりのP(p,q)を誤検出: {:?}",
        named_coordinate_warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );
    let renamed_coordinates = named_coordinate_answer.replacen("P(p,q)\\in R", "P(x,y)\\in R", 1);
    assert!(scan_trajectory_solution_structure(named_coordinate_problem, &renamed_coordinates)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_POINT_NAME"));

    let snapshot = include_str!("fixtures/trajectory_hyperbola_midpoint.tex");
    let warnings = scan_trajectory_solution_structure(hyperbola_problem, snapshot);
    assert!(
        warnings.is_empty(),
        "回帰スナップショットに構造警告: {:?}",
        warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );
    let notation_warnings = scan_solution_notation(snapshot);
    assert!(
        notation_warnings.is_empty(),
        "回帰スナップショットに表記警告: {:?}",
        notation_warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );
    assert!(snapshot.contains("M(x,y)\\in R"));
    assert!(snapshot.starts_with(
        "求める軌跡を$R$とし、中点$M$の座標を$M(x,y)$とする。"
    ));
    assert!(snapshot.contains("判別式を$D$"));
    assert!(!snapshot.contains("\\Delta"));
    assert!(!snapshot.contains("\\exists"));
    assert!(!snapshot.contains("逆に"));
    assert!(!snapshot.contains("以上を一続きの同値変形でまとめると"));
    assert!(!snapshot.contains("以上の準備のもとで"));
    assert!(snapshot.contains("|x|&>\\frac32"));
    assert!(!snapshot.contains("|y|&>"));
    assert!(!snapshot.contains("すなわち"));
    assert!(snapshot.contains("\\text{「直線 }y=3x+k\\text{ が双曲線 }"));
    assert!(snapshot.contains("\\text{その中点が }M(x,y)\\text{ となる実数 }k\\text{ が存在する」}"));
    assert_eq!(snapshot.matches("「").count(), 3);
    assert_eq!(snapshot.matches("」").count(), 3);
    assert!(snapshot.contains(
        "\\text{「}\\;\n\\left\\{\n\\begin{aligned}\nD&>0"
    ));
    assert!(snapshot.contains("\\text{を満たす実数 }k\\text{ が存在する」}"));

    let compound_snapshot = include_str!("fixtures/moving_segment_rotation_volume.tex");
    let compound_warnings =
        scan_trajectory_solution_structure(moving_segment_volume_problem, compound_snapshot);
    assert!(
        compound_warnings.is_empty(),
        "複合問題の回帰スナップショットに構造警告: {:?}",
        compound_warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );
    let spatial_segment_volume_problem = r#"$xyz$空間で点Pは$x$軸上、点Qは$yz$平面上を動く。
線分PQが通過してできる立体の体積を求めよ。"#;
    let xz_cross_section_snapshot = compound_snapshot
        .replacen("$xy$平面上の任意の点を$X(x,y)$とする。", "$xz$平面上の任意の点を$X(x,z)$とする。", 1)
        .replace("X(x,y)\\in D", "X(x,z)\\in D");
    let xz_warnings = scan_trajectory_solution_structure(
        spatial_segment_volume_problem,
        &xz_cross_section_snapshot,
    );
    assert!(
        xz_warnings.iter().all(|warning| !matches!(
            warning.code.as_str(),
            "TRAJECTORY_SWEPT_MEMBERSHIP"
                | "TRAJECTORY_SWEPT_POINT_SETUP"
                | "TRAJECTORY_PARAMETER_ELIMINATION_FLOW"
        )),
        "xz平面で調べる空間問題をxy平面の答案として誤検出: {:?}",
        xz_warnings
            .iter()
            .map(|warning| (&warning.code, &warning.message))
            .collect::<Vec<_>>()
    );
    assert!(scan_solution_notation(compound_snapshot).is_empty());
    assert!(compound_snapshot.contains("領域$D$"));
    assert!(compound_snapshot.starts_with("$xy$平面上の任意の点を$X(x,y)$とする。"));
    assert!(!compound_snapshot.contains("領域$D$内の任意の点を"));
    assert!(compound_snapshot.contains("X(x,y)\\in D"));
    assert!(compound_snapshot.contains("0&\\leqq t\\leqq1"));
    assert!(compound_snapshot.contains(
        "\\text{「}\\;\n\\left\\{\n\\begin{aligned}\n0&\\leqq\\theta"
    ));
    assert!(compound_snapshot.contains(
        "\\text{を満たす実数 }\\theta,t\\text{ が存在する」}"
    ));
    assert!(compound_snapshot.contains(
        "0\\leqq t\\leqq1\n&\\Longleftrightarrow\n0<\\frac{x}{8\\cos\\theta}\\leqq1"
    ));
    assert!(compound_snapshot.contains(
        "&\\Longleftrightarrow\n0\\leqq\\theta\\leqq\\alpha"
    ));
    assert!(compound_snapshot.matches("X(x,y)\\in D").count() >= 3);
    assert_eq!(
        compound_snapshot
            .matches("\\text{を満たす実数 }\\theta,t\\text{ が存在する」}")
            .count(),
        3
    );
    assert_eq!(
        compound_snapshot
            .matches("\\text{を満たす実数 }\\theta\\text{ が存在する」}")
            .count(),
        2
    );
    assert!(compound_snapshot.contains(
        "t&=\\dfrac{x}{8\\cos\\theta}\\\\\ny&=\\left(1-\\dfrac{x}{8\\cos\\theta}\\right)\\sin\\theta"
    ));
    assert!(compound_snapshot.contains(
        "0&\\leqq\\theta\\leqq\\alpha\\\\\ny&=f_x(\\theta)\n\\end{aligned}\n\\right.\\\\\n\\text{を満たす実数 }\\theta\\text{ が存在する」}"
    ));
    assert!(compound_snapshot.contains("$0<x<8$を固定し"));
    assert!(compound_snapshot.contains(
        "$0\\leqq\\theta\\leqq\\alpha<\\dfrac{\\pi}{2}$では"
    ));
    assert!(compound_snapshot.contains("\\cos^3\\beta=\\frac{x}{8}"));
    assert!(compound_snapshot.contains(
        "\\frac{x}{8}<\\left(\\frac{x}{8}\\right)^{1/3}<1"
    ));
    assert!(compound_snapshot.contains("0<\\beta<\\alpha"));
    assert!(compound_snapshot.contains("f_x'(\\theta)&>0"));
    assert!(compound_snapshot.contains("f_x'(\\beta)&=0"));
    assert!(compound_snapshot.contains("f_x'(\\theta)&<0"));
    assert!(compound_snapshot.contains("\\begin{array}{c|ccccc}"));
    assert!(compound_snapshot.contains("f_x'(\\theta)&&+&0&-&"));
    assert!(compound_snapshot.contains("&\\nearrow&"));
    assert!(compound_snapshot.contains("&\\searrow&0"));
    assert!(compound_snapshot.contains(
        "$f_x(\\theta)$は$\\theta=\\beta$のとき最大となり、最大値は"
    ));
    assert!(compound_snapshot.contains("$f_x$の値域は"));
    let solved_t_position = compound_snapshot
        .find("t&=\\dfrac{x}{8\\cos\\theta}")
        .expect("補間パラメータを表す連立条件があること");
    let first_single_parameter_position = compound_snapshot
        .find("\\text{を満たす実数 }\\theta\\text{ が存在する」}")
        .expect("増減表前にthetaだけの存在条件があること");
    let variation_table_position = compound_snapshot
        .find("\\begin{array}{c|ccccc}")
        .expect("増減表があること");
    let last_single_parameter_position = compound_snapshot
        .rfind("\\text{を満たす実数 }\\theta\\text{ が存在する」}")
        .expect("増減表後にthetaだけの存在条件があること");
    assert!(solved_t_position < first_single_parameter_position);
    assert!(first_single_parameter_position < variation_table_position);
    assert!(variation_table_position < last_single_parameter_position);
    assert!(compound_snapshot.contains(
        "0&\\leqq y\\leqq\n\\left\\{1-\\left(\\dfrac{x}{8}\\right)^{2/3}\\right\\}^{3/2}"
    ));
    assert!(compound_snapshot.contains("V\n=\\pi\\int_0^8"));
    assert!(compound_snapshot.contains("\\frac{128\\pi}{105}"));
    assert!(compound_snapshot.contains("$x=0$では$0\\leqq y\\leqq1$"));
    assert!(compound_snapshot.contains("$x=8$では$t=1$かつ$\\theta=0$"));
    assert!(compound_snapshot.contains("\\Longleftrightarrow"));
    assert!(!compound_snapshot.contains("\\exists"));
    assert!(!compound_snapshot.contains("\\forall"));
    assert!(!compound_snapshot.contains("逆に"));
    assert!(!compound_snapshot.contains("この範囲は十分でもある"));
    assert!(!compound_snapshot.contains("十分性を確認"));
    assert!(!compound_snapshot.contains("この範囲の任意の"));
    assert!(!compound_snapshot.contains("実際に$t$を定めることができる"));
    for forbidden in ["臨界点", "臨界値", "critical point", "critical value"] {
        assert!(!compound_snapshot.contains(forbidden));
    }

    let missing_variation_table = compound_snapshot
        .replacen("\\begin{array}{c|ccccc}", "\\begin{aligned}", 1)
        .replacen("\\end{array}", "\\end{aligned}", 1);
    assert!(scan_solution_notation(&missing_variation_table)
        .iter()
        .any(|warning| warning.code == "MISSING_VARIATION_TABLE"));

    let incomplete_variation_table = compound_snapshot.replacen("\\nearrow", "\\quad", 1);
    assert!(scan_solution_notation(&incomplete_variation_table)
        .iter()
        .any(|warning| warning.code == "INCOMPLETE_VARIATION_TABLE"));

    let renamed_region = compound_snapshot
        .replace("領域$D$", "領域$R$")
        .replace("\\in D", "\\in R");
    assert!(scan_trajectory_solution_structure(moving_segment_volume_problem, &renamed_region)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_DEFINED_REGION_SYMBOL"));

    let stopped_after_region = compound_snapshot
        .split("この領域$D$を$x$軸のまわりに回転した立体の体積$V$は")
        .next()
        .unwrap_or_default();
    assert!(scan_trajectory_solution_structure(moving_segment_volume_problem, stopped_after_region)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_COMPOUND_INCOMPLETE"));
    assert!(!scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        compound_snapshot
    )
    .iter()
    .any(|warning| matches!(
        warning.code.as_str(),
        "TRAJECTORY_MISSING_EQUIVALENCE"
            | "TRAJECTORY_POINT_NAME"
            | "TRAJECTORY_SET_SYMBOL"
            | "TRAJECTORY_SWEPT_MEMBERSHIP"
            | "TRAJECTORY_SWEPT_POINT_SETUP"
            | "TRAJECTORY_SWEPT_ASSUMED_MEMBERSHIP"
            | "TRAJECTORY_SWEPT_PARAMETER_CONDITION"
            | "TRAJECTORY_SWEPT_QUOTED_CONDITION"
            | "TRAJECTORY_PARAMETER_ELIMINATION_FLOW"
    )));

    let missing_solved_parameter_system = compound_snapshot.replacen(
        "t&=\\dfrac{x}{8\\cos\\theta}\\\\",
        "t&\\in\\mathbb{R}\\\\",
        1,
    );
    assert!(scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        &missing_solved_parameter_system
    )
    .iter()
    .any(|warning| warning.code == "TRAJECTORY_PARAMETER_ELIMINATION_FLOW"));

    let missing_single_parameter_stage = compound_snapshot.replacen(
        "\\text{を満たす実数 }\\theta\\text{ が存在する」}",
        "\\text{を満たす実数 }\\theta,t\\text{ が存在する」}",
        1,
    );
    assert!(scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        &missing_single_parameter_stage
    )
    .iter()
    .any(|warning| warning.code == "TRAJECTORY_PARAMETER_ELIMINATION_FLOW"));

    let detached_parameter_elimination = compound_snapshot.replacen(
        "X(x,y)\\in D",
        "X_0(x,y)\\in D",
        2,
    );
    assert!(scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        &detached_parameter_elimination
    )
    .iter()
    .any(|warning| warning.code == "TRAJECTORY_PARAMETER_ELIMINATION_FLOW"));

    let missing_swept_membership = compound_snapshot.replacen(
        "X(x,y)\\in D\n&\\Longleftrightarrow",
        "X(x,y)\\in D\n&\\Longrightarrow",
        1,
    );
    assert!(scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        &missing_swept_membership
    )
    .iter()
    .any(|warning| warning.code == "TRAJECTORY_SWEPT_MEMBERSHIP"));

    let missing_interpolation_range = compound_snapshot.replacen(
        "0&\\leqq t\\leqq1\\\\",
        "t&\\in\\mathbb{R}\\\\",
        1,
    );
    assert!(scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        &missing_interpolation_range
    )
    .iter()
    .any(|warning| warning.code == "TRAJECTORY_SWEPT_PARAMETER_CONDITION"));

    let unquoted_existence = compound_snapshot
        .replace(
            "\\text{「}\\;\n\\left\\{",
            "\\left\\{",
        )
        .replace(
            "\\text{を満たす実数 }\\theta,t\\text{ が存在する」}",
            "\\text{を満たす実数 }\\theta,t\\text{ が存在する}",
        );
    assert!(scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        &unquoted_existence
    )
    .iter()
    .any(|warning| warning.code == "TRAJECTORY_SWEPT_QUOTED_CONDITION"));

    let membership_assumed_at_opening = compound_snapshot.replacen(
        "$xy$平面上の任意の点を$X(x,y)$とする。",
        "領域$D$内の任意の点を$X(x,y)$とする。",
        1,
    );
    assert!(scan_trajectory_solution_structure(
        moving_segment_volume_problem,
        &membership_assumed_at_opening
    )
    .iter()
    .any(|warning| warning.code == "TRAJECTORY_SWEPT_ASSUMED_MEMBERSHIP"));

    let x_name_collision_problem =
        "動点$P,X$を結ぶ線分$PX$が通過する部分を$D$とする。領域$D$を求めよ。";
    let x_name_collision_answer = r#"$xy$平面上の任意の点を$Y(x,y)$とする。
\[
\begin{aligned}
Y(x,y)\in D
&\Longleftrightarrow
\begin{gathered}
\text{「}\;
\left\{
\begin{aligned}
0&\leqq s\leqq1\\
0&\leqq t\leqq1\\
x&=t\\
y&=s(1-t)
\end{aligned}
\right.\\
\text{を満たす実数 }s,t\text{ が存在する」}
\end{gathered}
\end{aligned}
\]
"#;
    assert!(
        scan_trajectory_solution_structure(x_name_collision_problem, x_name_collision_answer)
            .is_empty(),
        "問題文でXが使用済みなら補助点Yを選ぶ"
    );

    let valid_condition_quote_examples = [
        r#"\begin{gathered}
\text{「①と②が異なる2点 }A,B\text{ で交わり，}\\
\text{線分 }AB\text{ の中点が }M(x,y)\text{ となる}\\
\text{実数 }k\text{ が存在する」}
\end{gathered}"#,
        r#"\text{「}y=f(\theta)\text{ となる実数 }\theta\text{ が存在する」}"#,
        r#"\text{「}0\leqq t\leqq1\text{ を満たす実数 }t\text{ が存在する」}"#,
        r#"\begin{gathered}
\text{「}\;
\left\{
\begin{aligned}
D&>0\\
x&=-\frac{3k}{8}\\
y&=-\frac{k}{8}
\end{aligned}
\right.\\
\text{を満たす実数 }k\text{ が存在する」}
\end{gathered}"#,
        r#"\text{「点 }P(x,y)\text{ が円 }C\text{ の内部にある」}"#,
    ];
    for example in valid_condition_quote_examples {
        assert!(
            scan_condition_quote_structure(example).is_empty(),
            "正しい条件全体の鉤括弧を誤検出: {example}"
        );
    }

    let invalid_condition_quote_examples = [
        (
            r#"0\leqq t\leqq1\quad\text{「を満たす実数 }t\text{ が存在する」}"#,
            "CONDITION_QUOTE_SCOPE",
        ),
        (
            r#"y=f(\theta)\quad\text{「となる実数 }\theta\text{ が存在する」}"#,
            "CONDITION_QUOTE_SCOPE",
        ),
        (
            r#"\text{「点が円の内部にある」}\quad P(x,y),C"#,
            "CONDITION_QUOTE_SCOPE",
        ),
        (
            r#"\text{「これらを満たす実数が存在する」 }k"#,
            "CONDITION_QUOTE_SCOPE",
        ),
        (
            r#"\begin{gathered}
\text{「①と②が異なる2点 }A,B\text{ で交わる」}\\
\text{「線分 }AB\text{ の中点が }M(x,y)\text{ となる」}\\
\text{「実数 }k\text{ が存在する」}
\end{gathered}"#,
            "CONDITION_QUOTE_MULTIPLE_PAIRS",
        ),
        (
            r#"\begin{gathered}
\left\{
\begin{aligned}
D&>0\\
x&=-\frac{3k}{8}\\
y&=-\frac{k}{8}
\end{aligned}
\right.\\
\text{「これらを満たす実数 }k\text{ が存在する」}
\end{gathered}"#,
            "CONDITION_QUOTE_SCOPE",
        ),
        (
            r#"\left\{\begin{aligned}\text{「}D&>0\text{」}\\x&=1\end{aligned}\right."#,
            "CONDITION_QUOTE_BRACED_SYSTEM",
        ),
        (
            r#"\text{「点 \(P(x,y)\) が円 \(C\) の内部にある」}"#,
            "CONDITION_QUOTE_MATH_MODE",
        ),
        (
            r#"\text{「}\quad\text{点 }P(x,y)\text{ が円 }C\text{ の内部にある}\quad\text{」}"#,
            "CONDITION_QUOTE_SCOPE",
        ),
    ];
    for (example, expected_code) in invalid_condition_quote_examples {
        assert!(
            scan_condition_quote_structure(example)
                .iter()
                .any(|warning| warning.code == expected_code),
            "誤った鉤括弧構造を検出できない: {example}"
        );
    }

    let wrong_point = snapshot.replacen("M(x,y)\\in R", "P(x,y)\\in R", 1);
    assert!(scan_trajectory_solution_structure(hyperbola_problem, &wrong_point)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_POINT_NAME"));

    let missing_coordinate_setup = snapshot.replacen(
        "求める軌跡を$R$とし、中点$M$の座標を$M(x,y)$とする。",
        "求める軌跡を$R$とする。",
        1,
    );
    assert!(scan_trajectory_solution_structure(hyperbola_problem, &missing_coordinate_setup)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_MISSING_COORDINATE_SETUP"));
    assert!(scan_trajectory_solution_structure("", &missing_coordinate_setup)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_MISSING_COORDINATE_SETUP"));

    let delta = snapshot.replacen("D=(6k)^2", "\\Delta=(6k)^2", 1);
    assert!(scan_trajectory_solution_structure(hyperbola_problem, &delta)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_DISCRIMINANT_SYMBOL"));

    let posthoc = format!("{}\n以上を一続きの同値変形でまとめると、次のようになる。", snapshot);
    assert!(scan_trajectory_solution_structure(hyperbola_problem, &posthoc)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_POSTHOC_EQUIVALENCE"));

    for unnecessary_preface in [
        "以上の準備のもとで、軌跡の条件を同値変形すると",
        "以上の準備のもとで,軌跡の条件を同値変形すると",
    ] {
        let with_preface = snapshot.replacen(
            "\\[\n\\begin{aligned}\nM(x,y)\\in R",
            &format!("{}\n\\[\n\\begin{{aligned}}\nM(x,y)\\in R", unnecessary_preface),
            1,
        );
        assert!(scan_trajectory_solution_structure(hyperbola_problem, &with_preface)
            .iter()
            .any(|warning| warning.code == "TRAJECTORY_POSTHOC_EQUIVALENCE"));
    }

    let split_proof = format!("{}\n逆に、この条件から十分性を確認する。", snapshot);
    assert!(scan_trajectory_solution_structure(hyperbola_problem, &split_proof)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_SPLIT_NECESSITY_SUFFICIENCY"));

    let bad_existence_layout = r#"
求める軌跡を$R$とする。
\[
\begin{aligned}
M(x,y)\in R
&\Longleftrightarrow
|k|>4,\quad x=-\frac{3k}{8},\quad y=-\frac{k}{8}
\text{を満たす実数 }k\text{ が存在する}
\end{aligned}
\]
"#;
    assert!(scan_trajectory_solution_structure(hyperbola_problem, bad_existence_layout)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_EXISTENCE_LAYOUT"));

    let redundant_conclusion = format!("{}\nすなわち、これは2本の半直線である。", snapshot);
    assert!(scan_trajectory_solution_structure(hyperbola_problem, &redundant_conclusion)
        .iter()
        .any(|warning| warning.code == "TRAJECTORY_REDUNDANT_CONCLUSION"));

    let structural_cases = [
        ("媒介変数$t$で表された動点$P$の軌跡を求めよ。", 'P', "R"),
        ("線分上を動く点$Q$の軌跡を求めよ。", 'Q', "R"),
        ("距離の等式を2乗して点$M$の軌跡を求めよ。", 'M', "R"),
        ("距離の不等式から点$P$の存在領域を求めよ。", 'P', "R"),
        ("境界を含む点$Q$の領域を求めよ。", 'Q', "R"),
        ("境界を含まない点$C$の領域を求めよ。", 'C', "R"),
        ("円弧上を動く点$M$の軌跡を求めよ。", 'M', "R"),
        ("円$R$上を動く点$Q$の軌跡を求めよ。", 'Q', "S"),
    ];
    for (problem, point, set_name) in structural_cases {
        let answer = format!(
            "求める軌跡を${set_name}$とし、点${point}$の座標を${point}(x,y)$とする。\\[\\begin{{aligned}}{point}(x,y)\\in {set_name}&\\Longleftrightarrow\\text{{問題文の条件}}\\\\&\\Longleftrightarrow\\text{{最終条件}}\\end{{aligned}}\\]"
        );
        assert!(
            scan_trajectory_solution_structure(problem, &answer).is_empty(),
            "構造ケースで警告: {problem}"
        );
    }
}

#[test]
fn explanation_must_follow_reference_answer_structure() {
    use kyozai_kobo_lib::ai::scan_explanation_reference_alignment;

    let input = r#"【問題文】
双曲線$x^2-y^2=2$と直線$y=3x+k$が異なる2点$A,B$で交わるとき、線分$AB$の中点$M$の軌跡を求めよ。

【参照する解答】
求める軌跡を$R$とし、中点$M$の座標を$M(x,y)$とする。
\[
\begin{aligned}
M(x,y)\in R
&\Longleftrightarrow \text{問題文の条件を満たす実数 }k\text{ が存在する}\\
&\Longleftrightarrow
\left\{
\begin{aligned}
|k|&>4\\
x&=-\frac{3k}{8}\\
y&=-\frac{k}{8}
\end{aligned}
\right.\\
&\Longleftrightarrow
\left\{
\begin{aligned}
x&=3y\\
|x|&>\frac32
\end{aligned}
\right.
\end{aligned}
\]
したがって、求める軌跡は直線$x=3y$上で$|x|>\frac32$を満たす部分である。"#;

    let aligned = r#"\textbf{【着眼点】}\par
参照する解答では、判別式と解と係数の関係で中点を$k$によって表し、その存在条件を消去している。
\textbf{【定石】}\par
軌跡は、求める点がその集合に属する条件から必要十分条件を保って媒介変数を消去する。
\[
\begin{aligned}
M(x,y)\in R
&\Longleftrightarrow \text{問題文の条件を満たす実数 }k\text{ が存在する}\\
&\Longleftrightarrow
\left\{\begin{aligned}|k|&>4\\x&=-\frac{3k}{8}\\y&=-\frac{k}{8}\end{aligned}\right.\\
&\Longleftrightarrow
\left\{\begin{aligned}x&=3y\\|x|&>\frac32\end{aligned}\right.
\end{aligned}
\]
第1の同値変形では交点が異なる2点となる条件と中点座標をまとめ、第2の同値変形で$k$を消去している。"#;
    assert!(scan_explanation_reference_alignment(input, aligned).is_empty());

    let ordinary_caution = format!(
        "{}\n\\textbf{{確認}}\\par\n内分公式の係数の順序を逆にしないことが重要である。",
        aligned
    );
    assert!(
        scan_explanation_reference_alignment(input, &ordinary_caution).is_empty(),
        "一般的な注意の『逆にしない』を逆向きの証明と誤判定しないこと"
    );

    let drifted = r#"\textbf{【定石】}\par
通常の一方向の計算で$x=3y$と$|x|>\frac32$を得る。
\textbf{【逆向きの確認】}\par
逆に、この条件を満たす点をとり、十分性を確認する。
以上より、$M(x,y)\in R\Longleftrightarrow x=3y,\ |x|>\frac32$である。
端点は判別式が0となるので軌跡に含まれない。"#;
    let warnings = scan_explanation_reference_alignment(input, drifted);
    for code in [
        "EXPLANATION_REFERENCE_PROOF_DRIFT",
        "EXPLANATION_REFERENCE_EQUIVALENCE_LOST",
        "EXPLANATION_REFERENCE_ADDED_ENDPOINT_CHECK",
    ] {
        assert!(
            warnings.iter().any(|warning| warning.code == code),
            "参照解答からの逸脱を検出すること: {code}"
        );
    }
}

#[test]
fn topic_method_guide_structure_regression() {
    use kyozai_kobo_lib::ai::scan_topic_method_guide_structure;

    let guide = r#"
\textbf{【概要】}\par
2次関数の最大・最小を扱う。
\textbf{【基本事項】}\par
$y=a(x-p)^2+q$では頂点は$(p,q)$である。
\textbf{【定石】}\par
定義域と頂点の位置に着目する。
\textbf{【手順】}\par
平方完成し、頂点と端点の値を比較する。
\textbf{【典型例】}\par
$0\leqq x\leqq2$での値を調べる。
\textbf{【よくある誤り】}\par
定義域を確認せず頂点の値だけを採用しない。
"#;
    assert!(scan_topic_method_guide_structure(guide).is_empty());

    let missing = guide.replace(
        "\\textbf{【典型例】}\\par\n$0\\leqq x\\leqq2$での値を調べる。\n",
        "",
    );
    assert!(scan_topic_method_guide_structure(&missing)
        .iter()
        .any(|warning| warning.code == "TOPIC_GUIDE_MISSING_SECTIONS"));

    let duplicated = format!("{}\n\\textbf{{【定石】}}\\par\n別の定石。", guide);
    assert!(scan_topic_method_guide_structure(&duplicated)
        .iter()
        .any(|warning| warning.code == "TOPIC_GUIDE_DUPLICATE_SECTIONS"));

    let wrong_order = guide
        .replace("【基本事項】", "【一時見出し】")
        .replace("【定石】", "【基本事項】")
        .replace("【一時見出し】", "【定石】");
    assert!(scan_topic_method_guide_structure(&wrong_order)
        .iter()
        .any(|warning| warning.code == "TOPIC_GUIDE_SECTION_ORDER"));
}

#[test]
fn ai_problem_bank_output_supports_multiple_problems_and_rejects_bad_sources() {
    use kyozai_kobo_lib::ai::{
        output_schema, source_revision_prompt, validate_output, BEGINNER_SOLUTION_INSTRUCTIONS,
        BEGINNER_TOPIC_METHOD_GUIDE_INSTRUCTIONS, FIXED_INSTRUCTIONS,
        CONTENT_REVIEW_FIXED_INSTRUCTIONS,
        DUAL_PROBLEM_LAYOUT_INSTRUCTIONS,
        PROJECT_REVIEW_FIXED_INSTRUCTIONS,
        SOLUTION_FIXED_INSTRUCTIONS, SOURCE_REVISION_FIXED_INSTRUCTIONS,
        TIKZ_GENERATION_INSTRUCTIONS,
        TOPIC_METHOD_GUIDE_INSTRUCTIONS,
        SINGLE_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS, SINGLE_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS,
        SOLUTION_REFERENCE_PROFILE,
        TRAJECTORY_REGION_INSTRUCTIONS, TWO_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS,
        TWO_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS,
    };

    let valid = json!({
        "schemaVersion": 1,
        "detectedType": "problem",
        "latex": "問題A\\par\\medskip 問題B",
        "plainText": "問題A 問題B",
        "requiredPackages": [],
        "warnings": [],
        "uncertainFragments": [],
        "segments": [],
        "suggestedInsertTarget": "problem_body",
        "problems": [
            {"title": "二次関数", "statementLatex": "$y=x^2$について答えよ。", "statementLatexTwoColumn": "$y=x^2$ について\\par 答えよ。", "sourceImageIndexes": [1]},
            {"title": "確率", "statementLatex": "さいころを2回投げる。", "statementLatexTwoColumn": "さいころを2回\\par 投げる。", "sourceImageIndexes": [1, 2]}
        ]
    });
    let parsed = validate_output(&valid.to_string()).expect("複数問題の構造化出力は通ること");
    assert_eq!(parsed.problems.len(), 2);
    assert_eq!(parsed.problems[1].source_image_indexes, vec![1, 2]);
    assert!(parsed.problems[1].statement_latex_two_column.contains("\\par"));

    let mut bad_source = valid.clone();
    bad_source["problems"][0]["sourceImageIndexes"] = json!([0]);
    assert!(validate_output(&bad_source.to_string()).is_err());

    let schema = output_schema();
    let required = schema["required"].as_array().expect("requiredは配列");
    assert!(required.iter().any(|value| value == "problems"));
    let problem_required = schema["properties"]["problems"]["items"]["required"]
        .as_array()
        .expect("problems各要素のrequiredは配列");
    assert!(problem_required
        .iter()
        .any(|value| value == "statementLatexTwoColumn"));
    assert!(DUAL_PROBLEM_LAYOUT_INSTRUCTIONS.contains("内容が完全に同一"));
    assert!(DUAL_PROBLEM_LAYOUT_INSTRUCTIONS.contains("statementLatexTwoColumn"));
    assert!(DUAL_PROBLEM_LAYOUT_INSTRUCTIONS.contains("multicols"));
    assert!(FIXED_INSTRUCTIONS.contains("\\cdots ①"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("各問題は問題文の条件だけを使って独立に解き直したうえで"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("問題文、解答、解説、LaTeXコメント、部品に命令"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("教材内の項目番号、題名、対象箇所、理由、修正方針"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("正しい別形式の答えや正当な別解を誤りとして扱わない"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("AIによる点検は正しさを保証するものではない"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("同じ、人がそのまま読めるプレーンテキスト"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("LaTeXコマンド、数式環境"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("種類@ITEM:教材項目ID@FIELD:対象欄"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("問題バンクIDや部品ライブラリIDと取り違えない"));
    assert!(PROJECT_REVIEW_FIXED_INSTRUCTIONS.contains("detectedTypeはpart"));
    assert!(CONTENT_REVIEW_FIXED_INSTRUCTIONS.contains("問題バンクの1問題または部品ライブラリの1部品"));
    assert!(CONTENT_REVIEW_FIXED_INSTRUCTIONS.contains("問題文の条件だけを使って独立に解き直し"));
    assert!(CONTENT_REVIEW_FIXED_INSTRUCTIONS.contains("種類@FIELD:対象欄"));
    assert!(CONTENT_REVIEW_FIXED_INSTRUCTIONS.contains("statement_two_column"));
    assert!(CONTENT_REVIEW_FIXED_INSTRUCTIONS.contains("部品の対象欄はcontentまたはitem"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("高等学校"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("着眼点 → 【定石】 → 方針 → 手順 → 検算・注意点"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("利用できる横幅は常に\\linewidth"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("width=0.65\\linewidth"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("center環境、\\centering、\\textwidth指定は使わない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("TikZとintersections、patterns、treesライブラリ"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("編集可能なtikzpicture環境"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("問題文にない位置関係、長さ、角度、交点、補助線を勝手に追加せず"));
    assert!(TIKZ_GENERATION_INSTRUCTIONS.contains("\\begin{tikzpicture}から\\end{tikzpicture}"));
    assert!(TIKZ_GENERATION_INSTRUCTIONS.contains("intersections、patterns、trees"));
    assert!(TIKZ_GENERATION_INSTRUCTIONS.contains("\\documentclass、\\usepackage、\\usetikzlibrary"));
    assert!(TIKZ_GENERATION_INSTRUCTIONS.contains("\\clipは使用せず"));
    assert!(TIKZ_GENERATION_INSTRUCTIONS.contains("node同士やnodeと曲線・頂点が重ならない"));
    assert!(TIKZ_GENERATION_INSTRUCTIONS.contains("黒・白・グレーだけのモノクロ"));
    assert!(TIKZ_GENERATION_INSTRUCTIONS.contains("実線、破線、点線"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("AI生成のTikZ図では\\clipを使用しない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("黒・白・グレーだけのモノクロ"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("red、blue、green"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("図中では点名だけにする"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("主解法を含めて最大3つ"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("【参照する解答】"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("解答にない別解へ勝手に切り替えたり追加したりしない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("参照する解答を、解説全体の唯一の論証の骨格"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("参照する解答と別の構成で問題を最初から解き直したり"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("逆向きの確認・端点確認・除外点の列挙"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("同値変形を論証の中心に置き"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("端点を含まないことや判別式が0になる場合を結論後に重複"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("高校数学で標準的か判断が分かれる記号"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$a\\mid b$ は「$b$が$a$で割り切れる」"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("日本の高校の教科書・授業で一般的なもの"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("方程式を満たす値は必ず「解」と呼び、「根」と呼ばない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("「異なる2実根」ではなく「異なる2つの実数解」"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("「平方根」「立方根」「$n$乗根」「根号」"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("ユーザーから「解答の方針」"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("ユーザーから「解説内容の指示」"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("解説する箇所、説明の詳しさ、観点、強調点、つまずきやすい点"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("未指定部分でも論理を追うために必要な説明"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("論理を飛躍させない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("複雑な因数分解、置換後の式、場合分けの条件などを突然提示せず"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("その操作が正当である理由"));
    assert!(!SOLUTION_FIXED_INSTRUCTIONS.contains("【軌跡・領域問題専用の解答規則】"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("【軌跡・領域問題専用の解答規則】"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("【最初に問題の型を判定する】"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "2と3でも、$xy$平面上の任意の点の領域への所属条件をパラメータの存在条件として自然に表せる場合"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("問題文で中点が$M$なら$M(x,y)$"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "求める軌跡を$R$とし、中点$M$の座標を$M(x,y)$とする"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "後で使用する求める点の座標は、必要最小限の準備計算より前に必ず設定"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("形式統一のために$P$など別の点名へ変更してはいけません"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("判別式には必ず$D$"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("判別式に$\\Delta$を使用してはいけません"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "判別式を使用しないなら、領域を最後まで$D$と表してください"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "実際に存在しない衝突を避けるための改名は禁止"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("準備計算の段階で、求める軌跡・領域の最終的な$x,y$の条件まで導いてはいけません"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("M(x,y)\\in R"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("同値変形は解答末尾の要約ではなく、解答本体そのもの"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "以上の準備のもとで、軌跡の条件を同値変形すると"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "最終的な範囲が1文字だけの不等式となり、$x$でも$y$でも同程度に簡潔に書ける場合"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("$|x|>\\dfrac32$"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("以上を一続きの同値変形でまとめると"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("$\\exists$、$\\forall$などの量化記号を使用してはいけません"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("\\left\\{"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("存在文は連立条件の下の行へ置き"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("各条件の行末にもコンマを付けない"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("異なる2つの実数解をもつ$\\Longleftrightarrow D>0$"));
    assert!(!TRAJECTORY_REGION_INSTRUCTIONS.contains("異なる2実根"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("結論後に「すなわち」"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("【動く線分・図形が通過する領域】"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "領域への所属をパラメータの存在条件として記述するために$xy$平面上の任意の点を置くことは、必要な記号の導入"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "特に動く線分では、補間パラメータによって線分上の条件を正確に保持できる"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("0&\\leqq t\\leqq1"));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "\\text{「}\\;\n\\left\\{"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "\\text{を満たす実数 }s,t\\text{ が存在する」}"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "【条件全体を1組の鉤括弧で囲む】"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "日本語と数式、点名、図形名、変数、不等式などが組み合わさって論理的に1つの条件"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "\\text{「}y=f(\\theta)\\text{ となる実数 }\\theta\\text{ が存在する」}"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "開き鉤括弧を左波括弧の直前に置き、連立式と存在文の全体を1組で囲んでください"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "冒頭で「$xy$平面上の任意の点を$X(x,y)$とする。"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "「領域$D$内の任意の点を$X(x,y)$とする」"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "【複数パラメータを1文字ずつ消去する】"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "どの文字がどの段階で消去されたかが見える同値変形へ戻してください"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "t&=T_x(s)"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "\\text{を満たす実数 }s\\text{ が存在する」}"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "増減表の前には第3段階の$s$だけの存在条件へ到達"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "増減表の後は同じ条件を値域へ変形するために1回だけ再掲"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "0\\leqq t\\leqq1\n&\\Longleftrightarrow\n0<\\frac{x}{8\\cos\\theta}\\leqq1"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "この範囲は十分でもある"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "線分上の条件から得られる正しい定義域"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "$0\\leqq\\theta\\leqq\\cos^{-1}\\dfrac{x}{8}$"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "$\\cos^{-1}$そのものを微分してはいけません"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "$x=0$と$0<x\\leqq8$を分ける"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "【値域によるパラメータ消去のfew-shot】"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "t&=\\dfrac{x}{8\\cos\\theta}"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "y&=\\left(1-\\dfrac{x}{8\\cos\\theta}\\right)\\sin\\theta"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "\\text{を満たす実数 }\\theta\\text{ が存在する」}"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "この計算だけで所属条件を離れず、次の連続した同値変形へ必ず反映"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "増減表前に到達した$\\theta$だけの存在条件を、$\\theta$を消去するための起点として1回だけ再掲"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "$\\cos^3\\beta=\\dfrac{x}{8}$"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "$0<\\beta<\\alpha$を確認"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "\\begin{array}{c|ccccc}"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "f_x'(\\theta)&&+&0&-&"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "定義域、導関数、導関数が0になる点の定義と区間内確認、各区間での符号、関数値、増減表、最大値と値域の順"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "$f_x(\\theta)$は$\\theta=\\beta$のとき最大となり、その最大値は"
    ));
    let range_few_shot = TRAJECTORY_REGION_INSTRUCTIONS
        .split("【値域によるパラメータ消去のfew-shot】")
        .nth(1)
        .and_then(|text| text.split("【領域決定後に最終計算がある複合問題】").next())
        .expect("値域のfew-shotが存在すること");
    for forbidden in ["臨界点", "臨界値", "critical point", "critical value"] {
        assert!(!range_few_shot.contains(forbidden));
    }
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "V=\\pi\\int_0^8"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "【領域決定後に最終計算がある複合問題】"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "領域を求めた時点で解答を終了してはいけません"
    ));
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains(
        "$V=\\pi\\int f(x)^2\\,dx$"
    ));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("自力で同じ流れを再現できる粒度"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("計算量や場合分けを減らせる場合は、その方法を積極的に選んで"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("通常の計算より何を省けるか"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("同型問題にも応用できる判断の仕方"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("特殊で分かりにくい技巧を使わず"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("「差商」という用語は使用しない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$x=a$から$x=a+h$までの平均変化率"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("有限の$h$に対する平均変化率と、その極限である微分係数を区別"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("二項係数は、日本の高校数学で一般的な${}_n\\mathrm{C}_r$"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$\\binom{n}{r}$、$\\dbinom{n}{r}$、$\\tbinom{n}{r}$"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("${}_5\\mathrm{C}_2$"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("{}_n\\mathrm{C}_r="));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("\\frac{n!}{r!(n-r)!}"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("\\arcsin、\\arccos、\\arctan等のarcを付けた関数名は高校範囲外"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("必ず$x=\\sin y$と書き直してから"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$1=\\cos y\\dfrac{dy}{dx}$"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$x=\\cos y$、$x=\\tan y$へ戻し"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("必ず$\\leqq$と$\\geqq$を使用"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("元の式がすでに短く十分に扱いやすい場合"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("わずかに短くするだけのために新しい文字へ置き換えず"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("置換によって何が簡単になったか"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("試験で提出する答案を基準"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("採点官が前後の式のつながりと用いた根拠を確認できる"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("暗算で一段階に確認できる自明な四則計算"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("\\boxed、\\fbox、\\framebox等で囲んだり"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("必ず独立した見出し「【定石】」"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("数式の末尾や数式を閉じた直後にASCIIのピリオド"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("ベクトルは太字ではなく、必ず矢印付き"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$\\vec{a}$、2点を結ぶ有向線分は$\\overrightarrow{AB}$"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("\\mathbf、\\boldsymbol、\\bm、\\pmb等"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("点名と座標の組を等号で結ばない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$AB$の中点を$M(x,y)$とする"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("点$A(1,2)$"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("各行末にコンマを付けない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("下の式$\\leqq$対象の式$\\leqq$上の式"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("別々の不等式へ分けず"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("関数を微分して増減、極値、最大・最小"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains(
        "1変数の二次関数の最大・最小、値域、増減を調べるために微分してはいけません"
    ));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains(
        "平方完成して軸と頂点を求め、軸が定義域に含まれるかを確認"
    ));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains(
        "区分的な二次関数でも、各式を平方完成し、軸と各区間の位置関係"
    ));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("導関数の符号変化と結論の対応が見やすくなるときに増減表"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains(
        "1変数関数の値域または最大・最小を導関数の正負と符号変化から求める場合"
    ));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains(
        "文章で「増加し、その後減少する」と述べるだけで終えず"
    ));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains(
        "「臨界点」「臨界値」「critical point」「critical value」は高校数学の答案・解説では使用しない"
    ));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("先に導関数を求め"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("表は論証の代わりではなく"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("二段組では列数と記述を絞って\\linewidth内"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("増減表を加えても理解が改善しない場合は無理に入れない"));
    assert!(TWO_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("二段組の片方の列"));
    assert!(TWO_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("各行が単独で列幅に収まる"));
    assert!(TWO_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("所属式を1行目へ単独"));
    assert!(TWO_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("&\\Longleftrightarrow"));
    assert!(SINGLE_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("\\linewidthの横幅を活かし"));
    assert!(SINGLE_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("超えそうな場合"));
    assert!(TWO_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("問題文は二段組の片方の狭い列"));
    assert!(TWO_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("問題の条件、数値、記号、点名、小問、選択肢"));
    assert!(TWO_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("multicols、twocolumn、columns環境を追加してはいけません"));
    assert!(TWO_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("各行を単独で\\linewidth内"));
    assert!(TWO_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("原稿画像の改行が印刷上の都合"));
    assert!(SINGLE_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("一段組の広い本文"));
    assert!(SINGLE_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("横幅を活かして"));
    assert!(SINGLE_COLUMN_PROBLEM_LAYOUT_INSTRUCTIONS.contains("場合だけ、等号、不等号、演算子"));
    assert!(BEGINNER_SOLUTION_INSTRUCTIONS.contains("数学が苦手な高校生"));
    assert!(BEGINNER_SOLUTION_INSTRUCTIONS.contains("基本事項は省略しない"));
    assert!(BEGINNER_SOLUTION_INSTRUCTIONS.contains("非自明な変形を一段ずつ"));
    assert!(BEGINNER_SOLUTION_INSTRUCTIONS.contains("同じ手順を自分で再現できる"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("問題と解答・研究問題の完成解答調"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("板書・授業ノート調"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("問題別の追加指示がある場合は、その構成を優先"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("答えを枠で囲んだり"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("覚えるべき手法・知識、その手法を選ぶ目印"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("必要に応じて増減表で区間ごとの増減と関数値"));
    assert!(!SOLUTION_REFERENCE_PROFILE.contains("必要に応じて末尾へ「（答）」"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("単一の問題文ではなく"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("【概要】"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("【基本事項】"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("【定石】"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("【手順】"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("【典型例】"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("【よくある誤り】"));
    assert!(TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("detectedTypeはpart"));
    assert!(BEGINNER_TOPIC_METHOD_GUIDE_INSTRUCTIONS.contains("数学が苦手な高校生"));
    assert!(SOURCE_REVISION_FIXED_INSTRUCTIONS.contains("指定されていない文章、数値、条件"));
    assert!(SOURCE_REVISION_FIXED_INSTRUCTIONS.contains("修正後の対象ソースだけ"));
    assert!(SOURCE_REVISION_FIXED_INSTRUCTIONS.contains("参考情報として示された問題文・解答・解説"));
    let revision_prompt = source_revision_prompt(
        "problem_answer",
        "式①から式②への変形を1行補い、ほかは維持する",
    )
    .expect("解答ソースの修正プロンプトを作成できること");
    assert!(revision_prompt.contains("修正対象は解答です"));
    assert!(revision_prompt.contains("指定されていない箇所は変更しない"));
    assert!(revision_prompt.contains("detectedTypeはanswer"));
    assert!(revision_prompt.contains("suggestedInsertTargetはanswer"));
    assert!(source_revision_prompt("unknown", "誤字を直す").is_err());
    assert!(source_revision_prompt("part", "   ").is_err());
}

#[test]
fn project_review_accepts_a_full_material_sized_text_input() {
    use kyozai_kobo_lib::ai::max_input_text_chars;

    assert_eq!(max_input_text_chars("project_review"), 200_000);
    assert_eq!(max_input_text_chars("content_review"), 100_000);
    assert_eq!(max_input_text_chars("revise_source"), 60_000);
    assert_eq!(max_input_text_chars("generate_answer"), 20_000);
}

#[test]
fn content_review_requires_a_saved_problem_or_part_target() {
    use kyozai_kobo_lib::ai::{create_job, CreateJobPayload};

    let (_dir, state) = make_state();
    let part_id = kyozai_kobo_lib::commands::parts::create_part(
        &state,
        "確認対象の部品".into(),
    )
    .unwrap();
    let payload = |source_type: &str, entity_type: &str, entity_id: i64, version: i64| {
        CreateJobPayload {
            source_type: source_type.into(),
            conversion_mode: Some("content_review".into()),
            options: Some(json!({
                "contentReviewSourceVersion": version,
                "contentReviewEntityType": entity_type,
                "solutionSubject": "mathematics"
            })),
            input_text: Some("【対象種別】部品\n【部品本文LaTeX】\n$x^2$".into()),
            input_names: vec![],
            target_entity_type: Some(entity_type.into()),
            target_entity_id: Some(entity_id),
            target_field: Some("review".into()),
        }
    };

    let image_error = create_job(&state, payload("image", "part", part_id, 1))
        .expect_err("AIチェックは画像入力を受け付けないこと");
    assert!(image_error.contains("テキスト入力"));

    let target_error = create_job(&state, payload("text", "template", part_id, 1))
        .expect_err("問題・部品以外を対象にできないこと");
    assert!(target_error.contains("問題または部品"));

    let stale_error = create_job(&state, payload("text", "part", part_id, 99))
        .expect_err("古い版の内容をAIチェックへ送らないこと");
    assert!(stale_error.contains("対象が更新"));
}

#[test]
fn content_review_warning_codes_are_mapped_to_editable_fields() {
    use kyozai_kobo_lib::ai::{
        normalize_content_review_warning_codes, AiWarning,
    };

    let mut problem_warnings = vec![
        AiWarning {
            code: "MATH_ERROR@FIELD:answer".into(),
            severity: "error".into(),
            message: "計算を確認".into(),
        },
        AiWarning {
            code: "UNKNOWN_FIELD@FIELD:metadata".into(),
            severity: "warning".into(),
            message: "分類を確認".into(),
        },
    ];
    normalize_content_review_warning_codes(&mut problem_warnings, "problem");
    assert_eq!(problem_warnings[0].code, "MATH_ERROR@FIELD:answer");
    assert_eq!(problem_warnings[1].code, "UNKNOWN_FIELD@FIELD:item");

    let mut part_warnings = vec![AiWarning {
        code: "FACT_ERROR@FIELD:answer".into(),
        severity: "error".into(),
        message: "公式を確認".into(),
    }];
    normalize_content_review_warning_codes(&mut part_warnings, "part");
    assert_eq!(part_warnings[0].code, "FACT_ERROR@FIELD:item");
}

#[test]
fn subject_specific_solution_instructions_preserve_mathematics_and_separate_other_subjects() {
    use kyozai_kobo_lib::ai::{
        scan_subject_explanation_structure, scan_subject_topic_guide_structure,
        solution_subject_fixed_instructions, SOLUTION_FIXED_INSTRUCTIONS,
        SOLUTION_SUBJECT_VALUES,
    };

    assert_eq!(
        solution_subject_fixed_instructions("mathematics"),
        SOLUTION_FIXED_INSTRUCTIONS,
        "数学は従来の詳細指示をそのまま使用すること"
    );

    for (subject, expected) in [
        ("physics", "【科目：物理】"),
        ("chemistry", "【科目：化学】"),
        ("biology", "【科目：生物】"),
        ("english", "【科目：英語】"),
        ("japanese", "【科目：国語】"),
        ("social_studies", "【科目：地理・歴史・公民】"),
        ("information", "【科目：情報】"),
        ("general", "【科目：その他】"),
    ] {
        let instructions = solution_subject_fixed_instructions(subject);
        assert!(instructions.contains(expected));
        assert!(instructions.contains("【要点】"));
        assert!(!instructions.contains("【軌跡・領域問題専用の解答規則】"));
    }
    assert_eq!(SOLUTION_SUBJECT_VALUES.len(), 9);

    assert!(scan_subject_explanation_structure("【要点】\n単位と向きを確認する。").is_empty());
    assert!(scan_subject_explanation_structure("法則を説明する。")
        .iter()
        .any(|warning| warning.code == "MISSING_SUBJECT_KEY_POINTS"));

    let subject_guide =
        "【概要】a【基本事項】b【要点】c【手順】d【典型例】e【よくある誤り】f";
    assert!(scan_subject_topic_guide_structure(subject_guide).is_empty());
    assert!(scan_subject_topic_guide_structure(&subject_guide.replace("【要点】", ""))
        .iter()
        .any(|warning| warning.code == "SUBJECT_TOPIC_GUIDE_MISSING_SECTIONS"));
}

#[test]
fn part_preview_document_reflects_single_and_two_column_layouts() {
    use kyozai_kobo_lib::commands::latex::build_part_preview_doc;

    let template =
        "\\documentclass{ujarticle}\n\\begin{document}\n{{BODY}}\n\\end{document}\n";
    let single = build_part_preview_doc(template, "$x^2+y^2=1$", "single_column");
    assert!(single.contains("$x^2+y^2=1$"));
    assert!(!single.contains("\\begin{multicols}{2}"));

    let two_column = build_part_preview_doc(template, "$x^2+y^2=1$", "two_column");
    assert!(two_column.contains("\\usepackage{multicol}"));
    assert!(two_column.contains("\\begin{multicols}{2}"));
    assert!(two_column.contains("\\setlength{\\columnseprule}{0.4pt}"));
    assert!(two_column.contains("\\end{multicols}"));
}

#[test]
fn problem_preview_document_reflects_single_and_two_column_layouts() {
    use kyozai_kobo_lib::commands::latex::build_problem_preview_doc;

    let template =
        "\\documentclass{ujarticle}\n\\begin{document}\n{{BODY}}\n\\end{document}\n";
    let single = build_problem_preview_doc(
        template,
        "問題文",
        "解答",
        "解説",
        "single_column",
    );
    assert!(single.contains("問題文"));
    assert!(single.contains("【解答】"));
    assert!(single.contains("【解説】"));
    assert!(!single.contains("\\begin{multicols}{2}"));

    let two_column = build_problem_preview_doc(
        template,
        "問題文",
        "解答",
        "解説",
        "two_column",
    );
    assert!(two_column.contains("\\usepackage{multicol}"));
    assert!(two_column.contains("\\begin{multicols}{2}"));
    assert!(two_column.contains("\\setlength{\\columnseprule}{0.4pt}"));
    assert!(two_column.contains("\\end{multicols}"));
}

#[test]
fn ai_generation_guidance_has_a_bounded_length() {
    use kyozai_kobo_lib::ai::{create_job, CreateJobPayload};

    let (_dir, state) = make_state();
    let error = create_job(
        &state,
        CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("generate_answer".into()),
            options: Some(json!({"solutionGuidance": "あ".repeat(1001)})),
            input_text: Some("$x^2=1$を解け。".into()),
            input_names: vec![],
            target_entity_type: None,
            target_entity_id: None,
            target_field: None,
        },
    )
    .expect_err("長すぎる解答方針は拒否すること");
    assert!(error.contains("最大1,000文字"));

    let error = create_job(
        &state,
        CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("generate_explanation".into()),
            options: Some(json!({"explanationGuidance": "あ".repeat(1001)})),
            input_text: Some(r"【問題文】$x^2=1$を解け。
【参照する解答】$x=\pm1$".into()),
            input_names: vec![],
            target_entity_type: None,
            target_entity_id: None,
            target_field: None,
        },
    )
    .expect_err("長すぎる解説内容の指示は拒否すること");
    assert!(error.contains("解説内容の指示"));
    assert!(error.contains("最大1,000文字"));

    let error = create_job(
        &state,
        CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("generate_answer".into()),
            options: Some(json!({"solutionLayout": "three_column"})),
            input_text: Some("$x^2=1$を解け。".into()),
            input_names: vec![],
            target_entity_type: None,
            target_entity_id: None,
            target_field: None,
        },
    )
    .expect_err("未対応の想定レイアウトは拒否すること");
    assert!(error.contains("two_column / single_column"));

    let error = create_job(
        &state,
        CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("generate_answer".into()),
            options: Some(json!({"solutionDetail": "expert"})),
            input_text: Some("$x^2=1$を解け。".into()),
            input_names: vec![],
            target_entity_type: None,
            target_entity_id: None,
            target_field: None,
        },
    )
    .expect_err("未対応の解答モードは拒否すること");
    assert!(error.contains("standard / beginner"));

    let error = create_job(
        &state,
        CreateJobPayload {
            source_type: "text".into(),
            conversion_mode: Some("generate_answer".into()),
            options: Some(json!({"solutionSubject": "astronomy"})),
            input_text: Some("問題文".into()),
            input_names: vec![],
            target_entity_type: None,
            target_entity_id: None,
            target_field: None,
        },
    )
    .expect_err("未対応の生成科目は拒否すること");
    assert!(error.contains("生成科目"));
    assert!(error.contains("mathematics / physics"));
}

#[test]
fn ai_source_revision_requires_a_valid_target_and_instruction() {
    use kyozai_kobo_lib::ai::{create_job, CreateJobPayload};

    let (_dir, state) = make_state();
    let payload = |source_type: &str, options: Value| CreateJobPayload {
        source_type: source_type.into(),
        conversion_mode: Some("revise_source".into()),
        options: Some(options),
        input_text: Some("既存のLaTeXソース".into()),
        input_names: vec![],
        target_entity_type: Some("part".into()),
        target_entity_id: Some(1),
        target_field: Some("latex_source".into()),
    };

    let missing_instruction = create_job(
        &state,
        payload("text", json!({"revisionTarget": "part", "revisionGuidance": ""})),
    )
    .expect_err("空の修正指示は拒否すること");
    assert!(missing_instruction.contains("修正指示"));

    let invalid_target = create_job(
        &state,
        payload(
            "text",
            json!({"revisionTarget": "template", "revisionGuidance": "誤字を直す"}),
        ),
    )
    .expect_err("未対応の修正対象は拒否すること");
    assert!(invalid_target.contains("問題文・解答・解説・部品"));

    let image_source = create_job(
        &state,
        payload(
            "image",
            json!({"revisionTarget": "part", "revisionGuidance": "誤字を直す"}),
        ),
    )
    .expect_err("画像をソース修正入力として扱わないこと");
    assert!(image_source.contains("テキスト入力"));

    let too_long = create_job(
        &state,
        payload(
            "text",
            json!({"revisionTarget": "part", "revisionGuidance": "あ".repeat(1001)}),
        ),
    )
    .expect_err("長すぎる修正指示は拒否すること");
    assert!(too_long.contains("最大1,000文字"));
}

#[test]
fn graph_ai_output_validation_rejects_commands_and_unknown_fields() {
    use kyozai_kobo_lib::ai::validate_graph_output;
    let valid = json!({
        "schemaVersion":1,
        "detectedType":"function_graph",
        "title":"二次関数",
        "expressions":[{"id":"expression-1","expression":"y=x^2-4*x+3","style":{"lineType":"solid","lineWidth":2,"color":"#2563eb"}}],
        "viewport":{"xMin":-2,"xMax":6,"yMin":-3,"yMax":8},
        "axes":{"showX":true,"showY":true,"showGrid":false},
        "points":[],"lines":[],"regions":[],"labels":[],
        "warnings":[],"uncertainFragments":[]
    }).to_string();
    let parsed = validate_graph_output(&valid).expect("正しいグラフJSONは通ること");
    assert_eq!(parsed.expressions[0].expression, "y=x^2-4*x+3");

    let command = valid.replace("y=x^2-4*x+3", "powershell http://evil.invalid");
    assert!(validate_graph_output(&command).is_err());
    let extra = valid.replacen("{", "{\"unexpected\":true,", 1);
    assert!(validate_graph_output(&extra).is_err());
    let invalid_range = valid.replace("\"xMax\":6", "\"xMax\":-3");
    assert!(validate_graph_output(&invalid_range).is_err());
}

#[test]
fn spatial_ai_output_validation_rejects_commands_unknown_fields_and_bad_coordinates() {
    use kyozai_kobo_lib::ai::validate_spatial_output;
    let valid = json!({
        "schemaVersion":1,"detectedType":"solid_geometry","title":"立方体ABCD-EFGH","projection":{"type":"orthographic"},
        "solids":[{"id":"solid-1","type":"cube","name":"立方体","size":[4,4,4],"position":[0,0,0],"rotation":[0,0,0],"vertexNames":["A","B","C","D","E","F","G","H"]}],
        "segments":[{"id":"segment-1","name":"対角線AG","from":[-2,-2,2],"to":[2,2,-2],"lineType":"solid"}],
        "points":[],"labels":[],"warnings":[],"uncertainFragments":[]
    }).to_string();
    assert!(validate_spatial_output(&valid).is_ok());
    assert!(validate_spatial_output(&valid.replace("立方体", "powershell http://evil.invalid")).is_err());
    assert!(validate_spatial_output(&valid.replacen("{", "{\"unexpected\":true,", 1)).is_err());
    assert!(validate_spatial_output(&valid.replace("[-2,-2,2]", "[1000001,0,0]")).is_err());
}

#[test]
fn schema_migration_sets_user_version() {
    let (_dir, state) = make_state();
    let conn = state.conn.lock().unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 10);
    for table in ["projects", "templates"] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name='version'", table),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "{} must have an optimistic-lock version", table);
    }
    for table in ["parts", "part_versions"] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name='unit_id'", table),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "{} must have a unit classification", table);
    }
    for table in ["problems", "problem_versions"] {
        for column in ["answer_completed", "explanation_completed"] {
            let count: i64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name=?1",
                        table
                    ),
                    [column],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table}.{column} must store completion state");
        }
    }
    let ai_inserted_at_column: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('ai_conversion_jobs') WHERE name='inserted_at'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ai_inserted_at_column, 1);
    let problem_layout_column: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('project_settings') WHERE name='problem_two_column'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(problem_layout_column, 1);
    for (table, column) in [
        ("problems", "statement_latex_two_column"),
        ("problem_versions", "statement_latex_two_column"),
        ("project_items", "snap_statement_two_column"),
    ] {
        let count: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name=?1",
                    table
                ),
                [column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "{table}.{column} must store the two-column form");
    }
}

#[test]
fn schema_migration_from_v4_adds_part_unit_before_creating_its_index() {
    let dir = tempdir::TempDir::new("kyozai-v4-migration-test").unwrap();
    let db_path = dir.path().join("kyozai-kobo.db");
    {
        let legacy = rusqlite::Connection::open(&db_path).unwrap();
        let legacy_schema = kyozai_kobo_lib::db::SCHEMA
            .replace(
                "    unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL,\n",
                "",
            )
            .replace(
                "CREATE INDEX IF NOT EXISTS idx_parts_unit ON parts(unit_id);\n",
                "",
            );
        legacy.execute_batch(&legacy_schema).unwrap();
        legacy.execute_batch("PRAGMA user_version=4;").unwrap();
    }

    let migrated = kyozai_kobo_lib::db::open_db(dir.path()).unwrap();
    let version: i64 = migrated
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 10);

    for table in ["parts", "part_versions"] {
        let count: i64 = migrated
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name='unit_id'",
                    table
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "{table}.unit_id must be added during migration");
    }

    let index_count: i64 = migrated
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_parts_unit'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index_count, 1);
}

#[test]
fn schema_migration_from_v6_backfills_both_problem_statement_layouts() {
    let dir = tempdir::TempDir::new("kyozai-v6-layout-migration-test").unwrap();
    let db_path = dir.path().join("kyozai-kobo.db");
    {
        let legacy = rusqlite::Connection::open(&db_path).unwrap();
        let legacy_schema = kyozai_kobo_lib::db::SCHEMA
            .replace(
                "    statement_latex_two_column TEXT NOT NULL DEFAULT '',\n",
                "",
            )
            .replace(
                "    snap_statement_two_column TEXT NOT NULL DEFAULT '',\n",
                "",
            );
        legacy.execute_batch(&legacy_schema).unwrap();
        legacy
            .execute_batch(
                "INSERT INTO subjects (id,name) VALUES (1,'数学');
                 INSERT INTO fields (id,subject_id,name) VALUES (1,1,'数学I');
                 INSERT INTO units (id,field_id,name) VALUES (1,1,'二次関数');
                 INSERT INTO problems
                   (id,unit_id,title,statement_latex,created_at,updated_at)
                   VALUES (1,1,'旧問題','旧問題文','2026-01-01','2026-01-01');
                 INSERT INTO problem_versions
                   (problem_id,title,statement_latex,saved_at)
                   VALUES (1,'旧問題','旧履歴問題文','2026-01-01');
                 INSERT INTO projects
                   (id,name,created_at,updated_at)
                   VALUES (1,'旧教材','2026-01-01','2026-01-01');
                 INSERT INTO project_items
                   (project_id,item_type,problem_id,snap_title,snap_statement,created_at)
                   VALUES (1,'problem',1,'旧問題','旧スナップショット問題文','2026-01-01');
                 PRAGMA user_version=6;",
            )
            .unwrap();
    }

    let migrated = kyozai_kobo_lib::db::open_db(dir.path()).unwrap();
    let version: i64 = migrated
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 10);
    let bank_two: String = migrated
        .query_row(
            "SELECT statement_latex_two_column FROM problems WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let history_two: String = migrated
        .query_row(
            "SELECT statement_latex_two_column FROM problem_versions WHERE problem_id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let snapshot_two: String = migrated
        .query_row(
            "SELECT snap_statement_two_column FROM project_items WHERE project_id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bank_two, "旧問題文");
    assert_eq!(history_two, "旧履歴問題文");
    assert_eq!(snapshot_two, "旧スナップショット問題文");
}

#[test]
fn schema_migration_from_v7_adds_problem_completion_flags() {
    let dir = tempdir::TempDir::new("kyozai-v7-completion-migration-test").unwrap();
    let db_path = dir.path().join("kyozai-kobo.db");
    {
        let legacy = rusqlite::Connection::open(&db_path).unwrap();
        let legacy_schema = kyozai_kobo_lib::db::SCHEMA
            .replace(
                "    answer_completed INTEGER NOT NULL DEFAULT 0,\n",
                "",
            )
            .replace(
                "    explanation_completed INTEGER NOT NULL DEFAULT 0,\n",
                "",
            );
        legacy.execute_batch(&legacy_schema).unwrap();
        legacy
            .execute_batch(
                "INSERT INTO subjects (id,name) VALUES (1,'数学');
                 INSERT INTO fields (id,subject_id,name) VALUES (1,1,'数学I');
                 INSERT INTO units (id,field_id,name) VALUES (1,1,'二次関数');
                 INSERT INTO problems
                   (id,unit_id,title,answer_latex,explanation_latex,created_at,updated_at)
                   VALUES (1,1,'旧問題','旧解答','旧解説','2026-01-01','2026-01-01');
                 INSERT INTO problem_versions
                   (problem_id,title,answer_latex,explanation_latex,saved_at)
                   VALUES (1,'旧問題','旧解答','旧解説','2026-01-01');
                 PRAGMA user_version=7;",
            )
            .unwrap();
    }

    let migrated = kyozai_kobo_lib::db::open_db(dir.path()).unwrap();
    let version: i64 = migrated
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 10);
    for table in ["problems", "problem_versions"] {
        let flags: (i64, i64) = migrated
            .query_row(
                &format!(
                    "SELECT answer_completed, explanation_completed FROM {} LIMIT 1",
                    table
                ),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(flags, (0, 0), "{table}の既存データは未完成として移行する");
    }
}

#[test]
fn schema_migration_from_v8_adds_ai_job_inserted_state() {
    let dir = tempdir::TempDir::new("kyozai-v8-ai-inserted-migration-test").unwrap();
    let db_path = dir.path().join("kyozai-kobo.db");
    {
        let legacy = rusqlite::Connection::open(&db_path).unwrap();
        legacy.execute_batch(kyozai_kobo_lib::db::SCHEMA).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE ai_conversion_jobs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    job_uuid TEXT NOT NULL UNIQUE,
                    source_type TEXT NOT NULL DEFAULT 'image',
                    conversion_mode TEXT NOT NULL DEFAULT 'auto',
                    options_json TEXT NOT NULL DEFAULT '{}',
                    status TEXT NOT NULL DEFAULT 'queued',
                    progress_message TEXT NOT NULL DEFAULT '',
                    input_text TEXT NOT NULL DEFAULT '',
                    input_asset_paths TEXT NOT NULL DEFAULT '[]',
                    output_latex TEXT NOT NULL DEFAULT '',
                    structured_result_json TEXT NOT NULL DEFAULT '',
                    warnings_json TEXT NOT NULL DEFAULT '[]',
                    uncertain_fragments_json TEXT NOT NULL DEFAULT '[]',
                    compile_status TEXT NOT NULL DEFAULT 'none',
                    compile_log TEXT NOT NULL DEFAULT '',
                    preview_pdf_path TEXT NOT NULL DEFAULT '',
                    target_entity_type TEXT NOT NULL DEFAULT '',
                    target_entity_id INTEGER,
                    target_field TEXT NOT NULL DEFAULT '',
                    error_code TEXT NOT NULL DEFAULT '',
                    error_message TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    completed_at TEXT NOT NULL DEFAULT ''
                 );
                 INSERT INTO ai_conversion_jobs
                    (job_uuid,status,created_at,updated_at)
                 VALUES ('legacy-job','completed','2026-01-01','2026-01-01');
                 PRAGMA user_version=8;",
            )
            .unwrap();
    }

    let migrated = kyozai_kobo_lib::db::open_db(dir.path()).unwrap();
    let version: i64 = migrated
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 10);
    let inserted_at: String = migrated
        .query_row(
            "SELECT inserted_at FROM ai_conversion_jobs WHERE job_uuid='legacy-job'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(inserted_at.is_empty(), "既存の未判定ジョブは未挿入として移行する");
}

#[tokio::test]
async fn authenticated_graph_crud_validates_json_and_detects_conflicts() {
    let (_dir, state) = make_state();
    let router = build_router(state);
    let cookie = pair(&router).await;
    let project = json!({
        "version": 1,
        "appName": "MathGraph PDF Studio",
        "expressions": [{
            "id":"e1","input":"y=x^2","name":"","visible":true,"color":"#2563eb",
            "lineWidth":2,"lineStyle":"solid","fillColor":"#2563eb","fillOpacity":0.25,
            "fillStyle":"solid","tmin":0,"tmax":6.28
        }],
        "points": [],
        "labels": [],
        "range": {"xmin":-5,"xmax":5,"ymin":-5,"ymax":5,"xstep":1,"ystep":1},
        "paper": {}
    });

    let create = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/create_graph",
            json!({"payload":{"title":"二次関数","graphJson":project.to_string()}}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let graph_id = body_json(create).await.as_str().unwrap().to_string();

    let get = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/get_graph",
            json!({"id":graph_id}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    let stored = body_json(get).await;
    assert_eq!(stored["title"], "二次関数");
    assert_eq!(stored["version"], 1);

    let update_body = json!({"payload":{
        "id":graph_id,"title":"更新版","graphJson":project.to_string(),"expectedVersion":1
    }});
    let updated = router
        .clone()
        .oneshot(post_json("/api/invoke/update_graph", update_body.clone(), Some(&cookie), true))
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(body_json(updated).await, json!(2));

    let stale = router
        .clone()
        .oneshot(post_json("/api/invoke/update_graph", update_body, Some(&cookie), true))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);

    let invalid = router
        .clone()
        .oneshot(post_json(
            "/api/invoke/create_graph",
            json!({"payload":{"title":"invalid","graphJson":"{not json}"}}),
            Some(&cookie),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn graph_exports_and_material_insert_are_snapshotted() {
    use base64::Engine;
    use kyozai_kobo_lib::commands::{graphs, projects};
    use std::collections::BTreeMap;
    let (_dir, state) = make_state();
    let graph_json = json!({
        "version":1,"appName":"MathGraph PDF Studio","expressions":[],"points":[],"labels":[],
        "range":{"xmin":-5,"xmax":5,"ymin":-5,"ymax":5,"xstep":1,"ystep":1},"paper":{}
    }).to_string();
    let graph_id = graphs::create_graph(&state, graphs::CreateGraphPayload {
        title: "教材用グラフ".into(), graph_json, graph_type: None, source_type: None, warnings: None,
    }).unwrap();
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut files = BTreeMap::new();
    files.insert("pdf".into(), encode(b"%PDF-1.4\n%%EOF"));
    files.insert("png".into(), encode(&[0x89,b'P',b'N',b'G',0x0d,0x0a,0x1a,0x0a,0,0,0,0]));
    files.insert("svg".into(), encode(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>"));
    files.insert("tex".into(), encode(b"\\begin{tikzpicture}\\end{tikzpicture}"));
    let saved = graphs::save_graph_exports(&state, graph_id.clone(), files).unwrap();
    assert_eq!(saved.len(), 4);

    let project_id = projects::create_project(&state, "テスト教材".into(), None).unwrap();
    assert!(graphs::insert_graph_to_project(&state, graph_id.clone(), project_id, Some(0))
        .unwrap_err()
        .starts_with("CONFLICT:"));
    let item_id = graphs::insert_graph_to_project(&state, graph_id.clone(), project_id, Some(1)).unwrap();
    let conn = state.conn.lock().unwrap();
    let content: String = conn.query_row("SELECT content FROM project_items WHERE id=?1", [item_id], |r| r.get(0)).unwrap();
    assert!(content.contains("assets/graphs/snapshots/graphasset_"));
    assert!(content.contains("width=0.72\\linewidth"));
    assert!(!content.contains("\\begin{center}"));
    assert!(content.contains("height=0.28\\textheight,keepaspectratio"));
    let usage: i64 = conn.query_row("SELECT COUNT(*) FROM graph_assets WHERE graph_id=?1", [&graph_id], |r| r.get(0)).unwrap();
    let snapshot_pdf: String = conn.query_row(
        "SELECT primary_asset_path FROM graph_assets WHERE graph_id=?1",
        [&graph_id],
        |r| r.get(0),
    ).unwrap();
    let snapshot_before = std::fs::read(&snapshot_pdf).unwrap();
    drop(conn);
    std::fs::write(state.graph_dir(&graph_id).join("graph.pdf"), b"%PDF-1.7\nchanged").unwrap();
    assert_eq!(std::fs::read(snapshot_pdf).unwrap(), snapshot_before, "教材snapshotは正本更新で変化しないこと");
    assert_eq!(usage, 1);
}

#[tokio::test]
async fn graph_files_require_auth_and_stream_with_safe_disposition() {
    use base64::Engine;
    use kyozai_kobo_lib::commands::graphs;
    use std::collections::BTreeMap;

    let (_dir, state) = make_state();
    let graph_json = json!({
        "version":1,"appName":"MathGraph PDF Studio","expressions":[],"points":[],"labels":[],
        "range":{"xmin":-5,"xmax":5,"ymin":-5,"ymax":5,"xstep":1,"ystep":1},"paper":{}
    }).to_string();
    let graph_id = graphs::create_graph(&state, graphs::CreateGraphPayload {
        title: "配信テスト".into(), graph_json, graph_type: None, source_type: None, warnings: None,
    }).unwrap();
    let mut files = BTreeMap::new();
    files.insert("pdf".into(), base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4\n%%EOF"));
    graphs::save_graph_exports(&state, graph_id.clone(), files).unwrap();
    let router = build_router(state);

    let unauthenticated = router.clone().oneshot(
        Request::get(format!("/api/graphs/{graph_id}/files/pdf"))
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let cookie = pair(&router).await;
    let authenticated = router.clone().oneshot(
        Request::get(format!("/api/graphs/{graph_id}/files/pdf?download=1"))
            .header(header::COOKIE, &cookie)
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(authenticated.status(), StatusCode::OK);
    assert_eq!(authenticated.headers()[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(authenticated.headers()[header::CONTENT_DISPOSITION], "attachment; filename=\"graph.pdf\"");
    let bytes = authenticated.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"%PDF-"));

    let zip_response = router.clone().oneshot(
        Request::get(format!("/api/graphs/{graph_id}/files/zip?download=1"))
            .header(header::COOKIE, &cookie)
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(zip_response.status(), StatusCode::OK);
    assert_eq!(zip_response.headers()[header::CONTENT_TYPE], "application/zip");
    assert_eq!(zip_response.headers()[header::CONTENT_DISPOSITION], "attachment; filename=\"graph.zip\"");
    let zip_bytes = zip_response.into_body().collect().await.unwrap().to_bytes();
    assert!(zip_bytes.starts_with(b"PK\x03\x04"));
}

#[test]
fn web_graph_session_fixes_target_and_rejects_stale_material() {
    use base64::Engine;
    use kyozai_kobo_lib::commands::{graph_web, graphs, projects};
    use std::collections::BTreeMap;

    let (_dir, state) = make_state();
    let project_id = projects::create_project(&state, "連携先教材".into(), None).unwrap();
    let graph_json = json!({
        "version":1,"appName":"MathGraph PDF Studio","expressions":[],"points":[],"labels":[],
        "range":{"xmin":-5,"xmax":5,"ymin":-5,"ymax":5,"xstep":1,"ystep":1},"paper":{}
    }).to_string();
    let graph_id = graphs::create_graph(&state, graphs::CreateGraphPayload {
        title: "session test".into(), graph_json, graph_type: None, source_type: None, warnings: None,
    }).unwrap();
    let mut files = BTreeMap::new();
    files.insert("pdf".into(), base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4\n%%EOF"));
    files.insert("png".into(), base64::engine::general_purpose::STANDARD.encode(&[0x89,b'P',b'N',b'G',0x0d,0x0a,0x1a,0x0a,0,0,0,0]));
    graphs::save_graph_exports(&state, graph_id.clone(), files).unwrap();

    let session = graph_web::create_graph_web_session(&state, graph_web::CreateGraphWebSessionPayload {
        project_id: Some(project_id), problem_id: None, item_id: None,
        target_field: "project_text".into(), selection_start: Some(0), selection_end: Some(0),
    }).unwrap();
    assert_eq!(session.status, "pending");
    let completed = graph_web::complete_graph_web_session(&state, session.session_id, graph_id.clone(), 1).unwrap();
    assert_eq!(completed.session.status, "completed");
    assert!(completed.snapshot.inserted_latex.contains("assets/graphs/snapshots/graphasset_"));
    assert!(completed.snapshot.inserted_latex.contains("width=0.72\\linewidth"));
    assert!(!completed.snapshot.inserted_latex.contains("\\begin{center}"));
    assert!(state.graph_assets_dir().join("snapshots").join(&completed.snapshot.asset_id).join("graph.pdf").is_file());

    let stale = graph_web::create_graph_web_session(&state, graph_web::CreateGraphWebSessionPayload {
        project_id: Some(project_id), problem_id: None, item_id: None,
        target_field: "project_text".into(), selection_start: None, selection_end: None,
    }).unwrap();
    state.conn.lock().unwrap().execute(
        "UPDATE projects SET version=version+1 WHERE id=?1", [project_id]
    ).unwrap();
    assert!(graph_web::complete_graph_web_session(&state, stale.session_id, graph_id, 1)
        .unwrap_err()
        .starts_with("CONFLICT:"));
}

#[test]
fn legacy_graph_asset_is_imported_only_from_managed_storage_and_web_paths_are_redacted() {
    use kyozai_kobo_lib::commands::graphs;

    let (_dir, state) = make_state();
    let project_id = kyozai_kobo_lib::commands::projects::create_project(&state, "asset test".into(), None).unwrap();
    let source_dir = state.graph_assets_dir().join("legacy_asset");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("graph.json");
    std::fs::write(&source, json!({
        "version":1,"appName":"MathGraph PDF Studio","expressions":[],"points":[],"labels":[],
        "range":{"xmin":-5,"xmax":5,"ymin":-5,"ymax":5,"xstep":1,"ystep":1},"paper":{}
    }).to_string()).unwrap();
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO graph_assets
             (asset_id,graph_id,display_name,project_id,editable_source_path,primary_asset_path,created_at,updated_at)
             VALUES ('legacy_asset','legacy_graph','旧グラフ',?1,?2,?3,'2026-07-12','2026-07-12')",
            rusqlite::params![project_id, source.to_string_lossy(), source_dir.join("graph.pdf").to_string_lossy()],
        ).unwrap();
    }
    assert_eq!(graphs::ensure_graph_from_asset(&state, "legacy_asset".into()).unwrap(), "legacy_graph");
    assert_eq!(graphs::get_graph(&state, "legacy_graph".into()).unwrap().summary.source_type, "import");

    let web = dispatch(
        &state,
        "list_graph_assets",
        json!({"projectId": project_id, "problemId": null}),
        Origin::Web,
    ).unwrap();
    assert_eq!(web[0]["editableSourcePath"], "");
    assert_eq!(web[0]["primaryAssetPath"], "");

    let outside = state.data_dir.join("outside.json");
    std::fs::write(&outside, std::fs::read_to_string(&source).unwrap()).unwrap();
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO graph_assets
             (asset_id,graph_id,display_name,project_id,editable_source_path,primary_asset_path,created_at,updated_at)
             VALUES ('outside_asset','outside_graph','outside',?1,?2,?2,'2026-07-12','2026-07-12')",
            rusqlite::params![project_id, outside.to_string_lossy()],
        ).unwrap();
    }
    assert!(graphs::ensure_graph_from_asset(&state, "outside_asset".into()).is_err());
}

#[test]
fn backup_restore_is_integrity_checked_and_clears_sessions() {
    let (_dir, state) = make_state();
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO subjects (name, sort_order) VALUES ('復元前', 1)",
            [],
        )
        .unwrap();
    }
    std::fs::write(state.attachments_dir().join("restore-test.txt"), b"before").unwrap();
    let _token =
        kyozai_kobo_lib::server::auth::create_session(&state, "test", "test-agent").unwrap();

    let backup = kyozai_kobo_lib::server::backup::backup_now(&state).unwrap();
    let file_name = backup["dbFile"].as_str().unwrap().to_string();

    {
        let conn = state.conn.lock().unwrap();
        conn.execute("UPDATE subjects SET name='復元後の変更'", [])
            .unwrap();
    }
    std::fs::write(state.attachments_dir().join("restore-test.txt"), b"after").unwrap();

    kyozai_kobo_lib::server::backup::restore_backup(&state, &file_name).unwrap();
    let conn = state.conn.lock().unwrap();
    let name: String = conn
        .query_row("SELECT name FROM subjects LIMIT 1", [], |row| row.get(0))
        .unwrap();
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM web_sessions", [], |row| row.get(0))
        .unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    drop(conn);

    assert_eq!(name, "復元前");
    assert_eq!(sessions, 0, "復元した古いWebセッションは失効すること");
    assert_eq!(integrity, "ok");
    assert_eq!(
        std::fs::read(state.attachments_dir().join("restore-test.txt")).unwrap(),
        b"before"
    );
}

#[test]
fn parts_can_be_classified_filtered_and_duplicated_by_unit() {
    use kyozai_kobo_lib::commands::parts;
    use kyozai_kobo_lib::models::{PartSearchQuery, PartUpdate};

    let (_dir, state) = make_state();
    let (subject_id, field_id, unit_id) = {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO subjects (name, sort_order) VALUES ('数学III', 0)",
            [],
        )
        .unwrap();
        let subject_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO fields (subject_id, name, sort_order) VALUES (?1, '微分法', 0)",
            [subject_id],
        )
        .unwrap();
        let field_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO units (field_id, name, sort_order) VALUES (?1, '関数の増減', 0)",
            [field_id],
        )
        .unwrap();
        (subject_id, field_id, conn.last_insert_rowid())
    };

    let part_id = parts::create_part(&state, "増減表の定石".into()).unwrap();
    let payload = serde_json::from_value::<PartUpdate>(json!({
        "id": part_id,
        "unit_id": unit_id,
        "title": "増減表の定石",
        "part_type": "text",
        "category": "定石",
        "tags": ["微分"],
        "latex_source": "導関数の符号を増減表に整理する。",
        "description": "",
        "difficulty_rank": "B",
        "is_required": true,
        "output_target": "both",
        "layout_mode": "single_column",
        "expected_version": 1
    }))
    .unwrap();
    parts::update_part(&state, payload).unwrap();

    let full = parts::get_part(&state, part_id).unwrap();
    assert_eq!(full.subject_id, Some(subject_id));
    assert_eq!(full.subject_name, "数学III");
    assert_eq!(full.field_id, Some(field_id));
    assert_eq!(full.field_name, "微分法");
    assert_eq!(full.unit_id, Some(unit_id));
    assert_eq!(full.unit_name, "関数の増減");

    let query = PartSearchQuery {
        text: String::new(),
        subject_id: Some(subject_id),
        field_id: Some(field_id),
        unit_id: Some(unit_id),
        part_type: None,
        category: None,
        tag: None,
        difficulty_rank: None,
        difficulty_ranks: None,
        required_filter: None,
    };
    let found = parts::search_parts(&state, query).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, part_id);
    assert_eq!(found[0].unit_name, "関数の増減");

    let duplicate_id = parts::duplicate_part(&state, part_id).unwrap();
    let duplicate = parts::get_part(&state, duplicate_id).unwrap();
    assert_eq!(duplicate.unit_id, Some(unit_id));
}

#[test]
fn problem_completion_flags_are_saved_listed_duplicated_and_restored() {
    use kyozai_kobo_lib::commands::problems;
    use kyozai_kobo_lib::models::ProblemUpdate;

    let (_dir, state) = make_state();
    let unit_id = {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO subjects (name, sort_order) VALUES ('数学', 0)",
            [],
        )
        .unwrap();
        let subject_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO fields (subject_id, name, sort_order) VALUES (?1, '数学I', 0)",
            [subject_id],
        )
        .unwrap();
        let field_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO units (field_id, name, sort_order) VALUES (?1, '二次関数', 0)",
            [field_id],
        )
        .unwrap();
        conn.last_insert_rowid()
    };

    let problem_id = problems::create_problem(&state, unit_id, "完成管理".into()).unwrap();
    let payload = serde_json::from_value::<ProblemUpdate>(json!({
        "id": problem_id,
        "unit_id": unit_id,
        "title": "完成管理",
        "statement_latex": "問題文",
        "statement_latex_two_column": "問題文",
        "answer_latex": "解答",
        "explanation_latex": "解説",
        "answer_completed": true,
        "explanation_completed": true,
        "difficulty": "標準",
        "difficulty_rank": null,
        "is_required": false,
        "memo": "",
        "tags": [],
        "expected_version": 1
    }))
    .unwrap();
    problems::update_problem(&state, payload).unwrap();

    let full = problems::get_problem(&state, problem_id).unwrap();
    assert!(full.answer_completed);
    assert!(full.explanation_completed);
    let listed = problems::list_problems(&state, unit_id).unwrap();
    assert!(listed[0].answer_completed);
    assert!(listed[0].explanation_completed);

    let duplicate_id = problems::duplicate_problem(&state, problem_id).unwrap();
    let duplicate = problems::get_problem(&state, duplicate_id).unwrap();
    assert!(duplicate.answer_completed);
    assert!(duplicate.explanation_completed);

    let old_version = problems::list_versions(&state, problem_id).unwrap()[0].id;
    problems::restore_version(&state, old_version).unwrap();
    let restored = problems::get_problem(&state, problem_id).unwrap();
    assert!(!restored.answer_completed);
    assert!(!restored.explanation_completed);
}

#[test]
fn problem_booklet_two_column_setting_persists_and_duplicates() {
    use kyozai_kobo_lib::commands::projects;

    let (_dir, state) = make_state();
    let project_id = projects::create_project(&state, "問題冊子段組".into(), None).unwrap();
    let project = projects::get_project(&state, project_id).unwrap();
    assert!(!project.settings.problem_two_column);

    let mut settings = project.settings.clone();
    settings.problem_two_column = true;
    projects::update_project_settings(&state, project_id, settings, Some(project.version))
        .unwrap();
    let updated = projects::get_project(&state, project_id).unwrap();
    assert!(updated.settings.problem_two_column);

    let duplicate_id = projects::duplicate_project(&state, project_id).unwrap();
    let duplicate = projects::get_project(&state, duplicate_id).unwrap();
    assert!(duplicate.settings.problem_two_column);
}

#[test]
fn optimistic_lock_on_parts_items_projects_and_templates() {
    use kyozai_kobo_lib::commands::{parts, projects, templates};
    let (_dir, state) = make_state();

    let part_id = parts::create_part(&state, "部品A".into()).unwrap();
    let mk_payload = |expected: Option<i64>| {
        serde_json::from_value::<kyozai_kobo_lib::models::PartUpdate>(json!({
            "id": part_id,
            "title": "部品A",
            "part_type": "text",
            "category": "",
            "tags": [],
            "latex_source": "本文",
            "description": "",
            "difficulty_rank": null,
            "is_required": false,
            "output_target": "both",
            "expected_version": expected,
        }))
        .unwrap()
    };
    assert_eq!(parts::update_part(&state, mk_payload(Some(1))).unwrap(), 2);
    let err = parts::update_part(&state, mk_payload(Some(1))).unwrap_err();
    assert!(err.starts_with("CONFLICT:2"));

    // プロジェクト項目
    let project_id = projects::create_project(&state, "教材".into(), None).unwrap();
    let item_id = projects::add_content_item(&state, project_id, "text".into(), "説明".into(), None).unwrap();
    let upd = |expected: Option<i64>| {
        serde_json::from_value::<kyozai_kobo_lib::models::ProjectItemUpdate>(json!({
            "itemId": item_id,
            "content": "更新後",
            "expectedVersion": expected,
        }))
        .unwrap()
    };
    assert_eq!(projects::update_project_item(&state, upd(Some(1))).unwrap(), 2);
    let err = projects::update_project_item(&state, upd(Some(1))).unwrap_err();
    assert!(err.starts_with("CONFLICT:2"));

    assert_eq!(
        projects::update_project_meta(
            &state,
            project_id,
            "updated".into(),
            "".into(),
            Some(1),
        )
        .unwrap(),
        2
    );
    let err = projects::update_project_meta(
        &state,
        project_id,
        "stale".into(),
        "".into(),
        Some(1),
    )
    .unwrap_err();
    assert!(err.starts_with("CONFLICT:2"));

    let template_id = templates::create_template(&state, "template".into()).unwrap();
    let template = templates::get_template(&state, template_id).unwrap();
    let payload = |expected: Option<i64>| kyozai_kobo_lib::models::TemplateUpdate {
        id: template_id,
        expected_version: expected,
        name: template.name.clone(),
        description: template.description.clone(),
        base_template: template.base_template.clone(),
        problem_template: template.problem_template.clone(),
        answer_template: template.answer_template.clone(),
        compile_method: template.compile_method.clone(),
        packages_memo: template.packages_memo.clone(),
    };
    templates::update_template(&state, payload(Some(1))).unwrap();
    let err = templates::update_template(&state, payload(Some(1))).unwrap_err();
    assert!(err.starts_with("CONFLICT:2"));
}

#[test]
fn template_assets_with_same_display_name_are_immutable() {
    use kyozai_kobo_lib::commands::templates;
    let (dir, state) = make_state();
    let template_id = templates::create_template(&state, "asset-test".into()).unwrap();
    let source = dir.path().join("figure.png");
    std::fs::write(&source, b"first-generation").unwrap();
    let first = templates::add_template_asset(
        &state,
        template_id,
        source.to_string_lossy().to_string(),
    )
    .unwrap();
    std::fs::write(&source, b"second-generation").unwrap();
    let second = templates::add_template_asset(
        &state,
        template_id,
        source.to_string_lossy().to_string(),
    )
    .unwrap();

    assert_eq!(first.file_name, second.file_name);
    assert_ne!(first.stored_name, second.stored_name);
    assert_eq!(
        std::fs::read(state.data_dir.join("template_assets").join(&first.stored_name)).unwrap(),
        b"first-generation"
    );
    assert_eq!(
        std::fs::read(state.data_dir.join("template_assets").join(&second.stored_name)).unwrap(),
        b"second-generation"
    );
}

/// デスクトップのPDFプレビュー用 read_compiled_file:
/// 許可ルート配下はbase64で読め、ルート外は拒否、Webからはブロックされる
#[tokio::test]
async fn read_compiled_file_scope_and_origin() {
    let (dir, state) = make_state();

    // 許可ルート: %TEMP%\kyozai-kobo-build 配下
    let build_dir = std::env::temp_dir()
        .join("kyozai-kobo-build")
        .join("dispatch-read-test");
    std::fs::create_dir_all(&build_dir).unwrap();
    let pdf_path = build_dir.join("kyozai.pdf");
    std::fs::write(&pdf_path, b"%PDF-1.7 test-bytes").unwrap();

    let ok = dispatch(
        &state,
        "read_compiled_file",
        json!({"path": pdf_path.to_string_lossy()}),
        Origin::Desktop,
    )
    .unwrap();
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(ok.as_str().unwrap())
        .unwrap();
    assert!(decoded.starts_with(b"%PDF"));

    // 許可ルート外（データフォルダ直下）は拒否
    let outside = dir.path().join("secret.txt");
    std::fs::write(&outside, b"secret").unwrap();
    let denied = dispatch(
        &state,
        "read_compiled_file",
        json!({"path": outside.to_string_lossy()}),
        Origin::Desktop,
    );
    assert!(denied.is_err());

    // Webからは利用不可（Webは /api/files/build を使う）
    let web = dispatch(
        &state,
        "read_compiled_file",
        json!({"path": pdf_path.to_string_lossy()}),
        Origin::Web,
    );
    assert!(web.is_err());

    std::fs::remove_dir_all(&build_dir).ok();
}

/// 完了済みジョブの挿入（再コンパイル・起動時修復テスト用の最小フィクスチャ）
fn insert_completed_ai_job(state: &Arc<AppState>, status: &str, compile_status: &str) -> i64 {
    let structured = json!({
        "schemaVersion": 1,
        "detectedType": "math",
        "latex": "$x^2$",
        "plainText": "x^2",
        "requiredPackages": [],
        "warnings": [],
        "uncertainFragments": [],
        "segments": [],
        "suggestedInsertTarget": "problem_body"
    })
    .to_string();
    let conn = state.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO ai_conversion_jobs (job_uuid, source_type, conversion_mode, status, progress_message,
                input_text, output_latex, structured_result_json, compile_status, created_at, updated_at)
         VALUES (?1, 'text', 'auto', ?2, '', 'x^2', '$x^2$', ?3, ?4, ?5, ?5)",
        rusqlite::params![
            uuid::Uuid::new_v4().simple().to_string(),
            status,
            structured,
            compile_status,
            kyozai_kobo_lib::db::now_str()
        ],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[test]
fn unchanged_ai_latex_keeps_compile_result_and_can_be_saved_as_part() {
    let (_dir, state) = make_state();
    let job_id = insert_completed_ai_job(&state, "completed", "ok");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs
             SET compile_log='試験コンパイル成功', preview_pdf_path='preview.pdf',
                 inserted_at='2026-01-01 12:00:00'
             WHERE id=?1",
            [job_id],
        )
        .unwrap();
    }

    kyozai_kobo_lib::ai::update_job_latex(&state, job_id, "$x^2$".into())
        .expect("同じLaTeXの保存に失敗した");

    let (compile_status, compile_log, preview_pdf_path, inserted_at): (
        String,
        String,
        String,
        String,
    ) = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT compile_status, compile_log, preview_pdf_path, inserted_at
             FROM ai_conversion_jobs WHERE id=?1",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(compile_status, "ok");
    assert_eq!(compile_log, "試験コンパイル成功");
    assert_eq!(preview_pdf_path, "preview.pdf");
    assert_eq!(
        inserted_at, "2026-01-01 12:00:00",
        "同じLaTeXを再保存しただけなら挿入済み状態を維持すること"
    );

    let part_id = kyozai_kobo_lib::ai::save_as_part(
        &state,
        job_id,
        "直近のAI変換".into(),
        None,
        true,
    )
    .expect("同じLaTeXを再送した後も部品として保存できること");
    let saved_latex: String = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT latex_source FROM parts WHERE id=?1",
            [part_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(saved_latex, "$x^2$");
    let saved_job = kyozai_kobo_lib::ai::get_job(&state, job_id).unwrap();
    assert_eq!(saved_job["targetEntityName"].as_str(), Some("直近のAI変換"));
    assert!(
        !saved_job["insertedAt"].as_str().unwrap_or_default().is_empty(),
        "部品として保存したジョブは挿入済みと分かること"
    );
}

#[test]
fn changed_ai_latex_invalidates_previous_compile_result() {
    let (_dir, state) = make_state();
    let job_id = insert_completed_ai_job(&state, "completed", "ok");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs
             SET compile_log='試験コンパイル成功', preview_pdf_path='preview.pdf',
                 inserted_at='2026-01-01 12:00:00'
             WHERE id=?1",
            [job_id],
        )
        .unwrap();
    }

    kyozai_kobo_lib::ai::update_job_latex(&state, job_id, "$x^3$".into())
        .expect("編集後のLaTeXを保存できること");

    let (compile_status, compile_log, preview_pdf_path, inserted_at): (
        String,
        String,
        String,
        String,
    ) = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT compile_status, compile_log, preview_pdf_path, inserted_at
             FROM ai_conversion_jobs WHERE id=?1",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(compile_status, "none");
    assert!(compile_log.is_empty());
    assert!(preview_pdf_path.is_empty());
    assert!(
        inserted_at.is_empty(),
        "挿入後にLaTeXを変更したジョブは未挿入へ戻すこと"
    );
}

#[test]
fn extracted_problems_are_saved_as_independent_bank_entries() {
    use kyozai_kobo_lib::ai::{save_extracted_problems, ExtractedProblem};

    let (_dir, state) = make_state();
    let unit_id = {
        let conn = state.conn.lock().unwrap();
        conn.execute("INSERT INTO subjects (name) VALUES ('数学')", [])
            .unwrap();
        let subject_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO fields (subject_id, name) VALUES (?1, '数学I')",
            [subject_id],
        )
        .unwrap();
        let field_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO units (field_id, name) VALUES (?1, '二次関数')",
            [field_id],
        )
        .unwrap();
        conn.last_insert_rowid()
    };
    let job_id = insert_completed_ai_job(&state, "completed", "ok");
    let ids = save_extracted_problems(
        &state,
        job_id,
        unit_id,
        vec![
            ExtractedProblem {
                title: "平方完成".into(),
                statement_latex: "$y=x^2-4x+3$の最小値を求めよ。".into(),
                statement_latex_two_column:
                    "$y=x^2-4x+3$ の最小値を求めよ。".into(),
                source_image_indexes: vec![1],
            },
            ExtractedProblem {
                title: "放物線".into(),
                statement_latex: "放物線$y=x^2$を平行移動せよ。".into(),
                statement_latex_two_column:
                    "放物線 $y=x^2$ を\n平行移動せよ。".into(),
                source_image_indexes: vec![1, 2],
            },
        ],
        false,
    )
    .expect("複数問題を一括保存できること");
    assert_eq!(ids.len(), 2);

    let conn = state.conn.lock().unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM problems WHERE unit_id=?1 AND memo='AI変換から一括作成'",
            [unit_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let saved_two_column: String = conn
        .query_row(
            "SELECT statement_latex_two_column FROM problems WHERE id=?1",
            [ids[1]],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(saved_two_column, "放物線 $y=x^2$ を\n平行移動せよ。");
    drop(conn);
    let saved_job = kyozai_kobo_lib::ai::get_job(&state, job_id).unwrap();
    assert_eq!(saved_job["targetEntityName"].as_str(), Some("平方完成"));
    assert!(
        !saved_job["insertedAt"].as_str().unwrap_or_default().is_empty(),
        "一括保存したジョブは挿入済みと分かること"
    );
}

#[test]
fn completed_ai_answer_can_be_inserted_into_its_source_problem_once() {
    let (_dir, state) = make_state();
    let (problem_id, job_id) = {
        let conn = state.conn.lock().unwrap();
        conn.execute("INSERT INTO subjects (name) VALUES ('数学')", [])
            .unwrap();
        let subject_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO fields (subject_id, name) VALUES (?1, '数学I')",
            [subject_id],
        )
        .unwrap();
        let field_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO units (field_id, name) VALUES (?1, '二次関数')",
            [field_id],
        )
        .unwrap();
        let unit_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO problems (unit_id, title, statement_latex, answer_latex, answer_completed, explanation_completed, created_at, updated_at)
             VALUES (?1, '最大値', '$x^2$の最大値を求めよ。', '既存の解答', 1, 1, ?2, ?2)",
            rusqlite::params![unit_id, kyozai_kobo_lib::db::now_str()],
        )
        .unwrap();
        let problem_id = conn.last_insert_rowid();
        drop(conn);

        let job_id = insert_completed_ai_job(&state, "completed", "ok");
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs
             SET conversion_mode='generate_answer', target_entity_type='problem',
                 target_entity_id=?1, target_field='answer_latex'
             WHERE id=?2",
            rusqlite::params![problem_id, job_id],
        )
        .unwrap();
        (problem_id, job_id)
    };

    let inserted = dispatch(
        &state,
        "ai_insert_into_target_problem",
        json!({"jobId": job_id, "confirmed": true}),
        Origin::Desktop,
    )
    .expect("AI一覧から元問題の解答へ挿入できること");
    assert_eq!(inserted["problemId"].as_i64(), Some(problem_id));
    assert_eq!(inserted["field"].as_str(), Some("answer_latex"));

    let conn = state.conn.lock().unwrap();
    let (answer, answer_completed, explanation_completed, version): (String, i64, i64, i64) = conn
        .query_row(
            "SELECT answer_latex, answer_completed, explanation_completed, version FROM problems WHERE id=?1",
            [problem_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(answer, "既存の解答\n$x^2$");
    assert_eq!(answer_completed, 0, "AIで解答を変更したら完成状態を解除すること");
    assert_eq!(
        explanation_completed, 0,
        "解答変更時は解説の完成状態も解除すること"
    );
    assert_eq!(version, 2, "直接挿入でも問題の版を進めること");
    let history_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM problem_versions WHERE problem_id=?1",
            [problem_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(history_count, 1, "挿入前の問題を履歴へ保存すること");
    drop(conn);
    let inserted_job = kyozai_kobo_lib::ai::get_job(&state, job_id).unwrap();
    assert_eq!(inserted_job["targetEntityName"].as_str(), Some("最大値"));
    assert!(
        !inserted_job["insertedAt"].as_str().unwrap_or_default().is_empty(),
        "問題へ直接挿入したジョブは挿入済みと分かること"
    );

    let duplicate = dispatch(
        &state,
        "ai_insert_into_target_problem",
        json!({"jobId": job_id, "confirmed": true}),
        Origin::Desktop,
    );
    assert!(
        duplicate
            .unwrap_err()
            .contains("すでに挿入"),
        "同じ生成結果の二重挿入を拒否すること"
    );
}

#[test]
fn editor_insertion_records_part_name_and_inserted_state() {
    let (_dir, state) = make_state();
    let job_id = insert_completed_ai_job(&state, "completed", "ok");
    let part_id = {
        let conn = state.conn.lock().unwrap();
        let now = kyozai_kobo_lib::db::now_str();
        conn.execute(
            "INSERT INTO parts (title, latex_source, created_at, updated_at)
             VALUES ('微分係数の基本', '$f''(a)$', ?1, ?1)",
            [now],
        )
        .unwrap();
        conn.last_insert_rowid()
    };

    kyozai_kobo_lib::ai::mark_inserted(
        &state,
        job_id,
        "part".into(),
        part_id,
        "latex_source".into(),
        true,
    )
    .expect("部品エディタへの挿入を記録できること");

    let job = kyozai_kobo_lib::ai::get_job(&state, job_id).unwrap();
    assert_eq!(job["targetEntityType"].as_str(), Some("part"));
    assert_eq!(job["targetEntityName"].as_str(), Some("微分係数の基本"));
    assert!(!job["insertedAt"].as_str().unwrap_or_default().is_empty());
    let event_count: i64 = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM ai_conversion_events
             WHERE job_id=?1 AND kind='inserted'",
            [job_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_count, 1);
}

#[test]
fn ai_source_revision_replaces_problem_and_part_with_version_history() {
    use kyozai_kobo_lib::ai::apply_source_revision;

    let (_dir, state) = make_state();
    let (problem_id, part_id) = {
        let conn = state.conn.lock().unwrap();
        conn.execute("INSERT INTO subjects (name) VALUES ('数学')", [])
            .unwrap();
        let subject_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO fields (subject_id, name) VALUES (?1, '数学I')",
            [subject_id],
        )
        .unwrap();
        let field_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO units (field_id, name) VALUES (?1, '数と式')",
            [field_id],
        )
        .unwrap();
        let unit_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO problems (unit_id, title, statement_latex, created_at, updated_at)
             VALUES (?1, '修正対象', '$x^2$を計算せよ。', ?2, ?2)",
            rusqlite::params![unit_id, kyozai_kobo_lib::db::now_str()],
        )
        .unwrap();
        let problem_id = conn.last_insert_rowid();
        drop(conn);
        let part_id = kyozai_kobo_lib::commands::parts::create_part(
            &state,
            "修正対象の部品".into(),
        )
        .unwrap();
        state
            .conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE parts SET latex_source='$x$について説明する。' WHERE id=?1",
                [part_id],
            )
            .unwrap();
        (problem_id, part_id)
    };

    let problem_job = insert_completed_ai_job(&state, "completed", "ok");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs
             SET conversion_mode='revise_source', options_json=?1,
                 target_entity_type='problem', target_entity_id=?2, target_field='statement_latex'
             WHERE id=?3",
            rusqlite::params![
                json!({
                    "revisionTarget": "problem_statement",
                    "revisionGuidance": "誤字を直す",
                    "revisionSourceVersion": 1
                })
                .to_string(),
                problem_id,
                problem_job
            ],
        )
        .unwrap();
    }
    let applied = apply_source_revision(&state, problem_job, true)
        .expect("問題文をAI修正結果で置き換えられること");
    assert_eq!(applied["entityType"], "problem");
    let (statement, version, history): (String, i64, i64) = {
        let conn = state.conn.lock().unwrap();
        let (statement, version) = conn
            .query_row(
                "SELECT statement_latex, version FROM problems WHERE id=?1",
                [problem_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let history = conn
            .query_row(
                "SELECT COUNT(*) FROM problem_versions WHERE problem_id=?1",
                [problem_id],
                |row| row.get(0),
            )
            .unwrap();
        (statement, version, history)
    };
    assert_eq!(statement, "$x^2$");
    assert_eq!(version, 2);
    assert_eq!(history, 1);
    let applied_problem_job = kyozai_kobo_lib::ai::get_job(&state, problem_job).unwrap();
    assert_eq!(
        applied_problem_job["targetEntityName"].as_str(),
        Some("修正対象")
    );
    assert!(
        !applied_problem_job["insertedAt"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    );

    let part_job = insert_completed_ai_job(&state, "completed", "ok");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs
             SET conversion_mode='revise_source', options_json=?1,
                 target_entity_type='part', target_entity_id=?2, target_field='latex_source'
             WHERE id=?3",
            rusqlite::params![
                json!({
                    "revisionTarget": "part",
                    "revisionGuidance": "式を直す",
                    "revisionSourceVersion": 1
                })
                .to_string(),
                part_id,
                part_job
            ],
        )
        .unwrap();
    }
    let applied = apply_source_revision(&state, part_job, true)
        .expect("部品をAI修正結果で置き換えられること");
    assert_eq!(applied["entityType"], "part");
    let (part_latex, part_version, part_history): (String, i64, i64) = {
        let conn = state.conn.lock().unwrap();
        let (latex, version) = conn
            .query_row(
                "SELECT latex_source, version FROM parts WHERE id=?1",
                [part_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let history = conn
            .query_row(
                "SELECT COUNT(*) FROM part_versions WHERE part_id=?1",
                [part_id],
                |row| row.get(0),
            )
            .unwrap();
        (latex, version, history)
    };
    assert_eq!(part_latex, "$x^2$");
    assert_eq!(part_version, 2);
    assert_eq!(part_history, 1);
    let applied_part_job = kyozai_kobo_lib::ai::get_job(&state, part_job).unwrap();
    assert_eq!(
        applied_part_job["targetEntityName"].as_str(),
        Some("修正対象の部品")
    );
    assert!(
        !applied_part_job["insertedAt"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    );
    let duplicate_apply = apply_source_revision(&state, part_job, true)
        .expect_err("同じ修正結果を二重適用しないこと");
    assert!(duplicate_apply.contains("すでに適用"));

    let stale_job = insert_completed_ai_job(&state, "completed", "ok");
    state
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE ai_conversion_jobs
             SET conversion_mode='revise_source', options_json=?1,
                 target_entity_type='problem', target_entity_id=?2, target_field='statement_latex'
             WHERE id=?3",
            rusqlite::params![
                json!({
                    "revisionTarget": "problem_statement",
                    "revisionGuidance": "誤字を直す",
                    "revisionSourceVersion": 1
                })
                .to_string(),
                problem_id,
                stale_job
            ],
        )
        .unwrap();
    let stale = apply_source_revision(&state, stale_job, true)
        .expect_err("開始後に更新された問題を古い修正案で上書きしないこと");
    assert!(stale.contains("更新されています"));
}

#[test]
fn ai_job_content_warning_requires_confirmation_but_does_not_block_confirmed_save() {
    let (_dir, state) = make_state();
    let job_id = insert_completed_ai_job(&state, "completed", "ok");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs SET warnings_json=?1 WHERE id=?2",
            rusqlite::params![
                json!([{
                    "code": "TRAJECTORY_POINT_NAME",
                    "severity": "error",
                    "message": "問題文の点名と座標文字を保持してください"
                }])
                .to_string(),
                job_id
            ],
        )
        .unwrap();
    }

    let format_error = dispatch(
        &state,
        "ai_insert_into_target_problem",
        json!({"jobId": job_id, "confirmed": false}),
        Origin::Desktop,
    )
    .unwrap_err();
    assert!(format_error.contains("答案形式の検査エラー"));
    assert!(!format_error.contains("危険なLaTeX記述"));
    assert!(format_error.contains("問題文の点名と座標文字を保持してください"));

    let saved_part_id = kyozai_kobo_lib::ai::save_as_part(
        &state,
        job_id,
        "確認済みのAI答案".into(),
        None,
        true,
    )
    .expect("答案内容の警告は、利用者が確認した後の保存を妨げないこと");
    let saved_latex: String = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT latex_source FROM parts WHERE id=?1",
            [saved_part_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(saved_latex, "$x^2$");

    let security_job_id = insert_completed_ai_job(&state, "completed", "ok");

    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs SET output_latex='\\write18{blocked}' WHERE id=?1",
            [security_job_id],
        )
        .unwrap();
    }
    let security_error = dispatch(
        &state,
        "ai_insert_into_target_problem",
        json!({"jobId": security_job_id, "confirmed": true}),
        Origin::Desktop,
    )
    .unwrap_err();
    assert!(security_error.contains("危険なLaTeX記述"));
    assert!(!security_error.contains("答案形式の検査エラー"));
}

/// 再コンパイル後にジョブが「コンパイル中」のまま残らないこと（回帰: status復元漏れ）
#[test]
fn recompile_restores_completed_status() {
    let (_dir, state) = make_state();
    let job_id = insert_completed_ai_job(&state, "completed", "ok");
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_conversion_jobs SET warnings_json=?1 WHERE id=?2",
            rusqlite::params![
                json!([{
                    "code": "TRAJECTORY_POINT_NAME",
                    "severity": "error",
                    "message": "古い検査規則による誤警告"
                }])
                .to_string(),
                job_id
            ],
        )
        .unwrap();
    }

    let result = kyozai_kobo_lib::ai::recompile_job(&state, job_id).unwrap();
    assert_eq!(
        result.get("status").and_then(Value::as_str),
        Some("completed"),
        "再コンパイル後にstatusが完了へ戻っていない: {result}"
    );
    // TeX未導入環境ではskipped、導入済みならok/failedのいずれかになる
    let compile_status = result
        .get("compileStatus")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(
        ["ok", "failed", "skipped"].contains(&compile_status),
        "compileStatusが不正: {compile_status}"
    );
    let warnings_json: String = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT warnings_json FROM ai_conversion_jobs WHERE id=?1",
            [job_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !warnings_json.contains("古い検査規則による誤警告"),
        "再コンパイル時に保存済みの古い警告が再評価されていない"
    );
    // 完了扱いに戻っているため、編集・削除など完了前提の操作が可能
    kyozai_kobo_lib::ai::update_job_latex(&state, job_id, "$y^2$".into())
        .expect("再コンパイル後にLaTeX編集がブロックされている");
}

/// 起動時修復: 変換・コンパイル結果が揃った'compiling'残骸は完了へ復旧し、
/// 結果のない実行中ジョブは失敗へ畳む
#[test]
fn startup_repair_recovers_stuck_compiling_jobs() {
    let (_dir, state) = make_state();
    let stuck = insert_completed_ai_job(&state, "compiling", "ok");
    let interrupted = insert_completed_ai_job(&state, "converting", "none");

    {
        let conn = state.conn.lock().unwrap();
        kyozai_kobo_lib::ai::repair_interrupted_jobs(&conn);
    }

    let conn = state.conn.lock().unwrap();
    let status_of = |id: i64| -> String {
        conn.query_row(
            "SELECT status FROM ai_conversion_jobs WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(status_of(stuck), "completed", "compiling残骸が復旧されない");
    assert_eq!(status_of(interrupted), "failed", "実行中ジョブが失敗へ畳まれない");
}

#[test]
fn interrupted_chat_action_group_is_applied_and_can_be_undone() {
    let (_dir, state) = make_state();
    let problem_id = insert_chat_test_problem(&state);
    let session = kyozai_kobo_lib::ai_chat::create_session(&state, Some(json!({}))).unwrap();
    let session_id = session["id"].as_str().unwrap().to_string();

    kyozai_kobo_lib::ai_chat::execute_non_ai_tool_for_test(
        &state,
        &session_id,
        "update_problem",
        json!({"problem_id":problem_id,"title":"途中まで変更済み"}),
    )
    .unwrap();
    let group_id: i64 = {
        let conn = state.conn.lock().unwrap();
        let id = conn
            .query_row(
                "SELECT id FROM ai_action_groups WHERE session_id=?1 ORDER BY id DESC LIMIT 1",
                [&session_id],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "UPDATE ai_action_groups SET status='running' WHERE id=?1",
            [id],
        )
        .unwrap();
        conn.execute(
            "UPDATE ai_chat_sessions SET status='cancelling' WHERE id=?1",
            [&session_id],
        )
        .unwrap();
        id
    };

    kyozai_kobo_lib::ai_chat::repair_interrupted_sessions(&state);
    let group_status: String = state
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT status FROM ai_action_groups WHERE id=?1",
            [group_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(group_status, "applied");

    kyozai_kobo_lib::ai_chat::undo(&state, session_id).unwrap();
    let restored = kyozai_kobo_lib::commands::problems::get_problem(&state, problem_id).unwrap();
    assert_eq!(restored.title, "元の題名");
}

#[test]
fn dangerous_generated_problem_latex_is_rejected_before_save() {
    let (_dir, state) = make_state();
    let problem_id = insert_chat_test_problem(&state);
    let before = kyozai_kobo_lib::commands::problems::get_problem(&state, problem_id).unwrap();

    let blocked = kyozai_kobo_lib::ai_chat::validate_generated_problem_latex_for_test(
        &json!({"compileStatus":"blocked","outputLatex":"\\write18{blocked}"}),
        "解答",
    )
    .unwrap_err();
    assert_eq!(
        blocked,
        "危険なLaTeX記述が残っています。修正して再コンパイルしてください"
    );
    let rescanned = kyozai_kobo_lib::ai_chat::validate_generated_problem_latex_for_test(
        &json!({"compileStatus":"ok","outputLatex":"\\input{secret}"}),
        "解説",
    )
    .unwrap_err();
    assert!(rescanned.contains("解説に危険なLaTeX"));

    let after = kyozai_kobo_lib::commands::problems::get_problem(&state, problem_id).unwrap();
    assert_eq!(after.answer_latex, before.answer_latex);
    assert_eq!(after.explanation_latex, before.explanation_latex);
    assert_eq!(after.version, before.version);
}

#[test]
fn interrupted_chat_session_is_repaired_and_accepts_new_message_validation() {
    let (_dir, state) = make_state();
    let session = kyozai_kobo_lib::ai_chat::create_session(&state, Some(json!({}))).unwrap();
    let session_id = session["id"].as_str().unwrap().to_string();
    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_chat_sessions SET status='running' WHERE id=?1",
            [&session_id],
        )
        .unwrap();
    }

    kyozai_kobo_lib::ai_chat::repair_interrupted_sessions(&state);
    let status = dispatch(
        &state,
        "ai_chat_session_status",
        json!({"sessionId":session_id}),
        Origin::Desktop,
    )
    .unwrap();
    assert_eq!(status["status"], "failed");
    let repaired = kyozai_kobo_lib::ai_chat::get_session(&state, &session_id).unwrap();
    assert_eq!(repaired["lastError"], "アプリ再起動により中断されました");

    // 存在しない添付の検査まで進めれば、古いrunning状態による拒否は解消している。
    let error = kyozai_kobo_lib::ai_chat::send_message(
        &state,
        session_id,
        "再開".into(),
        vec!["missing.png".into()],
        None,
    )
    .unwrap_err();
    assert!(error.contains("画像 missing.png が見つかりません"));
    assert!(!error.contains("AIが処理中"));
}

#[test]
fn chat_title_only_update_preserves_problem_completion_flags() {
    let (_dir, state) = make_state();
    let problem_id = insert_chat_test_problem(&state);
    let session = kyozai_kobo_lib::ai_chat::create_session(&state, Some(json!({}))).unwrap();
    let session_id = session["id"].as_str().unwrap();

    kyozai_kobo_lib::ai_chat::execute_non_ai_tool_for_test(
        &state,
        session_id,
        "update_problem",
        json!({"problem_id":problem_id,"title":"題名だけ変更"}),
    )
    .unwrap();
    let updated = kyozai_kobo_lib::commands::problems::get_problem(&state, problem_id).unwrap();
    assert_eq!(updated.title, "題名だけ変更");
    assert!(updated.answer_completed, "解答の完成フラグが落ちている");
    assert!(updated.explanation_completed, "解説の完成フラグが落ちている");
}

#[test]
fn invalid_ai_chat_web_settings_return_specific_errors() {
    let (_dir, state) = make_state();
    for (key, value, expected) in [
        ("ai_chat_enabled", "yes", "AIチャットの有効設定"),
        ("ai_chat_execution_mode", "dangerous", "AIチャットの実行モード"),
        ("ai_chat_max_tool_calls", "100", "AIチャットのTool数上限"),
    ] {
        let error = kyozai_kobo_lib::commands::settings::set_web_settings(
            &state,
            std::collections::HashMap::from([(key.to_string(), value.to_string())]),
        )
        .unwrap_err();
        assert!(error.contains(expected), "{key}のエラー文が不適切: {error}");
        assert!(!error.contains("ブラウザから変更できない設定"));
    }
}

#[test]
fn chat_history_commands_reject_invalid_session_ids() {
    let (_dir, state) = make_state();
    for error in [
        kyozai_kobo_lib::ai_chat::undo(&state, "../bad".into()).unwrap_err(),
        kyozai_kobo_lib::ai_chat::redo(&state, "../bad".into()).unwrap_err(),
        kyozai_kobo_lib::ai_chat::get_history(&state, "../bad".into(), None).unwrap_err(),
    ] {
        assert_eq!(error, "チャットIDが不正です");
    }
}
