//! バックアップの強化:
//! - SQLiteのオンラインバックアップAPIによる安全なコピー（サーバー停止不要）
//! - 教材アセットフォルダのミラーコピー
//! - 世代一覧・復元

use crate::state::{err_str, AppState};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const ASSET_DIRS: &[&str] = &[
    "attachments",
    "part_attachments",
    "template_assets",
    "graph_assets",
];

fn backups_dir(state: &AppState) -> PathBuf {
    let dir = state.data_dir.join("backups");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn copy_dir_all(src: &Path, dest: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}

/// 手動バックアップ: DB（オンラインバックアップ）＋アセットのミラー
pub fn backup_now(state: &AppState) -> Result<Value, String> {
    let dir = backups_dir(state);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let nonce = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let base = format!("kyozai-kobo-manual-{}-{}", stamp, nonce);
    let db_dest = dir.join(format!("{}.db", base));
    let assets_dest = dir.join(format!("{}.assets", base));

    {
        let conn = state.conn.lock().map_err(err_str)?;
        let mut dest = Connection::open(&db_dest).map_err(err_str)?;
        let bk = rusqlite::backup::Backup::new(&conn, &mut dest).map_err(err_str)?;
        bk.run_to_completion(64, std::time::Duration::from_millis(5), None)
            .map_err(err_str)?;
    }
    validate_backup_file(&db_dest)?;

    let mut copied = vec![];
    for name in ASSET_DIRS {
        let src = state.data_dir.join(name);
        if src.exists() {
            if let Err(error) = copy_dir_all(&src, &assets_dest.join(name)) {
                std::fs::remove_file(&db_dest).ok();
                std::fs::remove_dir_all(&assets_dest).ok();
                return Err(format!("アセットのバックアップに失敗しました: {}", error));
            }
            copied.push(*name);
        }
    }

    // 手動バックアップは20世代まで
    let mut manuals: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(err_str)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("kyozai-kobo-manual-"))
                .unwrap_or(false)
                && p.extension().map(|e| e == "db").unwrap_or(false)
        })
        .collect();
    manuals.sort();
    while manuals.len() > 20 {
        let old = manuals.remove(0);
        let old_assets = old.with_extension("assets");
        std::fs::remove_file(old).ok();
        std::fs::remove_dir_all(old_assets).ok();
    }

    Ok(json!({
        "dbFile": db_dest.file_name().map(|n| n.to_string_lossy().to_string()),
        "assetsMirrored": copied,
    }))
}

pub fn list_backups(state: &AppState) -> Result<Value, String> {
    let dir = backups_dir(state);
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().map(|x| x == "db").unwrap_or(false)
            })
            .collect();
        files.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
        for e in files {
            let meta = e.metadata().ok();
            out.push(json!({
                "fileName": e.file_name().to_string_lossy(),
                "sizeBytes": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                "modified": meta
                    .and_then(|m| m.modified().ok())
                    .map(|t| chrono::DateTime::<chrono::Local>::from(t)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string())
                    .unwrap_or_default(),
            }));
        }
    }
    Ok(Value::Array(out))
}

fn validate_backup_file(path: &Path) -> Result<(), String> {
    let test = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("バックアップを開けません: {}", e))?;
    let integrity: String = test
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| format!("整合性チェックに失敗しました: {}", e))?;
    if integrity != "ok" {
        return Err(format!("バックアップDBの整合性が不正です: {}", integrity));
    }
    let required: i64 = test
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='table' AND name IN
             ('subjects','fields','units','problems','projects','templates')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("バックアップのスキーマを確認できません: {}", e))?;
    if required != 6 {
        return Err("教材工房の必須テーブルが不足しています".into());
    }
    Ok(())
}

/// バックアップからDBを復元する。
/// 現在のDBは pre-restore として退避してから置き換え、接続を開き直す。
pub fn restore_backup(state: &AppState, file_name: &str) -> Result<(), String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("不正なファイル名です".into());
    }
    let src = backups_dir(state).join(file_name);
    if !src.exists() {
        return Err("バックアップファイルが見つかりません".into());
    }
    validate_backup_file(&src)?;

    // 復元中にHTTP/AIから別の書き込み規則が走らないよう、静止状態だけで許可する。
    if state
        .server
        .running
        .lock()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("復元前に教材サーバーを停止してください".into());
    }
    {
        let conn = state.conn.lock().map_err(err_str)?;
        let active_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ai_conversion_jobs
                 WHERE status IN ('queued','preprocessing','waiting_for_codex','converting','validating','compiling')",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if active_jobs > 0 {
            return Err("実行中または待機中のAI変換を完了・キャンセルしてから復元してください".into());
        }
    }

    let db_path = state.data_dir.join("kyozai-kobo.db");
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let nonce = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let safety = backups_dir(state).join(format!(
        "kyozai-kobo-pre-restore-{}-{}.db",
        stamp, nonce
    ));
    let staging = state
        .data_dir
        .join(format!(".kyozai-restore-{}.db", nonce));
    let old_path = state
        .data_dir
        .join(format!(".kyozai-pre-restore-live-{}.db", nonce));

    std::fs::copy(&src, &staging)
        .map_err(|e| format!("復元候補の準備に失敗: {}", e))?;
    if let Err(error) = validate_backup_file(&staging) {
        std::fs::remove_file(&staging).ok();
        return Err(error);
    }

    let mut guard = state.conn.lock().map_err(err_str)?;
    // 現DBはオンラインバックアップAPIで復元点を作る。
    {
        let mut dest = Connection::open(&safety).map_err(err_str)?;
        let backup = rusqlite::backup::Backup::new(&guard, &mut dest).map_err(err_str)?;
        backup
            .run_to_completion(64, std::time::Duration::from_millis(5), None)
            .map_err(err_str)?;
    }
    validate_backup_file(&safety)?;

    // Windowsでは接続中のDBを置換できないため、一時接続へ差し替えて閉じる。
    let old = std::mem::replace(
        &mut *guard,
        Connection::open_in_memory().map_err(err_str)?,
    );
    drop(old);

    if let Err(error) = std::fs::rename(&db_path, &old_path) {
        std::fs::remove_file(&staging).ok();
        *guard = crate::db::open_db(&state.data_dir)
            .map_err(|e| format!("元DBの再オープンにも失敗しました: {}", e))?;
        return Err(format!("現DBの退避に失敗: {}", error));
    }
    if let Err(error) = std::fs::rename(&staging, &db_path) {
        let _ = std::fs::rename(&old_path, &db_path);
        *guard = crate::db::open_db(&state.data_dir)
            .map_err(|e| format!("元DBの復旧にも失敗しました: {}", e))?;
        return Err(format!("復元DBの設置に失敗: {}", error));
    }

    match crate::db::open_db(&state.data_dir) {
        Ok(restored) => {
            *guard = restored;
        }
        Err(error) => {
            let failed = state
                .data_dir
                .join(format!(".kyozai-failed-restore-{}.db", nonce));
            let _ = std::fs::rename(&db_path, &failed);
            let _ = std::fs::rename(&old_path, &db_path);
            *guard = crate::db::open_db(&state.data_dir)
                .map_err(|e| format!("復元失敗後に元DBも開けません: {}", e))?;
            return Err(format!(
                "復元DBを開けなかったため元DBへ戻しました: {}",
                error
            ));
        }
    }
    // 古いバックアップに保存されたセッションを復活させない。
    guard
        .execute("DELETE FROM web_sessions", [])
        .map_err(err_str)?;
    // 新形式の手動バックアップでは、同じ世代のアセットを上書き復元する。
    // 世代に存在しないファイルは削除せず、DBから未参照の安全な孤立ファイルとして残す。
    let asset_source = src.with_extension("assets");
    if asset_source.is_dir() {
        copy_dir_all(&asset_source, &state.data_dir)
            .map_err(|e| format!("DBは復元しましたがアセット復元に失敗しました: {}", e))?;
    }
    std::fs::remove_file(&old_path).ok();
    Ok(())
}
