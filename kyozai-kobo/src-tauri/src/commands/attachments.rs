use crate::db::now_str;
use crate::models::Attachment;
use crate::state::{err_str, AppState};
use rusqlite::params;
use std::path::Path;

/// ファイルをアプリ管理領域へコピーして問題に添付する
pub fn add_attachment(state: &AppState, problem_id: i64, source_path: String) -> Result<Attachment, String> {
    let src = Path::new(&source_path);
    if !src.exists() {
        return Err("ファイルが見つかりません".into());
    }
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "pdf"].contains(&ext.as_str()) {
        return Err("対応形式は PNG / JPG / PDF です".into());
    }
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    // LaTeXで扱いやすいようASCIIのみの保存名を付ける
    let stored_name = format!("img{}.{}", uuid::Uuid::new_v4().simple(), ext);
    let dest = state.attachments_dir().join(&stored_name);
    std::fs::copy(src, &dest).map_err(|e| format!("コピーに失敗しました: {}", e))?;

    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let now = now_str();
    if let Err(error) = tx.execute(
        "INSERT INTO attachments (problem_id, file_name, stored_name, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![problem_id, file_name, stored_name, now],
    ) {
        std::fs::remove_file(&dest).ok();
        return Err(error.to_string());
    }
    let attachment_id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE problems SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now, problem_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(Attachment {
        id: attachment_id,
        problem_id,
        file_name,
        stored_name,
        created_at: now,
    })
}

/// 添付を問題から外す（実ファイルは教材スナップショットが参照している可能性があるため残す）
pub fn remove_attachment(state: &AppState, attachment_id: i64) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let problem_id: i64 = tx
        .query_row(
            "SELECT problem_id FROM attachments WHERE id=?1",
            params![attachment_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    tx.execute("DELETE FROM attachments WHERE id=?1", params![attachment_id])
        .map_err(err_str)?;
    tx.execute(
        "UPDATE problems SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now_str(), problem_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}
