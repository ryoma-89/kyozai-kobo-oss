use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

const COMPAT_SUBJECT: &str = "__kyozai_bank_compat_subject__";
const COMPAT_FIELD: &str = "__kyozai_bank_compat_field__";
const COMPAT_UNIT: &str = "__kyozai_bank_compat_unit__";

pub fn get_tree(state: &AppState) -> Result<Vec<SubjectNode>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut subjects: Vec<SubjectNode> = {
        let mut stmt = conn
            .prepare("SELECT id, name, sort_order FROM subjects WHERE name NOT LIKE '__kyozai_bank_%' ORDER BY sort_order, id")
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
                            (SELECT COUNT(*) FROM problems p WHERE p.unit_id = u.id),
                            (SELECT COUNT(*) FROM parts pt WHERE pt.unit_id = u.id)
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
                        part_count: r.get(4)?,
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

#[derive(Clone)]
struct FlatBankNode {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    sort_order: i64,
    problem_count: i64,
    descendant_problem_count: i64,
    part_count: i64,
    descendant_part_count: i64,
    legacy_unit_id: Option<i64>,
}

fn build_bank_children(
    parent_id: Option<i64>,
    flat: &[FlatBankNode],
    visiting: &mut HashSet<i64>,
) -> Vec<BankNode> {
    flat.iter()
        .filter(|node| node.parent_id == parent_id)
        .filter_map(|node| {
            if !visiting.insert(node.id) {
                return None;
            }
            let children = build_bank_children(Some(node.id), flat, visiting);
            visiting.remove(&node.id);
            Some(BankNode {
                id: node.id,
                parent_id: node.parent_id,
                name: node.name.clone(),
                sort_order: node.sort_order,
                problem_count: node.problem_count,
                descendant_problem_count: node.descendant_problem_count,
                part_count: node.part_count,
                descendant_part_count: node.descendant_part_count,
                legacy_unit_id: node.legacy_unit_id,
                children,
            })
        })
        .collect()
}

/// 任意深度の問題バンク正本を返す。
pub fn get_bank_tree(state: &AppState) -> Result<Vec<BankNode>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare(
            "SELECT n.id, n.parent_id, n.name, n.sort_order,
                    (SELECT COUNT(*) FROM problems p WHERE p.bank_node_id=n.id),
                    (WITH RECURSIVE descendants(id) AS (
                         SELECT n.id
                         UNION ALL
                         SELECT child.id FROM bank_nodes child JOIN descendants d ON child.parent_id=d.id
                     )
                     SELECT COUNT(*) FROM problems p WHERE p.bank_node_id IN (SELECT id FROM descendants)),
                    (SELECT COUNT(*) FROM parts part WHERE part.bank_node_id=n.id),
                    (WITH RECURSIVE descendants(id) AS (
                         SELECT n.id
                         UNION ALL
                         SELECT child.id FROM bank_nodes child JOIN descendants d ON child.parent_id=d.id
                     )
                     SELECT COUNT(*) FROM parts part WHERE part.bank_node_id IN (SELECT id FROM descendants)),
                    CASE WHEN n.legacy_kind='unit' THEN n.legacy_id ELSE NULL END
             FROM bank_nodes n
             ORDER BY n.sort_order, n.id",
        )
        .map_err(err_str)?;
    let flat: Vec<FlatBankNode> = stmt
        .query_map([], |row| {
            Ok(FlatBankNode {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                sort_order: row.get(3)?,
                problem_count: row.get(4)?,
                descendant_problem_count: row.get(5)?,
                part_count: row.get(6)?,
                descendant_part_count: row.get(7)?,
                legacy_unit_id: row.get(8)?,
            })
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(build_bank_children(None, &flat, &mut HashSet::new()))
}

fn bank_node_exists(conn: &Connection, id: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM bank_nodes WHERE id=?1)",
        params![id],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
}

pub(crate) fn bank_node_for_legacy_unit(
    conn: &Connection,
    unit_id: i64,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM bank_nodes WHERE legacy_kind='unit' AND legacy_id=?1 LIMIT 1",
        params![unit_id],
        |row| row.get(0),
    )
    .optional()
}

/// 旧 problems.unit_id を満たす互換Unitを解決する。旧Unit由来ノードの子孫なら
/// そのUnitを使い、それ以外はUIへ出さない内部Unitを遅延作成する。
pub(crate) fn legacy_unit_for_bank_node(
    conn: &Connection,
    bank_node_id: i64,
) -> Result<i64, String> {
    if !bank_node_exists(conn, bank_node_id).map_err(err_str)? {
        return Err("移動先の階層が見つかりません".into());
    }
    let inherited = conn
        .query_row(
            "WITH RECURSIVE ancestors(id, parent_id, legacy_kind, legacy_id, depth) AS (
                 SELECT id, parent_id, legacy_kind, legacy_id, 0 FROM bank_nodes WHERE id=?1
                 UNION ALL
                 SELECT parent.id, parent.parent_id, parent.legacy_kind, parent.legacy_id, ancestors.depth+1
                 FROM bank_nodes parent JOIN ancestors ON ancestors.parent_id=parent.id
             )
             SELECT legacy_id FROM ancestors
             WHERE legacy_kind='unit' AND legacy_id IS NOT NULL
             ORDER BY depth LIMIT 1",
            params![bank_node_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(err_str)?;
    if let Some(unit_id) = inherited {
        return Ok(unit_id);
    }

    let subject_id = match conn
        .query_row(
            "SELECT id FROM subjects WHERE name=?1 LIMIT 1",
            params![COMPAT_SUBJECT],
            |row| row.get(0),
        )
        .optional()
        .map_err(err_str)?
    {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO subjects(name, sort_order) VALUES (?1, 2147483647)",
                params![COMPAT_SUBJECT],
            )
            .map_err(err_str)?;
            conn.last_insert_rowid()
        }
    };
    let field_id = match conn
        .query_row(
            "SELECT id FROM fields WHERE subject_id=?1 AND name=?2 LIMIT 1",
            params![subject_id, COMPAT_FIELD],
            |row| row.get(0),
        )
        .optional()
        .map_err(err_str)?
    {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO fields(subject_id, name, sort_order) VALUES (?1, ?2, 1)",
                params![subject_id, COMPAT_FIELD],
            )
            .map_err(err_str)?;
            conn.last_insert_rowid()
        }
    };
    match conn
        .query_row(
            "SELECT id FROM units WHERE field_id=?1 AND name=?2 LIMIT 1",
            params![field_id, COMPAT_UNIT],
            |row| row.get(0),
        )
        .optional()
        .map_err(err_str)?
    {
        Some(id) => Ok(id),
        None => {
            conn.execute(
                "INSERT INTO units(field_id, name, sort_order) VALUES (?1, ?2, 1)",
                params![field_id, COMPAT_UNIT],
            )
            .map_err(err_str)?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn create_bank_node(
    state: &AppState,
    parent_id: Option<i64>,
    name: String,
) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("名前を入力してください".into());
    }
    if name.chars().count() > 200 {
        return Err("名前は200文字以内で入力してください".into());
    }
    if let Some(parent) = parent_id {
        if !bank_node_exists(&conn, parent).map_err(err_str)? {
            return Err("親階層が見つかりません".into());
        }
    }
    let next: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order),0)+1 FROM bank_nodes WHERE parent_id IS ?1",
            params![parent_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    conn.execute(
        "INSERT INTO bank_nodes(parent_id, name, sort_order) VALUES (?1, ?2, ?3)",
        params![parent_id, name, next],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_bank_node(state: &AppState, id: i64, name: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("名前を入力してください".into());
    }
    let changed = conn
        .execute(
            "UPDATE bank_nodes SET name=?1 WHERE id=?2",
            params![name, id],
        )
        .map_err(err_str)?;
    if changed == 0 {
        return Err("階層が見つかりません".into());
    }
    // 旧Parts画面でも名称だけは一致させる。親関係は任意階層の正本へ影響させない。
    let legacy: Option<(String, i64)> = conn
        .query_row(
            "SELECT legacy_kind, legacy_id FROM bank_nodes
             WHERE id=?1 AND legacy_kind IS NOT NULL AND legacy_id IS NOT NULL",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(err_str)?;
    if let Some((kind, legacy_id)) = legacy {
        let table = match kind.as_str() {
            "subject" => Some("subjects"),
            "field" => Some("fields"),
            "unit" => Some("units"),
            _ => None,
        };
        if let Some(table) = table {
            conn.execute(
                &format!("UPDATE {} SET name=?1 WHERE id=?2", table),
                params![name, legacy_id],
            )
            .map_err(err_str)?;
        }
    }
    Ok(())
}

fn normalize_bank_order(conn: &Connection, parent_id: Option<i64>) -> rusqlite::Result<()> {
    let siblings: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM bank_nodes WHERE parent_id IS ?1 ORDER BY sort_order, id")?;
        let rows = stmt
            .query_map(params![parent_id], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        rows
    };
    for (index, sibling) in siblings.into_iter().enumerate() {
        conn.execute(
            "UPDATE bank_nodes SET sort_order=?1 WHERE id=?2",
            params![index as i64 + 1, sibling],
        )?;
    }
    Ok(())
}

pub fn reorder_bank_node(state: &AppState, id: i64, delta: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let parent_id: Option<i64> = conn
        .query_row(
            "SELECT parent_id FROM bank_nodes WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    let siblings: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM bank_nodes WHERE parent_id IS ?1 ORDER BY sort_order, id")
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![parent_id], |row| row.get(0))
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    let position = siblings
        .iter()
        .position(|candidate| *candidate == id)
        .ok_or("階層が見つかりません")?;
    let target = position as i64 + delta.signum();
    if delta == 0 || target < 0 || target >= siblings.len() as i64 {
        return Ok(());
    }
    let mut ordered = siblings;
    ordered.swap(position, target as usize);
    for (index, sibling) in ordered.into_iter().enumerate() {
        conn.execute(
            "UPDATE bank_nodes SET sort_order=?1 WHERE id=?2",
            params![index as i64 + 1, sibling],
        )
        .map_err(err_str)?;
    }
    Ok(())
}

pub fn move_bank_node(
    state: &AppState,
    id: i64,
    new_parent_id: Option<i64>,
    sort_order: Option<i64>,
) -> Result<(), String> {
    if new_parent_id == Some(id) {
        return Err("階層を自分自身の中へ移動できません".into());
    }
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let old_parent: Option<i64> = tx
        .query_row(
            "SELECT parent_id FROM bank_nodes WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    if let Some(parent) = new_parent_id {
        if !bank_node_exists(&tx, parent).map_err(err_str)? {
            return Err("移動先が見つかりません".into());
        }
        let would_cycle: i64 = tx
            .query_row(
                "WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM bank_nodes WHERE parent_id=?1
                     UNION ALL
                     SELECT child.id FROM bank_nodes child JOIN descendants d ON child.parent_id=d.id
                 )
                 SELECT EXISTS(SELECT 1 FROM descendants WHERE id=?2)",
                params![id, parent],
                |row| row.get(0),
            )
            .map_err(err_str)?;
        if would_cycle != 0 {
            return Err("子階層の中へ移動すると循環するため実行できません".into());
        }
    }
    let target_order = match sort_order {
        Some(value) if value > 0 => value,
        _ => tx
            .query_row(
                "SELECT COALESCE(MAX(sort_order),0)+1 FROM bank_nodes WHERE parent_id IS ?1",
                params![new_parent_id],
                |row| row.get(0),
            )
            .map_err(err_str)?,
    };
    tx.execute(
        "UPDATE bank_nodes SET parent_id=?1, sort_order=?2 WHERE id=?3",
        params![new_parent_id, target_order, id],
    )
    .map_err(err_str)?;
    normalize_bank_order(&tx, old_parent).map_err(err_str)?;
    normalize_bank_order(&tx, new_parent_id).map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}

pub fn get_bank_node_delete_impact(
    state: &AppState,
    id: i64,
) -> Result<BankNodeDeleteImpact, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.query_row(
        "WITH RECURSIVE descendants(id) AS (
             SELECT id FROM bank_nodes WHERE parent_id=?1
             UNION ALL
             SELECT child.id FROM bank_nodes child JOIN descendants d ON child.parent_id=d.id
         )
         SELECT n.id,
                (SELECT COUNT(*) FROM descendants),
                (SELECT COUNT(*) FROM problems WHERE bank_node_id=n.id),
                (SELECT COUNT(*) FROM problems WHERE bank_node_id=n.id OR bank_node_id IN (SELECT id FROM descendants)),
                (SELECT COUNT(*) FROM parts WHERE bank_node_id=n.id),
                (SELECT COUNT(*) FROM parts WHERE bank_node_id=n.id OR bank_node_id IN (SELECT id FROM descendants)),
                n.parent_id
         FROM bank_nodes n WHERE n.id=?1",
        params![id],
        |row| {
            Ok(BankNodeDeleteImpact {
                node_id: row.get(0)?,
                child_node_count: row.get(1)?,
                direct_problem_count: row.get(2)?,
                descendant_problem_count: row.get(3)?,
                direct_part_count: row.get(4)?,
                descendant_part_count: row.get(5)?,
                parent_id: row.get(6)?,
            })
        },
    )
    .map_err(err_str)
}

/// strategy: "delete_all" または "move_to_parent"。キャンセルは呼び出し側で処理する。
pub fn delete_bank_node(state: &AppState, id: i64, strategy: String) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let parent_id: Option<i64> = tx
        .query_row(
            "SELECT parent_id FROM bank_nodes WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    match strategy.as_str() {
        "move_to_parent" => {
            let parent = parent_id.ok_or("ルート階層には移動先の親がありません")?;
            let legacy_unit = legacy_unit_for_bank_node(&tx, parent)?;
            tx.execute(
                "UPDATE problems SET bank_node_id=?1, unit_id=?2 WHERE bank_node_id=?3",
                params![parent, legacy_unit, id],
            )
            .map_err(err_str)?;
            tx.execute(
                "UPDATE parts SET bank_node_id=?1, unit_id=?2 WHERE bank_node_id=?3",
                params![parent, legacy_unit, id],
            )
            .map_err(err_str)?;
            let mut children: Vec<i64> = {
                let mut stmt = tx
                    .prepare("SELECT id FROM bank_nodes WHERE parent_id=?1 ORDER BY sort_order, id")
                    .map_err(err_str)?;
                let rows = stmt
                    .query_map(params![id], |row| row.get(0))
                    .map_err(err_str)?
                    .collect::<Result<_, _>>()
                    .map_err(err_str)?;
                rows
            };
            let next: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(sort_order),0)+1 FROM bank_nodes WHERE parent_id=?1",
                    params![parent],
                    |row| row.get(0),
                )
                .map_err(err_str)?;
            for (offset, child) in children.drain(..).enumerate() {
                tx.execute(
                    "UPDATE bank_nodes SET parent_id=?1, sort_order=?2 WHERE id=?3",
                    params![parent, next + offset as i64, child],
                )
                .map_err(err_str)?;
            }
            tx.execute("DELETE FROM bank_nodes WHERE id=?1", params![id])
                .map_err(err_str)?;
            normalize_bank_order(&tx, Some(parent)).map_err(err_str)?;
        }
        "delete_all" => {
            tx.execute(
                "WITH RECURSIVE subtree(id) AS (
                     SELECT id FROM bank_nodes WHERE id=?1
                     UNION ALL
                     SELECT child.id FROM bank_nodes child JOIN subtree s ON child.parent_id=s.id
                 )
                 UPDATE parts SET bank_node_id=NULL, unit_id=NULL
                 WHERE bank_node_id IN (SELECT id FROM subtree)",
                params![id],
            )
            .map_err(err_str)?;
            tx.execute(
                "WITH RECURSIVE subtree(id) AS (
                     SELECT id FROM bank_nodes WHERE id=?1
                     UNION ALL
                     SELECT child.id FROM bank_nodes child JOIN subtree s ON child.parent_id=s.id
                 )
                 DELETE FROM problems WHERE bank_node_id IN (SELECT id FROM subtree)",
                params![id],
            )
            .map_err(err_str)?;
            tx.execute("DELETE FROM bank_nodes WHERE id=?1", params![id])
                .map_err(err_str)?;
            normalize_bank_order(&tx, parent_id).map_err(err_str)?;
        }
        _ => return Err("削除方法が不正です".into()),
    }
    tx.commit().map_err(err_str)?;
    Ok(())
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
                .query_row(
                    &format!("SELECT COALESCE(MAX(sort_order),0)+1 FROM {}", table),
                    [],
                    |r| r.get(0),
                )
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
                    &format!(
                        "SELECT COALESCE(MAX(sort_order),0)+1 FROM {} WHERE {}=?1",
                        table, pc
                    ),
                    params![pid],
                    |r| r.get(0),
                )
                .map_err(err_str)?;
            conn.execute(
                &format!(
                    "INSERT INTO {} ({}, name, sort_order) VALUES (?1, ?2, ?3)",
                    table, pc
                ),
                params![pid, name, next],
            )
            .map_err(err_str)?;
            conn.last_insert_rowid()
        }
    };
    Ok(id)
}

pub fn rename_tree_node(
    state: &AppState,
    kind: String,
    id: i64,
    name: String,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("名前を入力してください".into());
    }
    let (table, _) = table_for(&kind)?;
    conn.execute(
        &format!("UPDATE {} SET name=?1 WHERE id=?2", table),
        params![name, id],
    )
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
                .query_row(
                    &format!("SELECT {} FROM {} WHERE id=?1", pc, table),
                    params![id],
                    |r| r.get(0),
                )
                .map_err(err_str)?;
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT id FROM {} WHERE {}=?1 ORDER BY sort_order, id",
                    table, pc
                ))
                .map_err(err_str)?;
            let rows = stmt
                .query_map(params![parent], |r| r.get(0))
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        }
    };
    let pos = siblings
        .iter()
        .position(|&x| x == id)
        .ok_or("対象が見つかりません")?;
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
