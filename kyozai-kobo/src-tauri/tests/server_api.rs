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
        scan_latex_security, scan_solution_layout, scan_solution_notation, validate_output,
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
    assert!(scan_solution_layout(safe_figure).is_empty());
    let unsafe_layout = "\\begin{center}\\includegraphics[width=\\textwidth]{figure.pdf}\\end{center}";
    let layout_warnings = scan_solution_layout(unsafe_layout);
    assert!(layout_warnings.iter().any(|warning| warning.code == "TWO_COLUMN_LAYOUT"));
    assert!(layout_warnings.iter().any(|warning| warning.code == "FIGURE_SIZE"));
    assert!(layout_warnings.iter().all(|warning| warning.severity == "error"));

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
    assert!(scan_solution_notation("$x^2+1=0$").is_empty());
}

#[test]
fn ai_problem_bank_output_supports_multiple_problems_and_rejects_bad_sources() {
    use kyozai_kobo_lib::ai::{
        output_schema, validate_output, FIXED_INSTRUCTIONS, SOLUTION_FIXED_INSTRUCTIONS,
        SOLUTION_REFERENCE_PROFILE,
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
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("着眼点 → 方針 → 手順 → 検算・注意点"));
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
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("論理を飛躍させない"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("複雑な因数分解、置換後の式、場合分けの条件などを突然提示せず"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("その操作が正当である理由"));
    assert!(SOLUTION_FIXED_INSTRUCTIONS.contains("自力で同じ流れを再現できる粒度"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("問題と解答・研究問題の完成解答調"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("板書・授業ノート調"));
    assert!(SOLUTION_REFERENCE_PROFILE.contains("必要条件だけで進めた場合は最後に十分性"));
}

#[test]
fn ai_answer_guidance_has_a_bounded_length() {
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
