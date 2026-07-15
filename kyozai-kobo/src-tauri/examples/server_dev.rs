//! 開発検証用: GUI（Tauriウィンドウ）なしで教材サーバーだけを起動する。
//! 環境変数:
//!   KK_DATA_DIR … データフォルダ（省略時は %TEMP%\kyozai-server-dev）
//!   KK_PORT     … 待受ポート（省略時は 8760）
//! 実行: cargo run --example server_dev

use kyozai_kobo_lib::state::AppState;
use kyozai_kobo_lib::{ai, commands, db, server};
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    let data_dir = std::env::var("KK_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("kyozai-server-dev"));
    std::fs::create_dir_all(&data_dir).expect("データフォルダを作成できません");

    let conn = db::open_db(&data_dir).expect("DBを開けません");
    commands::templates::seed_default_template(&conn).ok();

    let state = Arc::new(AppState::new(conn, data_dir.clone()));
    if let Ok(port) = std::env::var("KK_PORT") {
        server::set_server_setting(&state, "port", &port).expect("ポート設定に失敗");
    }

    ai::start_worker(state.clone());
    let status = server::start(&state).expect("サーバー起動に失敗");
    // 開発検証用: ペアリングコードを固定できる（本番のTauriアプリでは使われない）
    if let Ok(code) = std::env::var("KK_CODE") {
        *state.server.pairing_code.lock().unwrap() = code;
    }
    println!("DATA_DIR={}", data_dir.display());
    println!("STATUS={}", status);
    println!(
        "PAIRING_CODE={}",
        state.server.pairing_code.lock().unwrap()
    );
    println!("READY");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
