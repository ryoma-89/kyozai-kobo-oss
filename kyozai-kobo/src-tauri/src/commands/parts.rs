use crate::db::now_str;
use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
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

pub fn normalize_layout_mode(layout_mode: &str) -> String {
    match layout_mode.trim() {
        "two_column" => "two_column".to_string(),
        _ => "single_column".to_string(),
    }
}

pub(crate) fn plain_preview(source: &str) -> String {
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
    let rows = stmt
        .query_map(params![part_id], |r| {
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

pub(crate) fn save_version(conn: &Connection, part_id: i64) -> rusqlite::Result<()> {
    let tags = tags_of(conn, part_id).unwrap_or_default();
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO part_versions (
            part_id, unit_id, bank_node_id, title, part_type, category, tags_json, latex_source, plain_text_preview,
            description, difficulty_rank, is_required, output_target, layout_mode, version, saved_at
         )
         SELECT id, unit_id, bank_node_id, title, part_type, category, ?2, latex_source, plain_text_preview,
                description, difficulty_rank, is_required, output_target, layout_mode, version, ?3
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
        "SELECT DISTINCT p.id, p.bank_node_id, p.unit_id, COALESCE(u.name,''), f.id, COALESCE(f.name,''),
                s.id, COALESCE(s.name,''), p.title, p.part_type, p.category, p.plain_text_preview,
                p.difficulty_rank, p.is_required, p.output_target, p.layout_mode,
                p.usage_count, p.updated_at, p.version
         FROM parts p
         LEFT JOIN units u ON u.id=p.unit_id
         LEFT JOIN fields f ON f.id=u.field_id
         LEFT JOIN subjects s ON s.id=f.subject_id
         LEFT JOIN bank_nodes bn ON bn.id=p.bank_node_id
         WHERE 1=1",
    );
    let mut args: Vec<Value> = vec![];
    let text = query.text.trim();
    if !text.is_empty() {
        args.push(Value::Text(format!("%{}%", text)));
        let idx = args.len();
        sql.push_str(&format!(
            " AND (p.title LIKE ?{0} OR p.latex_source LIKE ?{0} OR p.description LIKE ?{0}
               OR p.category LIKE ?{0} OR bn.name LIKE ?{0} OR u.name LIKE ?{0} OR f.name LIKE ?{0} OR s.name LIKE ?{0}
               OR EXISTS (SELECT 1 FROM part_tags pt WHERE pt.part_id=p.id AND pt.tag LIKE ?{0}))",
            idx
        ));
    }
    if let Some(bank_node_id) = query.bank_node_id {
        args.push(Value::Integer(bank_node_id));
        if query.include_descendants == Some(true) {
            sql.push_str(&format!(
                " AND p.bank_node_id IN (
                    WITH RECURSIVE subtree(id) AS (
                        SELECT id FROM bank_nodes WHERE id=?{0}
                        UNION ALL
                        SELECT child.id FROM bank_nodes child JOIN subtree parent ON child.parent_id=parent.id
                    ) SELECT id FROM subtree
                )",
                args.len()
            ));
        } else {
            sql.push_str(&format!(" AND p.bank_node_id = ?{}", args.len()));
        }
    }
    if let Some(subject_id) = query.subject_id {
        args.push(Value::Integer(subject_id));
        sql.push_str(&format!(" AND s.id = ?{}", args.len()));
    }
    if let Some(field_id) = query.field_id {
        args.push(Value::Integer(field_id));
        sql.push_str(&format!(" AND f.id = ?{}", args.len()));
    }
    if let Some(unit_id) = query.unit_id {
        args.push(Value::Integer(unit_id));
        sql.push_str(&format!(" AND p.unit_id = ?{}", args.len()));
    }
    if query.unassigned_only == Some(true) {
        sql.push_str(" AND p.bank_node_id IS NULL");
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

    let mut rows: Vec<PartSummary> = {
        let mut stmt = conn.prepare(&sql).map_err(err_str)?;
        let rows = stmt
            .query_map(params_from_iter(args.iter()), |r| {
                Ok(PartSummary {
                    id: r.get(0)?,
                    bank_node_id: r.get(1)?,
                    bank_path: String::new(),
                    unit_id: r.get(2)?,
                    unit_name: r.get(3)?,
                    field_id: r.get(4)?,
                    field_name: r.get(5)?,
                    subject_id: r.get(6)?,
                    subject_name: r.get(7)?,
                    title: r.get(8)?,
                    part_type: r.get(9)?,
                    category: r.get(10)?,
                    tags: vec![],
                    plain_text_preview: r.get(11)?,
                    difficulty_rank: r.get(12)?,
                    is_required: r.get::<_, i64>(13)? != 0,
                    output_target: r.get(14)?,
                    layout_mode: r.get(15)?,
                    usage_count: r.get(16)?,
                    updated_at: r.get(17)?,
                    version: r.get(18)?,
                })
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };

    for part in &mut rows {
        part.tags = tags_of(&conn, part.id).map_err(err_str)?;
        part.bank_path = part
            .bank_node_id
            .map(|id| super::problems::bank_path(&conn, id))
            .transpose()
            .map_err(err_str)?
            .unwrap_or_default();
    }
    Ok(rows)
}

pub fn list_all_part_tags(state: &AppState) -> Result<Vec<String>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT DISTINCT tag FROM part_tags ORDER BY tag")
        .map_err(err_str)?;
    let rows = stmt
        .query_map([], |r| r.get(0))
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
    let rows = stmt
        .query_map([], |r| r.get(0))
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str);
    rows
}

pub fn create_part(state: &AppState, title: String) -> Result<i64, String> {
    create_part_in_bank_node(state, title, None)
}

pub fn create_part_in_bank_node(
    state: &AppState,
    title: String,
    bank_node_id: Option<i64>,
) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let unit_id = bank_node_id
        .map(|id| super::tree::legacy_unit_for_bank_node(&conn, id))
        .transpose()?;
    insert_part(&conn, title, bank_node_id, unit_id)
}

pub fn create_part_in_unit(
    state: &AppState,
    title: String,
    unit_id: Option<i64>,
) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let bank_node_id = unit_id
        .map(|id| super::tree::bank_node_for_legacy_unit(&conn, id))
        .transpose()
        .map_err(err_str)?
        .flatten();
    insert_part(&conn, title, bank_node_id, unit_id)
}

fn insert_part(
    conn: &Connection,
    title: String,
    bank_node_id: Option<i64>,
    unit_id: Option<i64>,
) -> Result<i64, String> {
    let now = now_str();
    let title = if title.trim().is_empty() {
        "新しい部品".to_string()
    } else {
        title.trim().to_string()
    };
    if let Some(unit_id) = unit_id {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM units WHERE id=?1)",
                params![unit_id],
                |row| row.get(0),
            )
            .map_err(err_str)?;
        if !exists {
            return Err("選択した単元が見つかりません".into());
        }
    }
    conn.execute(
        "INSERT INTO parts (unit_id, bank_node_id, title, part_type, latex_source, plain_text_preview, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'text', '', '', ?4, ?4)",
        params![unit_id, bank_node_id, title, now],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

pub fn get_part(state: &AppState, id: i64) -> Result<PartFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut part = conn
        .query_row(
            "SELECT p.id, p.bank_node_id, p.unit_id, COALESCE(u.name,''), f.id, COALESCE(f.name,''),
                    s.id, COALESCE(s.name,''), p.title, p.part_type, p.category,
                    p.latex_source, p.plain_text_preview, p.description, p.difficulty_rank,
                    p.is_required, p.output_target, p.layout_mode, p.usage_count,
                    p.created_at, p.updated_at, p.version
             FROM parts p
             LEFT JOIN units u ON u.id=p.unit_id
             LEFT JOIN fields f ON f.id=u.field_id
             LEFT JOIN subjects s ON s.id=f.subject_id
             WHERE p.id=?1",
            params![id],
            |r| {
                Ok(PartFull {
                    id: r.get(0)?,
                    bank_node_id: r.get(1)?,
                    bank_path: String::new(),
                    unit_id: r.get(2)?,
                    unit_name: r.get(3)?,
                    field_id: r.get(4)?,
                    field_name: r.get(5)?,
                    subject_id: r.get(6)?,
                    subject_name: r.get(7)?,
                    title: r.get(8)?,
                    part_type: r.get(9)?,
                    category: r.get(10)?,
                    tags: vec![],
                    latex_source: r.get(11)?,
                    plain_text_preview: r.get(12)?,
                    description: r.get(13)?,
                    difficulty_rank: r.get(14)?,
                    is_required: r.get::<_, i64>(15)? != 0,
                    output_target: r.get(16)?,
                    layout_mode: r.get(17)?,
                    usage_count: r.get(18)?,
                    created_at: r.get(19)?,
                    updated_at: r.get(20)?,
                    version: r.get(21)?,
                    attachments: vec![],
                })
            },
        )
        .map_err(err_str)?;
    part.tags = tags_of(&conn, id).map_err(err_str)?;
    part.attachments = attachments_of(&conn, id).map_err(err_str)?;
    part.bank_path = part
        .bank_node_id
        .map(|node_id| super::problems::bank_path(&conn, node_id))
        .transpose()
        .map_err(err_str)?
        .unwrap_or_default();
    Ok(part)
}

/// 部品を更新して新しいversionを返す。競合時は "CONFLICT:<サーバー側version>"
pub fn update_part(state: &AppState, payload: PartUpdate) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let (bank_node_id, unit_id) = if let Some(bank_node_id) = payload.bank_node_id {
        (
            Some(bank_node_id),
            Some(super::tree::legacy_unit_for_bank_node(&conn, bank_node_id)?),
        )
    } else if let Some(unit_id) = payload.unit_id {
        let unit_exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM units WHERE id=?1)",
                params![unit_id],
                |r| r.get(0),
            )
            .map_err(err_str)?;
        if unit_exists == 0 {
            return Err("指定された単元が見つかりません".to_string());
        }
        (
            super::tree::bank_node_for_legacy_unit(&conn, unit_id).map_err(err_str)?,
            Some(unit_id),
        )
    } else {
        (None, None)
    };
    let current: i64 = conn
        .query_row(
            "SELECT version FROM parts WHERE id=?1",
            params![payload.id],
            |r| r.get(0),
        )
        .map_err(err_str)?;
    if let Some(expected) = payload.expected_version {
        if expected != current {
            return Err(format!("CONFLICT:{}", current));
        }
    }
    save_version(&conn, payload.id).map_err(err_str)?;
    let part_type = normalize_part_type(&payload.part_type);
    let output_target = normalize_output_target(&payload.output_target);
    let layout_mode = normalize_layout_mode(&payload.layout_mode);
    let preview = plain_preview(&payload.latex_source);
    let rank = super::problems::normalize_rank(payload.difficulty_rank);
    conn.execute(
        "UPDATE parts SET unit_id=?1, bank_node_id=?2, title=?3, part_type=?4, category=?5, latex_source=?6,
                plain_text_preview=?7, description=?8, difficulty_rank=?9, is_required=?10,
                output_target=?11, layout_mode=?12, updated_at=?13, version=version+1
         WHERE id=?14",
        params![
            unit_id,
            bank_node_id,
            payload.title.trim(),
            part_type,
            payload.category.trim(),
            payload.latex_source,
            preview,
            payload.description,
            rank,
            payload.is_required as i64,
            output_target,
            layout_mode,
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
        "INSERT INTO parts (unit_id, bank_node_id, title, part_type, category, latex_source, plain_text_preview, description,
                difficulty_rank, is_required, output_target, layout_mode, usage_count, created_at, updated_at, version)
         SELECT unit_id, bank_node_id, title || ' (コピー)', part_type, category, latex_source, plain_text_preview, description,
                difficulty_rank, is_required, output_target, layout_mode, 0, ?2, ?2, 1
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

pub fn list_part_versions(
    state: &AppState,
    part_id: i64,
) -> Result<Vec<PartVersionSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT id, title, version, saved_at FROM part_versions WHERE part_id=?1 ORDER BY id DESC")
        .map_err(err_str)?;
    let rows = stmt
        .query_map(params![part_id], |r| {
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

pub fn add_part_attachment(
    state: &AppState,
    part_id: i64,
    source_path: String,
) -> Result<PartAttachment, String> {
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
    tx.execute(
        "DELETE FROM part_attachments WHERE id=?1",
        params![attachment_id],
    )
    .map_err(err_str)?;
    tx.execute(
        "UPDATE parts SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now_str(), part_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commands::tree, db};
    use tempdir::TempDir;

    fn test_state() -> (TempDir, AppState) {
        let dir = TempDir::new("kyozai-parts-bank-tree").unwrap();
        let conn = db::open_db(dir.path()).unwrap();
        let data_dir = dir.path().to_path_buf();
        (dir, AppState::new(conn, data_dir))
    }

    fn search_query(bank_node_id: Option<i64>, include_descendants: bool) -> PartSearchQuery {
        PartSearchQuery {
            text: String::new(),
            bank_node_id,
            include_descendants: Some(include_descendants),
            subject_id: None,
            field_id: None,
            unit_id: None,
            part_type: None,
            category: None,
            tag: None,
            difficulty_rank: None,
            difficulty_ranks: None,
            required_filter: None,
            unassigned_only: None,
        }
    }

    #[test]
    fn parts_share_arbitrary_depth_bank_nodes_and_search_descendants() {
        let (_dir, state) = test_state();
        let root = tree::create_bank_node(&state, None, "数学".into()).unwrap();
        let mut deep = root;
        for name in ["数III", "積分", "積分評価", "接線利用", "発展"] {
            deep = tree::create_bank_node(&state, Some(deep), name.into()).unwrap();
        }

        let root_part = create_part_in_bank_node(&state, "ルート部品".into(), Some(root)).unwrap();
        let deep_part = create_part_in_bank_node(&state, "深い部品".into(), Some(deep)).unwrap();
        assert_eq!(
            get_part(&state, root_part).unwrap().bank_node_id,
            Some(root)
        );
        assert!(get_part(&state, deep_part)
            .unwrap()
            .bank_path
            .ends_with("積分評価 / 接線利用 / 発展"));

        let direct = search_parts(&state, search_query(Some(root), false)).unwrap();
        assert_eq!(
            direct.iter().map(|part| part.id).collect::<Vec<_>>(),
            vec![root_part]
        );
        let descendants = search_parts(&state, search_query(Some(root), true)).unwrap();
        assert_eq!(descendants.len(), 2);

        let current = get_part(&state, root_part).unwrap();
        update_part(
            &state,
            PartUpdate {
                id: current.id,
                bank_node_id: Some(deep),
                unit_id: current.unit_id,
                title: current.title,
                part_type: current.part_type,
                category: current.category,
                tags: current.tags,
                latex_source: current.latex_source,
                description: current.description,
                difficulty_rank: current.difficulty_rank,
                is_required: current.is_required,
                output_target: current.output_target,
                layout_mode: current.layout_mode,
                expected_version: Some(current.version),
            },
        )
        .unwrap();
        assert_eq!(
            search_parts(&state, search_query(Some(deep), false))
                .unwrap()
                .len(),
            2
        );
        let version_node: Option<i64> = state
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT bank_node_id FROM part_versions WHERE part_id=?1 ORDER BY id DESC LIMIT 1",
                params![root_part],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version_node, Some(root));
    }

    #[test]
    fn deleting_bank_nodes_never_deletes_parts() {
        let (_dir, state) = test_state();
        let root = tree::create_bank_node(&state, None, "数学".into()).unwrap();
        let child = tree::create_bank_node(&state, Some(root), "移動対象".into()).unwrap();
        let moved = create_part_in_bank_node(&state, "親へ".into(), Some(child)).unwrap();
        tree::delete_bank_node(&state, child, "move_to_parent".into()).unwrap();
        assert_eq!(get_part(&state, moved).unwrap().bank_node_id, Some(root));

        let child = tree::create_bank_node(&state, Some(root), "削除対象".into()).unwrap();
        let preserved = create_part_in_bank_node(&state, "未分類へ".into(), Some(child)).unwrap();
        tree::delete_bank_node(&state, child, "delete_all".into()).unwrap();
        let part = get_part(&state, preserved).unwrap();
        assert_eq!(part.bank_node_id, None);
        assert_eq!(part.unit_id, None);
    }
}
