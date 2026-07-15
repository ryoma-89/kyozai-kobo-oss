use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::params;

pub fn get_tree(state: &AppState) -> Result<Vec<SubjectNode>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut subjects: Vec<SubjectNode> = {
        let mut stmt = conn
            .prepare("SELECT id, name, sort_order FROM subjects ORDER BY sort_order, id")
            .map_err(err_str)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SubjectNode {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    sort_order: r.get(2)?,
                    fields: vec![],
                })
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    for s in subjects.iter_mut() {
        let mut fields: Vec<FieldNode> = {
            let mut stmt = conn
                .prepare("SELECT id, name, sort_order FROM fields WHERE subject_id=?1 ORDER BY sort_order, id")
                .map_err(err_str)?;
            let rows = stmt
                .query_map(params![s.id], |r| {
                    Ok(FieldNode {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        sort_order: r.get(2)?,
                        units: vec![],
                    })
                })
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        };
        for f in fields.iter_mut() {
            let mut stmt = conn
                .prepare(
                    "SELECT u.id, u.name, u.sort_order,
                            (SELECT COUNT(*) FROM problems p WHERE p.unit_id = u.id)
                     FROM units u WHERE u.field_id=?1 ORDER BY u.sort_order, u.id",
                )
                .map_err(err_str)?;
            f.units = stmt
                .query_map(params![f.id], |r| {
                    Ok(UnitNode {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        sort_order: r.get(2)?,
                        problem_count: r.get(3)?,
                    })
                })
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
        }
        s.fields = fields;
    }
    Ok(subjects)
}

fn table_for(kind: &str) -> Result<(&'static str, Option<&'static str>), String> {
    match kind {
        "subject" => Ok(("subjects", None)),
        "field" => Ok(("fields", Some("subject_id"))),
        "unit" => Ok(("units", Some("field_id"))),
        _ => Err(format!("不明な階層種別: {}", kind)),
    }
}

pub fn add_tree_node(
    state: &AppState,
    kind: String,
    parent_id: Option<i64>,
    name: String,
) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("名前を入力してください".into());
    }
    let (table, parent_col) = table_for(&kind)?;
    let id = match parent_col {
        None => {
            let next: i64 = conn
                .query_row(&format!("SELECT COALESCE(MAX(sort_order),0)+1 FROM {}", table), [], |r| r.get(0))
                .map_err(err_str)?;
            conn.execute(
                &format!("INSERT INTO {} (name, sort_order) VALUES (?1, ?2)", table),
                params![name, next],
            )
            .map_err(err_str)?;
            conn.last_insert_rowid()
        }
        Some(pc) => {
            let pid = parent_id.ok_or("親要素が指定されていません")?;
            let next: i64 = conn
                .query_row(
                    &format!("SELECT COALESCE(MAX(sort_order),0)+1 FROM {} WHERE {}=?1", table, pc),
                    params![pid],
                    |r| r.get(0),
                )
                .map_err(err_str)?;
            conn.execute(
                &format!("INSERT INTO {} ({}, name, sort_order) VALUES (?1, ?2, ?3)", table, pc),
                params![pid, name, next],
            )
            .map_err(err_str)?;
            conn.last_insert_rowid()
        }
    };
    Ok(id)
}

pub fn rename_tree_node(state: &AppState, kind: String, id: i64, name: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("名前を入力してください".into());
    }
    let (table, _) = table_for(&kind)?;
    conn.execute(&format!("UPDATE {} SET name=?1 WHERE id=?2", table), params![name, id])
        .map_err(err_str)?;
    Ok(())
}

pub fn delete_tree_node(state: &AppState, kind: String, id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let (table, _) = table_for(&kind)?;
    conn.execute(&format!("DELETE FROM {} WHERE id=?1", table), params![id])
        .map_err(err_str)?;
    Ok(())
}

/// 同一親内で上下に移動する（delta = -1 or 1）
pub fn move_tree_node(state: &AppState, kind: String, id: i64, delta: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let (table, parent_col) = table_for(&kind)?;
    // 兄弟を並び順で取得
    let siblings: Vec<i64> = match parent_col {
        None => {
            let mut stmt = conn
                .prepare(&format!("SELECT id FROM {} ORDER BY sort_order, id", table))
                .map_err(err_str)?;
            let rows = stmt
                .query_map([], |r| r.get(0))
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        }
        Some(pc) => {
            let parent: i64 = conn
                .query_row(&format!("SELECT {} FROM {} WHERE id=?1", pc, table), params![id], |r| r.get(0))
                .map_err(err_str)?;
            let mut stmt = conn
                .prepare(&format!("SELECT id FROM {} WHERE {}=?1 ORDER BY sort_order, id", table, pc))
                .map_err(err_str)?;
            let rows = stmt
                .query_map(params![parent], |r| r.get(0))
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        }
    };
    let pos = siblings.iter().position(|&x| x == id).ok_or("対象が見つかりません")?;
    let new_pos = pos as i64 + delta;
    if new_pos < 0 || new_pos >= siblings.len() as i64 {
        return Ok(()); // 端なので何もしない
    }
    let mut order = siblings.clone();
    order.swap(pos, new_pos as usize);
    for (i, sid) in order.iter().enumerate() {
        conn.execute(
            &format!("UPDATE {} SET sort_order=?1 WHERE id=?2", table),
            params![i as i64 + 1, sid],
        )
        .map_err(err_str)?;
    }
    Ok(())
}
