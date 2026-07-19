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
        scan_explanation_structure, scan_latex_security, scan_solution_layout,
        scan_solution_notation, validate_output,
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
    let derivative_range_without_table = r#"$f'(x)=x-1$であり、$f'(x)>0$と$f'(x)<0$となる区間を調べると、値域が求まる。"#;
    assert!(scan_solution_notation(derivative_range_without_table)
        .iter()
        .any(|warning| warning.code == "MISSING_VARIATION_TABLE"));
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
        output_schema, validate_output, BEGINNER_SOLUTION_INSTRUCTIONS,
        BEGINNER_TOPIC_METHOD_GUIDE_INSTRUCTIONS, FIXED_INSTRUCTIONS,
        SOLUTION_FIXED_INSTRUCTIONS, TOPIC_METHOD_GUIDE_INSTRUCTIONS,
        SINGLE_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS, SOLUTION_REFERENCE_PROFILE,
        TRAJECTORY_REGION_INSTRUCTIONS, TWO_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS,
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
            {"title": "二次関数", "statementLatex": "$y=x^2$について答えよ。", "sourceImageIndexes": [1]},
            {"title": "確率", "statementLatex": "さいころを2回投げる。", "sourceImageIndexes": [1, 2]}
        ]
    });
    let parsed = validate_output(&valid.to_string()).expect("複数問題の構造化出力は通ること");
    assert_eq!(parsed.problems.len(), 2);
    assert_eq!(parsed.problems[1].source_image_indexes, vec![1, 2]);

    let mut bad_source = valid.clone();
    bad_source["problems"][0]["sourceImageIndexes"] = json!([0]);
    assert!(validate_output(&bad_source.to_string()).is_err());

    let schema = output_schema();
    let required = schema["required"].as_array().expect("requiredは配列");
    assert!(required.iter().any(|value| value == "problems"));
    assert!(FIXED_INSTRUCTIONS.contains("\\cdots ①"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("高等学校"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("着眼点 → 【定石】 → 方針 → 手順 → 検算・注意点"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("利用できる横幅は常に\\linewidth"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("width=0.65\\linewidth"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("center環境、\\centering、\\textwidth指定は使わない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("主解法を含めて最大3つ"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("【参照する解答】"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("解答にない別解へ勝手に切り替えたり追加したりしない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("高校数学で標準的か判断が分かれる記号"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("$a\\mid b$ は「$b$が$a$で割り切れる」"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("日本の高校の教科書・授業で一般的なもの"));
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
    assert!(TRAJECTORY_REGION_INSTRUCTIONS.contains("異なる2実根をもつ$\\Longleftrightarrow D>0$"));
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
    assert!(SINGLE_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("\\linewidthの横幅を活かし"));
    assert!(SINGLE_COLUMN_SOLUTION_LAYOUT_INSTRUCTIONS.contains("超えそうな場合"));
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
    assert_eq!(version, 4);
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
                source_image_indexes: vec![1],
            },
            ExtractedProblem {
                title: "放物線".into(),
                statement_latex: "放物線$y=x^2$を平行移動せよ。".into(),
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
}

/// 再コンパイル後にジョブが「コンパイル中」のまま残らないこと（回帰: status復元漏れ）
#[test]
fn recompile_restores_completed_status() {
    let (_dir, state) = make_state();
    let job_id = insert_completed_ai_job(&state, "completed", "ok");

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
