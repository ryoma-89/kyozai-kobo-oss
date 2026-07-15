//! Web版の認証: ペアリングコード → セッションCookie。
//! - ペアリングコードはサーバー起動時に生成し、デスクトップ画面にのみ表示する
//! - 成功するとコードは再生成される（1回限り）
//! - セッショントークンはSHA-256ハッシュのみDBへ保存する

use crate::db::now_str;
use crate::state::AppState;
use rand::RngCore;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const SESSION_COOKIE: &str = "kk_session";
/// セッション有効期間（日）
const SESSION_DAYS: i64 = 180;

pub fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    format!("{:x}", h.finalize())
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn generate_pairing_code() -> String {
    let mut rng = rand::thread_rng();
    let n = rng.next_u32() % 100_000_000;
    format!("{:08}", n)
}

fn expires_at() -> String {
    (chrono::Local::now() + chrono::Duration::days(SESSION_DAYS))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// ペアリング成功時: 端末を登録しセッショントークンを発行して返す
pub fn create_session(state: &Arc<AppState>, device_name: &str, user_agent: &str) -> Result<String, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let now = now_str();
    conn.execute(
        "INSERT INTO trusted_devices (device_name, user_agent, created_at, last_seen_at) VALUES (?1, ?2, ?3, ?3)",
        params![device_name, user_agent, now],
    )
    .map_err(|e| e.to_string())?;
    let device_id = conn.last_insert_rowid();
    let token = generate_token();
    conn.execute(
        "INSERT INTO web_sessions (token_hash, device_id, created_at, expires_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?3)",
        params![hash_token(&token), device_id, now, expires_at()],
    )
    .map_err(|e| e.to_string())?;
    Ok(token)
}

/// Cookieのトークンからセッションを検証。有効なら device_id を返し last_seen を更新
pub fn validate_session(state: &Arc<AppState>, token: &str) -> Option<i64> {
    let conn = state.conn.lock().ok()?;
    let hash = hash_token(token);
    let now = now_str();
    let row: Option<(i64, i64, String)> = conn
        .query_row(
            "SELECT s.id, s.device_id, s.expires_at FROM web_sessions s
             JOIN trusted_devices d ON d.id = s.device_id
             WHERE s.token_hash=?1 AND d.revoked=0",
            params![hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let (session_id, device_id, expires) = row?;
    if expires < now {
        conn.execute("DELETE FROM web_sessions WHERE id=?1", params![session_id]).ok();
        return None;
    }
    conn.execute(
        "UPDATE web_sessions SET last_seen_at=?1 WHERE id=?2",
        params![now, session_id],
    )
    .ok();
    conn.execute(
        "UPDATE trusted_devices SET last_seen_at=?1 WHERE id=?2",
        params![now, device_id],
    )
    .ok();
    Some(device_id)
}

pub fn delete_session(state: &Arc<AppState>, token: &str) {
    if let Ok(conn) = state.conn.lock() {
        conn.execute(
            "DELETE FROM web_sessions WHERE token_hash=?1",
            params![hash_token(token)],
        )
        .ok();
    }
}

/// 直近5分以内にアクセスのあったセッション数（接続中端末の目安）
pub fn active_session_count(state: &Arc<AppState>) -> i64 {
    let Ok(conn) = state.conn.lock() else { return 0 };
    let cutoff = (chrono::Local::now() - chrono::Duration::minutes(5))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    conn.query_row(
        "SELECT COUNT(*) FROM web_sessions WHERE last_seen_at >= ?1",
        params![cutoff],
        |r| r.get(0),
    )
    .unwrap_or(0)
}
