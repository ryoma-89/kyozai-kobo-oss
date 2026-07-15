//! 全サービス関数への単一ディスパッチ層。
//! Tauri（デスクトップ）とHTTP API（ブラウザ）の両方から同じコードパスで呼ばれる。
//! ここでイベント通知（他端末へのリアルタイム反映）も一元的に行う。

use crate::state::{err_str, AppState};
use serde_json::Value;
use std::sync::Arc;

use super::*;

/// 呼び出し元。Webからはローカルファイルパスを受け取るコマンドを禁止する
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Desktop,
    Web,
}

/// ローカルファイルパスを引数に取る等、ブラウザからの直接呼び出しを禁止するコマンド。
/// （Webでは専用のアップロード/ダウンロードエンドポイントを使う）
const WEB_BLOCKED: &[&str] = &[
    "add_attachment",
    "add_part_attachment",
    "add_template_asset",
    "analyze_tex_file",
    "import_template_from_tex",
    "import_template_file",
    "export_template",
    "export_bank",
    "import_bank",
    // Webは /api/files/build を使う（パス引数のコマンドはブラウザへ公開しない）
    "read_compiled_file",
    "detect_graph_app_path",
    "test_graph_integration_settings",
    "start_graph_integration",
    "poll_graph_integration",
    // サーバー管理・バックアップ・システム連携はデスクトップからのみ
    "server_status",
    "server_start",
    "server_stop",
    "server_regen_pairing",
    "server_settings_get",
    "server_settings_set",
    "list_web_devices",
    "revoke_web_device",
    "tailscale_status",
    "autostart_get",
    "autostart_set",
    "backup_now",
    "list_backups",
    "restore_backup",
    "codex_set_path",
];

fn arg<T: serde::de::DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("引数 {} が不正です: {}", name, e))
}

fn ok<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(err_str)
}

/// コマンド名 → 変更イベント種別（読み取り専用コマンドは None）
fn event_kind(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "add_tree_node" | "rename_tree_node" | "delete_tree_node" | "move_tree_node" => "tree",
        "create_problem" | "update_problem" | "duplicate_problem" | "delete_problem"
        | "restore_version" | "move_problems" | "delete_problems" | "import_bank"
        | "add_attachment" | "remove_attachment" | "create_sample_data" => "problems",
        "create_project" | "update_project_meta" | "delete_project" | "duplicate_project"
        | "add_problem_to_project" | "add_part_to_project" | "add_content_item"
        | "update_project_item" | "refresh_item_from_bank" | "refresh_part_item_from_library"
        | "remove_project_item" | "reorder_project_items" | "update_project_settings"
        | "set_project_template" | "refresh_project_template" => "projects",
        "create_part" | "update_part" | "duplicate_part" | "delete_part"
        | "add_part_attachment" | "remove_part_attachment" => "parts",
        "create_template" | "update_template" | "delete_template" | "duplicate_template"
        | "restore_template_version" | "import_template_from_tex" | "import_template_file"
        | "add_template_asset" | "remove_template_asset" => "templates",
        "create_graph" | "update_graph" | "duplicate_graph" | "delete_graph"
        | "restore_graph" | "restore_graph_version" | "save_graph_exports"
        | "complete_graph_web_session" | "ensure_graph_from_asset" => "graphs",
        "insert_graph_to_project" => "projects",
        "set_settings" => "settings",
        _ => return None,
    })
}

/// 成功時イベントへ含める関連ID（既知のIDキーのみ抽出。本文等は含めない）
fn extract_ids(args: &Value) -> Value {
    let keys = [
        "id",
        "unitId",
        "problemId",
        "projectId",
        "partId",
        "templateId",
        "itemId",
        "versionId",
        "attachmentId",
        "assetId",
    ];
    let mut out = serde_json::Map::new();
    if let Some(map) = args.as_object() {
        for k in keys {
            if let Some(v) = map.get(k) {
                if !v.is_null() {
                    out.insert(k.to_string(), v.clone());
                }
            }
        }
        // payload内のidも拾う（update_problem等）
        for nested in ["payload"] {
            if let Some(Value::Object(p)) = map.get(nested) {
                if let Some(v) = p.get("id") {
                    out.insert("id".to_string(), v.clone());
                }
            }
        }
    }
    Value::Object(out)
}

/// コマンド名とJSON引数でサービス関数を呼び出す。
/// 引数キーは既存フロントエンドの invoke と同じ camelCase。
pub fn dispatch(state: &Arc<AppState>, cmd: &str, args: Value, origin: Origin) -> Result<Value, String> {
    if origin == Origin::Web && WEB_BLOCKED.contains(&cmd) {
        return Err("このコマンドはブラウザからは利用できません".into());
    }

    let result = dispatch_inner(state, cmd, &args, origin);

    if result.is_ok() {
        if let Some(kind) = event_kind(cmd) {
            state.emit(kind, cmd, extract_ids(&args));
        }
    }
    result
}

fn dispatch_inner(
    state: &Arc<AppState>,
    cmd: &str,
    args: &Value,
    origin: Origin,
) -> Result<Value, String> {
    match cmd {
        // ---- ツリー ----
        "get_tree" => ok(tree::get_tree(state)?),
        "add_tree_node" => ok(tree::add_tree_node(
            state,
            arg(args, "kind")?,
            arg(args, "parentId")?,
            arg(args, "name")?,
        )?),
        "rename_tree_node" => ok(tree::rename_tree_node(
            state,
            arg(args, "kind")?,
            arg(args, "id")?,
            arg(args, "name")?,
        )?),
        "delete_tree_node" => ok(tree::delete_tree_node(state, arg(args, "kind")?, arg(args, "id")?)?),
        "move_tree_node" => ok(tree::move_tree_node(
            state,
            arg(args, "kind")?,
            arg(args, "id")?,
            arg(args, "delta")?,
        )?),

        // ---- 問題 ----
        "list_problems" => ok(problems::list_problems(state, arg(args, "unitId")?)?),
        "get_problem" => ok(problems::get_problem(state, arg(args, "id")?)?),
        "create_problem" => ok(problems::create_problem(state, arg(args, "unitId")?, arg(args, "title")?)?),
        "update_problem" => ok(problems::update_problem(state, arg(args, "payload")?)?),
        "duplicate_problem" => ok(problems::duplicate_problem(state, arg(args, "id")?)?),
        "delete_problem" => ok(problems::delete_problem(state, arg(args, "id")?)?),
        "list_versions" => ok(problems::list_versions(state, arg(args, "problemId")?)?),
        "get_version" => ok(problems::get_version(state, arg(args, "versionId")?)?),
        "restore_version" => ok(problems::restore_version(state, arg(args, "versionId")?)?),
        "list_all_tags" => ok(problems::list_all_tags(state)?),
        "search_problems" => ok(problems::search_problems(state, arg(args, "query")?)?),

        // ---- 教材プロジェクト ----
        "list_projects" => ok(projects::list_projects(state)?),
        "create_project" => ok(projects::create_project(state, arg(args, "name")?, arg(args, "templateId")?)?),
        "set_project_template" => ok(projects::set_project_template(
            state,
            arg(args, "projectId")?,
            arg(args, "templateId")?,
        )?),
        "refresh_project_template" => ok(projects::refresh_project_template(state, arg(args, "projectId")?)?),
        "update_project_meta" => ok(projects::update_project_meta(
            state,
            arg(args, "id")?,
            arg(args, "name")?,
            arg(args, "description")?,
            arg(args, "expectedVersion")?,
        )?),
        "delete_project" => ok(projects::delete_project(state, arg(args, "id")?)?),
        "duplicate_project" => ok(projects::duplicate_project(state, arg(args, "id")?)?),
        "get_project" => ok(projects::get_project(state, arg(args, "id")?)?),
        "add_problem_to_project" => ok(projects::add_problem_to_project(
            state,
            arg(args, "projectId")?,
            arg(args, "problemId")?,
        )?),
        "add_part_to_project" => ok(projects::add_part_to_project(
            state,
            arg(args, "projectId")?,
            arg(args, "partId")?,
        )?),
        "add_content_item" => ok(projects::add_content_item(
            state,
            arg(args, "projectId")?,
            arg(args, "itemType")?,
            arg(args, "content")?,
            arg(args, "headingLevel")?,
        )?),
        "update_project_item" => {
            let payload = serde_json::from_value(args.clone())
                .map_err(|e| format!("引数が不正です: {}", e))?;
            ok(projects::update_project_item(state, payload)?)
        }
        "refresh_item_from_bank" => ok(projects::refresh_item_from_bank(state, arg(args, "itemId")?)?),
        "refresh_part_item_from_library" => {
            ok(projects::refresh_part_item_from_library(state, arg(args, "itemId")?)?)
        }
        "remove_project_item" => ok(projects::remove_project_item(state, arg(args, "itemId")?)?),
        "reorder_project_items" => ok(projects::reorder_project_items(
            state,
            arg(args, "projectId")?,
            arg(args, "orderedIds")?,
        )?),
        "update_project_settings" => ok(projects::update_project_settings(
            state,
            arg(args, "projectId")?,
            arg(args, "settings")?,
            arg(args, "expectedVersion")?,
        )?),

        // ---- 部品ライブラリ ----
        "search_parts" => ok(parts::search_parts(state, arg(args, "query")?)?),
        "list_all_part_tags" => ok(parts::list_all_part_tags(state)?),
        "list_part_categories" => ok(parts::list_part_categories(state)?),
        "create_part" => ok(parts::create_part(state, arg(args, "title")?)?),
        "get_part" => ok(parts::get_part(state, arg(args, "id")?)?),
        "update_part" => ok(parts::update_part(state, arg(args, "payload")?)?),
        "duplicate_part" => ok(parts::duplicate_part(state, arg(args, "id")?)?),
        "delete_part" => ok(parts::delete_part(state, arg(args, "id")?)?),
        "list_part_versions" => ok(parts::list_part_versions(state, arg(args, "partId")?)?),
        "add_part_attachment" => ok(parts::add_part_attachment(
            state,
            arg(args, "partId")?,
            arg(args, "sourcePath")?,
        )?),
        "remove_part_attachment" => ok(parts::remove_part_attachment(state, arg(args, "attachmentId")?)?),

        // ---- テンプレート ----
        "list_templates" => ok(templates::list_templates(state)?),
        "get_template" => ok(templates::get_template(state, arg(args, "id")?)?),
        "create_template" => ok(templates::create_template(state, arg(args, "name")?)?),
        "update_template" => ok(templates::update_template(state, arg(args, "payload")?)?),
        "delete_template" => ok(templates::delete_template(state, arg(args, "id")?)?),
        "duplicate_template" => ok(templates::duplicate_template(state, arg(args, "id")?)?),
        "list_template_versions" => ok(templates::list_template_versions(state, arg(args, "templateId")?)?),
        "restore_template_version" => ok(templates::restore_template_version(state, arg(args, "versionId")?)?),
        "analyze_tex_file" => ok(templates::analyze_tex_file(state, arg(args, "path")?)?),
        "import_template_from_tex" => ok(templates::import_template_from_tex(
            state,
            arg(args, "path")?,
            arg(args, "name")?,
            arg(args, "mode")?,
        )?),
        "add_template_asset" => ok(templates::add_template_asset(
            state,
            arg(args, "templateId")?,
            arg(args, "sourcePath")?,
        )?),
        "remove_template_asset" => ok(templates::remove_template_asset(state, arg(args, "assetId")?)?),
        "export_template" => ok(templates::export_template(state, arg(args, "id")?, arg(args, "destPath")?)?),
        "import_template_file" => ok(templates::import_template_file(state, arg(args, "path")?)?),

        // ---- LaTeX ----
        "test_compile_template" => ok(latex::test_compile_template(
            state,
            arg(args, "templateId")?,
            arg(args, "kind")?,
        )?),
        "compile_problem_preview" => ok(latex::compile_problem_preview(
            state,
            arg(args, "problemId")?,
            arg(args, "statement")?,
            arg(args, "answer")?,
            arg(args, "explanation")?,
        )?),
        "generate_tex" => ok(latex::generate_tex(state, arg(args, "projectId")?, arg(args, "kind")?)?),
        "export_tex" => ok(latex::export_tex(state, arg(args, "projectId")?, arg(args, "kind")?)?),
        "compile_pdf" => ok(latex::compile_pdf(state, arg(args, "projectId")?, arg(args, "kind")?)?),
        "detect_tex" => ok(latex::detect_tex(state)?),
        "read_compiled_file" => ok(latex::read_compiled_file(state, arg(args, "path")?)?),

        // ---- 問題バンク入出力 ----
        "export_bank" => ok(bank::export_bank(
            state,
            arg(args, "scopeKind")?,
            arg(args, "id")?,
            arg(args, "problemIds")?,
            arg(args, "destPath")?,
        )?),
        "import_bank" => ok(bank::import_bank(state, arg(args, "path")?)?),
        "move_problems" => ok(bank::move_problems(state, arg(args, "problemIds")?, arg(args, "unitId")?)?),
        "delete_problems" => ok(bank::delete_problems(state, arg(args, "problemIds")?)?),

        // ---- 設定・添付・サンプル ----
        "get_settings" => {
            if origin == Origin::Web {
                ok(settings::get_web_settings(state)?)
            } else {
                ok(settings::get_settings(state)?)
            }
        }
        "set_settings" => {
            let values = arg(args, "settings")?;
            if origin == Origin::Web {
                ok(settings::set_web_settings(state, values)?)
            } else {
                ok(settings::set_settings(state, values)?)
            }
        }
        "add_attachment" => ok(attachments::add_attachment(
            state,
            arg(args, "problemId")?,
            arg(args, "sourcePath")?,
        )?),
        "remove_attachment" => ok(attachments::remove_attachment(state, arg(args, "attachmentId")?)?),
        "create_sample_data" => ok(sample::create_sample_data(state)?),
        "has_any_data" => ok(sample::has_any_data(state)?),

        // ---- グラフ作成アプリ連携（デスクトップのみ） ----
        "detect_graph_app_path" => ok(graph_integration::detect_graph_app_path(state)?),
        "test_graph_integration_settings" => ok(graph_integration::test_graph_integration_settings(state)?),
        "start_graph_integration" => ok(graph_integration::start_graph_integration(state, arg(args, "payload")?)?),
        "poll_graph_integration" => ok(graph_integration::poll_graph_integration(
            state,
            arg(args, "requestId")?,
            arg(args, "requestPath")?,
        )?),
        "list_graph_assets" => {
            let mut assets = graph_integration::list_graph_assets(
                state,
                arg(args, "projectId")?,
                arg(args, "problemId")?,
            )?;
            if origin == Origin::Web {
                // AppData配下の絶対パスはブラウザへ返さない。Web側はgraphId/assetIdと
                // 認証付き配信APIだけを利用する。
                for asset in &mut assets {
                    asset.editable_source_path.clear();
                    asset.primary_asset_path.clear();
                    asset.preview_asset_path.clear();
                    asset.latex_source_path.clear();
                }
            }
            ok(assets)
        }

        // ---- グラフ正本・Web編集（デスクトップ/Web共通） ----
        "list_graphs" => ok(graphs::list_graphs(state, arg(args, "includeDeleted")?)?),
        "get_graph" => ok(graphs::get_graph(state, arg(args, "id")?)?),
        "ensure_graph_from_asset" => ok(graphs::ensure_graph_from_asset(state, arg(args, "assetId")?)?),
        "list_graph_versions" => ok(graphs::list_graph_versions(state, arg(args, "graphId")?)?),
        "get_graph_version" => ok(graphs::get_graph_version(state, arg(args, "versionId")?)?),
        "create_graph" => ok(graphs::create_graph(state, arg(args, "payload")?)?),
        "update_graph" => ok(graphs::update_graph(state, arg(args, "payload")?)?),
        "duplicate_graph" => ok(graphs::duplicate_graph(state, arg(args, "id")?)?),
        "delete_graph" => ok(graphs::delete_graph(
            state,
            arg(args, "id")?,
            arg(args, "expectedVersion")?,
        )?),
        "restore_graph" => ok(graphs::restore_graph(state, arg(args, "id")?)?),
        "restore_graph_version" => ok(graphs::restore_graph_version(
            state,
            arg(args, "versionId")?,
            arg(args, "expectedVersion")?,
        )?),
        "save_graph_exports" => ok(graphs::save_graph_exports(
            state,
            arg(args, "id")?,
            arg(args, "files")?,
        )?),
        "insert_graph_to_project" => ok(graphs::insert_graph_to_project(
            state,
            arg(args, "id")?,
            arg(args, "projectId")?,
            arg(args, "expectedProjectVersion")?,
        )?),
        "create_graph_web_session" => ok(graph_web::create_graph_web_session(
            state,
            arg(args, "payload")?,
        )?),
        "get_graph_web_session" => ok(graph_web::get_graph_web_session(
            state,
            arg(args, "sessionId")?,
        )?),
        "cancel_graph_web_session" => ok(graph_web::cancel_graph_web_session(
            state,
            arg(args, "sessionId")?,
        )?),
        "complete_graph_web_session" => ok(graph_web::complete_graph_web_session(
            state,
            arg(args, "sessionId")?,
            arg(args, "graphId")?,
            arg(args, "expectedGraphVersion")?,
        )?),

        // ---- 教材サーバー管理 ----
        "server_status" => crate::server::status(state),
        "server_start" => crate::server::start(state),
        "server_stop" => crate::server::stop(state),
        "server_regen_pairing" => crate::server::regen_pairing(state),
        "server_settings_get" => {
            let port = crate::server::configured_port(state);
            let lan = crate::server::get_server_setting(state, "lan_mode").as_deref() == Some("1");
            let autostart_server =
                crate::server::get_server_setting(state, "server_autostart").as_deref() == Some("1");
            ok(serde_json::json!({
                "port": port,
                "lanMode": lan,
                "serverAutostart": autostart_server,
            }))
        }
        "server_settings_set" => {
            if let Some(port) = args.get("port").and_then(|v| v.as_i64()) {
                if !(1024..=65535).contains(&port) {
                    return Err("ポートは1024〜65535で指定してください".into());
                }
                crate::server::set_server_setting(state, "port", &port.to_string())?;
            }
            if let Some(lan) = args.get("lanMode").and_then(|v| v.as_bool()) {
                crate::server::set_server_setting(state, "lan_mode", if lan { "1" } else { "0" })?;
            }
            if let Some(auto) = args.get("serverAutostart").and_then(|v| v.as_bool()) {
                crate::server::set_server_setting(state, "server_autostart", if auto { "1" } else { "0" })?;
            }
            ok(())
        }
        "list_web_devices" => crate::server::list_devices(state),
        "revoke_web_device" => ok(crate::server::revoke_device(state, arg(args, "deviceId")?)?),
        "tailscale_status" => crate::server::system::tailscale_status(state),
        "autostart_get" => ok(crate::server::system::autostart_get()?),
        "autostart_set" => ok(crate::server::system::autostart_set(arg(args, "enabled")?)?),
        "backup_now" => crate::server::backup::backup_now(state),
        "list_backups" => crate::server::backup::list_backups(state),
        "restore_backup" => {
            let r = ok(crate::server::backup::restore_backup(state, &arg::<String>(args, "fileName")?)?);
            if r.is_ok() {
                // 復元後は全データが変わるため各画面へ更新を通知
                for kind in ["tree", "problems", "projects", "parts", "templates", "settings"] {
                    state.emit(kind, "restore_backup", Value::Null);
                }
            }
            r
        }

        // ---- Codex / ChatGPT接続 ----
        "codex_status" => crate::codex::codex_status(state),
        "codex_login_start" => crate::codex::login_start(state, &arg::<String>(args, "method")?),
        "codex_login_cancel" => ok(crate::codex::login_cancel(state)?),
        "codex_logout" => ok(crate::codex::logout(state)?),
        "codex_test" => crate::codex::test_connection(state),
        "codex_set_path" => {
            let path: String = arg(args, "path")?;
            if !path.trim().is_empty() && !std::path::Path::new(&path).exists() {
                return Err("指定されたファイルが存在しません".into());
            }
            let conn = state.conn.lock().map_err(err_str)?;
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES ('codex_path', ?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                rusqlite::params![path],
            )
            .map_err(err_str)?;
            ok(())
        }

        // ---- AI変換ジョブ ----
        "ai_store_input_image" => ok(crate::ai::store_input_image(
            state,
            &arg::<String>(args, "dataBase64")?,
            &arg::<Option<String>>(args, "fileName")?.unwrap_or_default(),
        )?),
        "ai_create_job" => {
            let payload = serde_json::from_value(args.clone())
                .map_err(|e| format!("引数が不正です: {}", e))?;
            crate::ai::create_job(state, payload)
        }
        "ai_get_job" => crate::ai::get_job(state, arg(args, "jobId")?),
        "ai_list_jobs" => crate::ai::list_jobs(state, arg(args, "limit")?),
        "ai_cancel_job" => ok(crate::ai::cancel_job(state, arg(args, "jobId")?)?),
        "ai_retry_job" => crate::ai::retry_job(
            state,
            arg(args, "jobId")?,
            arg(args, "mode")?,
            arg(args, "options")?,
        ),
        "ai_delete_job" => ok(crate::ai::delete_job(state, arg(args, "jobId")?)?),
        "ai_update_job_latex" => ok(crate::ai::update_job_latex(
            state,
            arg(args, "jobId")?,
            arg(args, "latex")?,
        )?),
        "ai_recompile_job" => crate::ai::recompile_job(state, arg(args, "jobId")?),
        "ai_save_as_part" => ok(crate::ai::save_as_part(
            state,
            arg(args, "jobId")?,
            arg::<Option<String>>(args, "title")?.unwrap_or_default(),
            arg(args, "category")?,
            arg::<Option<bool>>(args, "confirmed")?.unwrap_or(false),
        )?),
        "ai_save_as_problem" => ok(crate::ai::save_as_problem(
            state,
            arg(args, "jobId")?,
            arg(args, "unitId")?,
            arg::<Option<String>>(args, "title")?.unwrap_or_default(),
            arg::<Option<bool>>(args, "confirmed")?.unwrap_or(false),
        )?),
        "ai_mark_inserted" => ok(crate::ai::mark_inserted(
            state,
            arg(args, "jobId")?,
            arg(args, "entityType")?,
            arg(args, "entityId")?,
            arg(args, "field")?,
            arg::<Option<bool>>(args, "confirmed")?.unwrap_or(false),
        )?),

        _ => Err(format!("不明なコマンド: {}", cmd)),
    }
}
