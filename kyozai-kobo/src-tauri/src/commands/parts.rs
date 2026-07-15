use crate::db::now_str;
use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::{params, params_from_iter, Connection};
use rusqlite::types::Value;
use std::path::Path;

const PART_TYPES: &[&str] = &[
    "heading",
    "text",
    "notice",
    "hint",
    "example",
    "homework",
    "reflection",
    "box",
    "table",
    "image_block",
    "latex_snippet",
    "page_break",
    "custom",
];

fn normalize_part_type(part_type: &str) -> String {
    let value = part_type.trim();
    if PART_TYPES.contains(&value) {
        value.to_string()
    } else {
        "custom".to_string()
    }
}

pub fn normalize_output_target(target: &str) -> String {
    match target.trim() {
        "problems" | "answers" | "both" | "none" => target.trim().to_string(),
        _ => "both".to_string(),
    }
}

fn plain_preview(source: &str) -> String {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}

pub fn tags_of(conn: &Connection, part_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tag FROM part_tags WHERE part_id=?1 ORDER BY tag")?;
    let rows = stmt.query_map(params![part_id], |r| r.get(0))?.collect();
    rows
}

pub fn attachments_of(conn: &Connection, part_id: i64) -> rusqlite::Result<Vec<PartAttachment>> {
    let mut stmt = conn.prepare(
        "SELECT id, part_id, file_name, stored_name, created_at FROM part_attachments WHERE part_id=?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![part_id], |r| {
        Ok(PartAttachment {
            id: r.get(0)?,
            part_id: r.get(1)?,
            file_name: r.get(2)?,
            stored_name: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?
    .collect();
    rows
}

fn set_tags(conn: &Connection, part_id: i64, tags: &[String]) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM part_tags WHERE part_id=?1", params![part_id])?;
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO part_tags (part_id, tag) VALUES (?1, ?2)",
            params![part_id, tag],
        )?;
    }
    Ok(())
}

fn save_version(conn: &Connection, part_id: i64) -> rusqlite::Result<()> {
    let tags = tags_of(conn, part_id).unwrap_or_default();
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO part_versions (
            part_id, title, part_type, category, tags_json, latex_source, plain_text_preview,
            description, difficulty_rank, is_required, output_target, version, saved_at
         )
         SELECT id, title, part_type, category, ?2, latex_source, plain_text_preview,
                description, difficulty_rank, is_required, output_target, version, ?3
         FROM parts WHERE id=?1",
        params![part_id, tags_json, now_str()],
    )?;
    conn.execute(
        "DELETE FROM part_versions WHERE part_id=?1 AND id NOT IN (
            SELECT id FROM part_versions WHERE part_id=?1 ORDER BY id DESC LIMIT 30)",
        params![part_id],
    )?;
    Ok(())
}

pub fn search_parts(state: &AppState, query: PartSearchQuery) -> Result<Vec<PartSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut sql = String::from(
        "SELECT DISTINCT p.id, p.title, p.part_type, p.category, p.plain_text_preview,
                p.difficulty_rank, p.is_required, p.output_target, p.usage_count, p.updated_at, p.version
         FROM parts p
         WHERE 1=1",
    );
    let mut args: Vec<Value> = vec![];
    let text = query.text.trim();
    if !text.is_empty() {
        args.push(Value::Text(format!("%{}%", text)));
        let idx = args.len();
        sql.push_str(&format!(
            " AND (p.title LIKE ?{0} OR p.latex_source LIKE ?{0} OR p.description LIKE ?{0}
               OR p.category LIKE ?{0}
               OR EXISTS (SELECT 1 FROM part_tags pt WHERE pt.part_id=p.id AND pt.tag LIKE ?{0}))",
            idx
        ));
    }
    if let Some(part_type) = query.part_type.filter(|v| !v.is_empty()) {
        args.push(Value::Text(part_type));
        sql.push_str(&format!(" AND p.part_type = ?{}", args.len()));
    }
    if let Some(category) = query.category.filter(|v| !v.is_empty()) {
        args.push(Value::Text(category));
        sql.push_str(&format!(" AND p.category = ?{}", args.len()));
    }
    let requested_ranks = query.difficulty_ranks.clone().unwrap_or_default();
    let mut ranks: Vec<String> = requested_ranks
        .iter()
        .cloned()
        .filter_map(|r| super::problems::normalize_rank(Some(r)))
        .collect();
    if ranks.is_empty() {
        if let Some(rank) = super::problems::normalize_rank(query.difficulty_rank) {
            ranks.push(rank);
        }
    }
    let include_unset = requested_ranks.iter().any(|r| r == "__unset");
    if !ranks.is_empty() || include_unset {
        let mut clauses = vec![];
        if !ranks.is_empty() {
            let mut placeholders = vec![];
            for rank in ranks {
                args.push(Value::Text(rank));
                placeholders.push(format!("?{}", args.len()));
            }
            clauses.push(format!("p.difficulty_rank IN ({})", placeholders.join(",")));
        }
        if include_unset {
            clauses.push("(p.difficulty_rank IS NULL OR p.difficulty_rank='')".to_string());
        }
        sql.push_str(&format!(" AND ({})", clauses.join(" OR ")));
    }
    match query.required_filter.as_deref() {
        Some("required") => sql.push_str(" AND p.is_required != 0"),
        Some("not_required") => sql.push_str(" AND p.is_required = 0"),
        _ => {}
    }
    if let Some(tag) = query.tag.filter(|v| !v.is_empty()) {
        args.push(Value::Text(tag));
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM part_tags pt WHERE pt.part_id=p.id AND pt.tag = ?{})",
            args.len()
        ));
    }
    sql.push_str(" ORDER BY p.updated_at DESC LIMIT 500");

    let rows: Vec<(i64, String, String, String, String, Option<String>, bool, String, i64, String, i64)> = {
        let mut stmt = conn.prepare(&sql).map_err(err_str)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get::<_, i64>(6)? != 0,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
            ))
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
        rows
    };

    let mut out = vec![];
    for (id, title, part_type, category, plain_text_preview, difficulty_rank, is_required, output_target, usage_count, updated_at, version) in rows {
        out.push(PartSummary {
            id,
            title,
            part_type,
            category,
            tags: tags_of(&conn, id).map_err(err_str)?,
            plain_text_preview,
            difficulty_rank,
            is_required,
            output_target,
            usage_count,
            updated_at,
            version,
        });
    }
    Ok(out)
}

pub fn list_all_part_tags(state: &AppState) -> Result<Vec<String>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT DISTINCT tag FROM part_tags ORDER BY tag")
        .map_err(err_str)?;
    let rows = stmt.query_map([], |r| r.get(0))
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str);
    rows
}

pub fn list_part_categories(state: &AppState) -> Result<Vec<String>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT DISTINCT category FROM parts WHERE category <> '' ORDER BY category")
        .map_err(err_str)?;
    let rows = stmt.query_map([], |r| r.get(0))
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str);
    rows
}

pub fn create_part(state: &AppState, title: String) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    let title = if title.trim().is_empty() {
        "新しい部品".to_string()
    } else {
        title.trim().to_string()
    };
    conn.execute(
        "INSERT INTO parts (title, part_type, latex_source, plain_text_preview, created_at, updated_at)
         VALUES (?1, 'text', '', '', ?2, ?2)",
        params![title, now],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

pub fn get_part(state: &AppState, id: i64) -> Result<PartFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut part = conn
        .query_row(
            "SELECT id, title, part_type, category, latex_source, plain_text_preview, description,
                    difficulty_rank, is_required, output_target, usage_count, created_at, updated_at, version
             FROM parts WHERE id=?1",
            params![id],
            |r| {
                Ok(PartFull {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    part_type: r.get(2)?,
                    category: r.get(3)?,
                    tags: vec![],
                    latex_source: r.get(4)?,
                    plain_text_preview: r.get(5)?,
                    description: r.get(6)?,
                    difficulty_rank: r.get(7)?,
                    is_required: r.get::<_, i64>(8)? != 0,
                    output_target: r.get(9)?,
                    usage_count: r.get(10)?,
                    created_at: r.get(11)?,
                    updated_at: r.get(12)?,
                    version: r.get(13)?,
                    attachments: vec![],
                })
            },
        )
        .map_err(err_str)?;
    part.tags = tags_of(&conn, id).map_err(err_str)?;
    part.attachments = attachments_of(&conn, id).map_err(err_str)?;
    Ok(part)
}

/// 部品を更新して新しいversionを返す。競合時は "CONFLICT:<サーバー側version>"
pub fn update_part(state: &AppState, payload: PartUpdate) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let current: i64 = conn
        .query_row("SELECT version FROM parts WHERE id=?1", params![payload.id], |r| r.get(0))
        .map_err(err_str)?;
    if let Some(expected) = payload.expected_version {
        if expected != current {
            return Err(format!("CONFLICT:{}", current));
        }
    }
    save_version(&conn, payload.id).map_err(err_str)?;
    let part_type = normalize_part_type(&payload.part_type);
    let output_target = normalize_output_target(&payload.output_target);
    let preview = plain_preview(&payload.latex_source);
    let rank = super::problems::normalize_rank(payload.difficulty_rank);
    conn.execute(
        "UPDATE parts SET title=?1, part_type=?2, category=?3, latex_source=?4,
                plain_text_preview=?5, description=?6, difficulty_rank=?7, is_required=?8,
                output_target=?9, updated_at=?10, version=version+1
         WHERE id=?11",
        params![
            payload.title.trim(),
            part_type,
            payload.category.trim(),
            payload.latex_source,
            preview,
            payload.description,
            rank,
            payload.is_required as i64,
            output_target,
            now_str(),
            payload.id
        ],
    )
    .map_err(err_str)?;
    set_tags(&conn, payload.id, &payload.tags).map_err(err_str)?;
    Ok(current + 1)
}

pub fn duplicate_part(state: &AppState, id: i64) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    conn.execute(
        "INSERT INTO parts (title, part_type, category, latex_source, plain_text_preview, description,
                difficulty_rank, is_required, output_target, usage_count, created_at, updated_at, version)
         SELECT title || ' (コピー)', part_type, category, latex_source, plain_text_preview, description,
                difficulty_rank, is_required, output_target, 0, ?2, ?2, 1
         FROM parts WHERE id=?1",
        params![id, now],
    )
    .map_err(err_str)?;
    let new_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO part_tags (part_id, tag) SELECT ?2, tag FROM part_tags WHERE part_id=?1",
        params![id, new_id],
    )
    .map_err(err_str)?;
    conn.execute(
        "INSERT INTO part_attachments (part_id, file_name, stored_name, created_at)
         SELECT ?2, file_name, stored_name, ?3 FROM part_attachments WHERE part_id=?1",
        params![id, new_id, now],
    )
    .map_err(err_str)?;
    Ok(new_id)
}

pub fn delete_part(state: &AppState, id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute("DELETE FROM parts WHERE id=?1", params![id])
        .map_err(err_str)?;
    Ok(())
}

pub fn list_part_versions(state: &AppState, part_id: i64) -> Result<Vec<PartVersionSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT id, title, version, saved_at FROM part_versions WHERE part_id=?1 ORDER BY id DESC")
        .map_err(err_str)?;
    let rows = stmt.query_map(params![part_id], |r| {
        Ok(PartVersionSummary {
            id: r.get(0)?,
            title: r.get(1)?,
            version: r.get(2)?,
            saved_at: r.get(3)?,
        })
    })
    .map_err(err_str)?
    .collect::<Result<_, _>>()
    .map_err(err_str);
    rows
}

pub fn add_part_attachment(state: &AppState, part_id: i64, source_path: String) -> Result<PartAttachment, String> {
    let src = Path::new(&source_path);
    if !src.exists() {
        return Err("ファイルが見つかりません".into());
    }
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "pdf", "svg", "tex", "sty"].contains(&ext.as_str()) {
        return Err("対応形式は PNG / JPG / PDF / SVG / TEX / STY です".into());
    }
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let stored_name = format!("part{}.{}", uuid::Uuid::new_v4().simple(), ext);
    let dest = state.part_attachments_dir().join(&stored_name);
    std::fs::copy(src, &dest).map_err(|e| format!("コピーに失敗しました: {}", e))?;

    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let now = now_str();
    if let Err(error) = tx.execute(
        "INSERT INTO part_attachments (part_id, file_name, stored_name, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![part_id, file_name, stored_name, now],
    ) {
        std::fs::remove_file(&dest).ok();
        return Err(error.to_string());
    }
    let attachment_id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE parts SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now, part_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(PartAttachment {
        id: attachment_id,
        part_id,
        file_name,
        stored_name,
        created_at: now,
    })
}

pub fn remove_part_attachment(state: &AppState, attachment_id: i64) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let part_id: i64 = tx
        .query_row(
            "SELECT part_id FROM part_attachments WHERE id=?1",
            params![attachment_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    tx.execute("DELETE FROM part_attachments WHERE id=?1", params![attachment_id])
        .map_err(err_str)?;
    tx.execute(
        "UPDATE parts SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now_str(), part_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}
