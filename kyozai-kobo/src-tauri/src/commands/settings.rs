use crate::state::{err_str, AppState};
use rusqlite::params;
use std::collections::HashMap;

const WEB_SETTING_KEYS: &[&str] = &["preview_template_id"];

pub fn get_settings(state: &AppState) -> Result<HashMap<String, String>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn.prepare("SELECT key, value FROM app_settings").map_err(err_str)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(err_str)?;
    let mut map = HashMap::new();
    for row in rows {
        let (k, v) = row.map_err(err_str)?;
        map.insert(k, v);
    }
    map.insert("data_dir".into(), state.data_dir.to_string_lossy().to_string());
    Ok(map)
}

/// ブラウザへ返してよいUI設定だけを返す。
/// 実行ファイル・出力先・data_dir等のホスト情報は外部端末へ公開しない。
pub fn get_web_settings(state: &AppState) -> Result<HashMap<String, String>, String> {
    let all = get_settings(state)?;
    Ok(all
        .into_iter()
        .filter(|(key, _)| WEB_SETTING_KEYS.contains(&key.as_str()))
        .collect())
}

fn validate_web_setting(key: &str, value: &str) -> Result<(), String> {
    if !WEB_SETTING_KEYS.contains(&key) {
        return Err(format!("設定 {} はブラウザから変更できません", key));
    }
    match key {
        "preview_template_id"
            if value.is_empty()
                || (value.len() <= 20 && value.chars().all(|c| c.is_ascii_digit())) =>
        {
            Ok(())
        }
        "preview_template_id" => Err("プレビューテンプレートIDが不正です".into()),
        _ => Err("ブラウザから変更できない設定です".into()),
    }
}

pub fn set_settings(state: &AppState, settings: HashMap<String, String>) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    for (k, v) in settings {
        if k == "data_dir" {
            continue; // データ保存先は表示のみ（DB自身の場所のため）
        }
        tx.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![k, v],
        )
        .map_err(err_str)?;
    }
    tx.commit().map_err(err_str)?;
    Ok(())
}

pub fn set_web_settings(
    state: &AppState,
    settings: HashMap<String, String>,
) -> Result<(), String> {
    for (key, value) in &settings {
        validate_web_setting(key, value)?;
    }
    set_settings(state, settings)
}
