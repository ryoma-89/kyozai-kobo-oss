pub mod ai;
pub mod codex;
pub mod commands;
pub mod db;
pub mod models;
pub mod server;
pub mod state;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

/// 全サービス関数への単一入口（フロントエンドは invoke("dispatch", {cmd, args}) を使う）
#[tauri::command]
async fn dispatch(
    state: tauri::State<'_, Arc<AppState>>,
    cmd: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // LaTeXコンパイルやバックアップなどの同期I/OをWebViewのイベント処理から分離する。
    // Web API側も同じ理由でspawn_blockingを使っており、デスクトップ版と挙動を揃える。
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        commands::dispatch::dispatch(&state, &cmd, args, commands::dispatch::Origin::Desktop)
    })
    .await
    .map_err(|_| "内部処理スレッドでエラーが発生しました".to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 二重起動防止: ×でトレイへ格納した状態のままスタートメニュー等から
        // 再起動すると、同一SQLiteを2プロセスで開いてしまう。2つ目の起動は
        // 既存インスタンスのウィンドウを前面表示して即終了する。
        // （公式ドキュメントの推奨どおり最初のpluginとして登録する）
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("アプリデータフォルダを取得できません");
            std::fs::create_dir_all(&data_dir)?;

            let conn = db::open_db(&data_dir).expect("データベースを開けません");

            // 初期テンプレートの投入（無い場合のみ）
            commands::templates::seed_default_template(&conn).ok();

            // 自動バックアップ（設定で無効化されていなければ実行）
            let auto_backup: String = conn
                .query_row(
                    "SELECT value FROM app_settings WHERE key='auto_backup'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| "1".to_string());
            if auto_backup != "0" {
                db::backup_db(&data_dir);
            }

            // 初回起動時にTeXコマンドを自動検出して設定に保存
            let has_tex_setting: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM app_settings WHERE key IN ('uplatex_path','dvipdfmx_path')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if has_tex_setting == 0 {
                for (name, key) in [("uplatex", "uplatex_path"), ("dvipdfmx", "dvipdfmx_path")] {
                    if let Some(p) = commands::latex::resolve_tex_cmd(&conn, name, key) {
                        conn.execute(
                            "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
                            rusqlite::params![key, p.to_string_lossy().to_string()],
                        )
                        .ok();
                    }
                }
            }

            let mut st = AppState::new(conn, data_dir);
            st.documents_dir = app.path().document_dir().ok();
            st.resource_dir = app.path().resource_dir().ok();
            let state = Arc::new(st);

            // 変更イベントをデスクトップUIへ転送（Web側はSSEで受け取る）
            {
                let handle = app.handle().clone();
                let mut rx = state.events.subscribe();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    loop {
                        match rx.recv().await {
                            Ok(ev) => {
                                let _ = handle.emit("app-event", &ev);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }

            // タスクトレイ常駐（開く / バックアップ / 終了）
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::TrayIconBuilder;
                let open_item = MenuItem::with_id(app, "open", "教材工房を開く", true, None::<&str>)?;
                let backup_item = MenuItem::with_id(app, "backup", "今すぐバックアップ", true, None::<&str>)?;
                let data_item = MenuItem::with_id(app, "data_dir", "データ保存先を開く", true, None::<&str>)?;
                let quit_item = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open_item, &backup_item, &data_item, &quit_item])?;
                if let Some(icon) = app.default_window_icon().cloned() {
                    TrayIconBuilder::new()
                        .icon(icon)
                        .menu(&menu)
                        .tooltip("教材工房（教材サーバー）")
                        .on_menu_event(|app, event| {
                            let state = app.try_state::<Arc<AppState>>();
                            match event.id.as_ref() {
                                "open" => {
                                    if let Some(w) = app.get_webview_window("main") {
                                        let _ = w.show();
                                        let _ = w.unminimize();
                                        let _ = w.set_focus();
                                    }
                                }
                                "backup" => {
                                    if let Some(state) = state {
                                        match server::backup::backup_now(&state) {
                                            Ok(_) => state.server.log_line("トレイからバックアップを実行しました"),
                                            Err(e) => state.server.log_line(&format!("バックアップ失敗: {}", e)),
                                        }
                                    }
                                }
                                "data_dir" => {
                                    if let Some(state) = state {
                                        use tauri_plugin_opener::OpenerExt;
                                        let _ = app
                                            .opener()
                                            .open_path(state.data_dir.to_string_lossy().to_string(), None::<&str>);
                                    }
                                }
                                "quit" => {
                                    app.exit(0);
                                }
                                _ => {}
                            }
                        })
                        .build(app)?;
                }
            }

            // 一時ビルドフォルダの掃除: コンパイル毎に %TEMP%\kyozai-kobo-build\<uuid> が
            // 作られるが従来削除されず無限に蓄積していた。直近のプレビュー配信を壊さないよう、
            // 48時間より古いフォルダだけを起動時にバックグラウンドで削除する。
            std::thread::spawn(|| {
                let root = std::env::temp_dir().join("kyozai-kobo-build");
                let Ok(entries) = std::fs::read_dir(&root) else { return };
                let cutoff = std::time::SystemTime::now()
                    - std::time::Duration::from_secs(48 * 60 * 60);
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let old = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .map(|m| m < cutoff)
                        .unwrap_or(false);
                    if old {
                        std::fs::remove_dir_all(&path).ok();
                    }
                }
            });

            // AI変換ワーカーを起動
            ai::start_worker(state.clone());

            // サーバー自動起動（設定が有効な場合）
            if server::get_server_setting(&state, "server_autostart").as_deref() == Some("1") {
                if let Err(e) = server::start(&state) {
                    state.server.log_line(&format!("サーバー自動起動に失敗: {}", e));
                }
            }

            app.manage(state);
            Ok(())
        })
        // ×は終了ではなくトレイへ格納する。教材サーバー・AIワーカーは
        // ウィンドウと独立のスレッドで動いているため、hideでもそのまま継続する。
        // 完全終了はトレイメニューの「終了」（app.exit(0)）から行う。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    if let Some(state) = window.app_handle().try_state::<Arc<AppState>>() {
                        state
                            .server
                            .log_line("ウィンドウをトレイへ格納しました（サーバーは継続。終了はトレイの「終了」から）");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            dispatch,
            commands::latex::open_path,
            commands::latex::show_in_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // HTTPサーバーへgraceful shutdownを送り、Codex子プロセスを安全に停止
                if let Some(state) = app_handle.try_state::<Arc<AppState>>() {
                    let _ = server::stop(&state);
                    codex::shutdown(&state);
                }
            }
        });
}
