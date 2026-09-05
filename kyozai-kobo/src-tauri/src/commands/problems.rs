use crate::db::now_str;
use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
use std::collections::HashSet;

fn parse_solution_variants(json: String) -> Vec<ProblemSolutionVariant> {
    serde_json::from_str(&json).unwrap_or_default()
}

fn parse_flow_blocks(json: String) -> Vec<SolutionFlowBlock> {
    serde_json::from_str(&json).unwrap_or_default()
}

fn normalize_solution_variants(
    mut variants: Vec<ProblemSolutionVariant>,
    common_pattern_ids: &HashSet<i64>,
) -> Result<Vec<ProblemSolutionVariant>, String> {
    if variants.len() > 6 {
        return Err("解法は最大6件まで保存できます".into());
    }
    let mut ids = HashSet::new();
    let mut main_seen = false;
    for (index, variant) in variants.iter_mut().enumerate() {
        variant.id = variant.id.trim().chars().take(100).collect();
        if variant.id.is_empty() || !ids.insert(variant.id.clone()) {
            variant.id = format!("variant-{}", index + 1);
            while !ids.insert(variant.id.clone()) {
                variant.id.push('x');
            }
        }
        variant.strategy.id = variant.strategy.id.trim().chars().take(100).collect();
        if variant.strategy.id.is_empty() {
            variant.strategy.id = format!("strategy-{}", index + 1);
        }
        variant.strategy.title = variant.strategy.title.trim().chars().take(120).collect();
        variant.strategy.summary = variant.strategy.summary.trim().chars().take(1000).collect();
        if variant.strategy.title.is_empty() || variant.strategy.summary.is_empty() {
            return Err("解法名と概要を入力してください".into());
        }
        if !matches!(
            variant.strategy.difficulty.as_deref(),
            None | Some("basic" | "standard" | "advanced")
        ) {
            variant.strategy.difficulty = None;
        }
        if !matches!(
            variant.strategy.answer_length.as_deref(),
            None | Some("short" | "medium" | "long")
        ) {
            variant.strategy.answer_length = None;
        }
        variant.strategy.concepts = variant
            .strategy
            .concepts
            .iter()
            .filter_map(|concept| {
                let value: String = concept.trim().chars().take(80).collect();
                (!value.is_empty()).then_some(value)
            })
            .take(12)
            .collect();
        if variant.solution.chars().count() > 200_000
            || variant.explanation.as_deref().unwrap_or("").chars().count() > 200_000
        {
            return Err("解答または解説が長すぎます".into());
        }
        if variant.role == "main" && !main_seen {
            main_seen = true;
        } else {
            variant.role = "alternative".into();
        }
        if let Some(plan) = variant.plan.as_mut() {
            plan.strategy_id = variant.strategy.id.clone();
            plan.outline.truncate(50);
        }
        if let Some(verification) = variant.verification.as_mut() {
            verification.issues.truncate(100);
            if !verification.issues.iter().all(|issue| {
                matches!(issue.severity.as_str(), "warning" | "error")
                    && !issue.message.trim().is_empty()
            }) {
                return Err("解答検証結果の形式が不正です".into());
            }
        }
        variant.solution_blocks.truncate(100);
        variant.explanation_sections.truncate(100);
        variant.flow_blocks = super::solution_flow::normalize_saved_flow_blocks(
            std::mem::take(&mut variant.flow_blocks),
            common_pattern_ids,
        )?;
    }
    if !variants.is_empty() && !main_seen {
        variants[0].role = "main".into();
    }
    Ok(variants)
}

pub fn tags_of(conn: &Connection, problem_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t JOIN problem_tags pt ON pt.tag_id=t.id WHERE pt.problem_id=?1 ORDER BY t.name",
    )?;
    let rows = stmt.query_map(params![problem_id], |r| r.get(0))?.collect();
    rows
}

pub fn attachments_of(conn: &Connection, problem_id: i64) -> rusqlite::Result<Vec<Attachment>> {
    let mut stmt = conn.prepare(
        "SELECT id, problem_id, file_name, stored_name, created_at FROM attachments WHERE problem_id=?1 ORDER BY id",
    )?;
    let rows = stmt
        .query_map(params![problem_id], |r| {
            Ok(Attachment {
                id: r.get(0)?,
                problem_id: r.get(1)?,
                file_name: r.get(2)?,
                stored_name: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect();
    rows
}

fn usage_count(conn: &Connection, problem_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM project_items WHERE problem_id=?1",
        params![problem_id],
        |r| r.get(0),
    )
}

pub fn normalize_rank(rank: Option<String>) -> Option<String> {
    rank.and_then(|r| {
        let r = r.trim().to_uppercase();
        if matches!(r.as_str(), "A" | "B" | "C" | "D") {
            Some(r)
        } else {
            None
        }
    })
}

pub fn list_problems(state: &AppState, unit_id: i64) -> Result<Vec<ProblemSummary>, String> {
    let bank_node_id = {
        let conn = state.conn.lock().map_err(err_str)?;
        super::tree::bank_node_for_legacy_unit(&conn, unit_id)
            .map_err(err_str)?
            .ok_or("対応する問題バンク階層が見つかりません")?
    };
    list_problems_in_bank_node(state, bank_node_id)
}

pub fn list_problems_in_bank_node(
    state: &AppState,
    bank_node_id: i64,
) -> Result<Vec<ProblemSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let rows: Vec<(
        i64,
        i64,
        i64,
        String,
        String,
        Option<String>,
        bool,
        bool,
        bool,
        String,
        String,
    )> = {
        let mut stmt = conn
            .prepare("SELECT id, bank_node_id, unit_id, title, difficulty, difficulty_rank, is_required, answer_completed, explanation_completed, created_at, updated_at FROM problems WHERE bank_node_id=?1 ORDER BY id")
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![bank_node_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get::<_, i64>(6)? != 0,
                    r.get::<_, i64>(7)? != 0,
                    r.get::<_, i64>(8)? != 0,
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
    for (
        id,
        bank_node_id,
        unit_id,
        title,
        difficulty,
        difficulty_rank,
        is_required,
        answer_completed,
        explanation_completed,
        created_at,
        updated_at,
    ) in rows
    {
        out.push(ProblemSummary {
            id,
            bank_node_id,
            unit_id,
            title,
            difficulty,
            difficulty_rank,
            is_required,
            answer_completed,
            explanation_completed,
            tags: tags_of(&conn, id).map_err(err_str)?,
            created_at,
            updated_at,
            usage_count: usage_count(&conn, id).map_err(err_str)?,
        });
    }
    Ok(out)
}

pub fn get_problem(state: &AppState, id: i64) -> Result<ProblemFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut p = conn
        .query_row(
            "SELECT id, bank_node_id, unit_id, title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, common_flow_blocks_json, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, created_at, updated_at, version FROM problems WHERE id=?1",
            params![id],
            |r| {
                Ok(ProblemFull {
                    id: r.get(0)?,
                    bank_node_id: r.get(1)?,
                    unit_id: r.get(2)?,
                    title: r.get(3)?,
                    statement_latex: r.get(4)?,
                    statement_latex_two_column: r.get(5)?,
                    answer_latex: r.get(6)?,
                    explanation_latex: r.get(7)?,
                    common_flow_blocks: parse_flow_blocks(r.get(8)?),
                    solution_variants: parse_solution_variants(r.get(9)?),
                    answer_completed: r.get::<_, i64>(10)? != 0,
                    explanation_completed: r.get::<_, i64>(11)? != 0,
                    difficulty: r.get(12)?,
                    difficulty_rank: r.get(13)?,
                    is_required: r.get::<_, i64>(14)? != 0,
                    memo: r.get(15)?,
                    created_at: r.get(16)?,
                    updated_at: r.get(17)?,
                    tags: vec![],
                    attachments: vec![],
                    version: r.get(18)?,
                })
            },
        )
        .map_err(err_str)?;
    p.tags = tags_of(&conn, id).map_err(err_str)?;
    p.attachments = attachments_of(&conn, id).map_err(err_str)?;
    Ok(p)
}

pub fn create_problem(state: &AppState, unit_id: i64, title: String) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let bank_node_id = super::tree::bank_node_for_legacy_unit(&conn, unit_id)
        .map_err(err_str)?
        .ok_or("対応する問題バンク階層が見つかりません")?;
    let now = now_str();
    let title = if title.trim().is_empty() {
        "新しい問題".to_string()
    } else {
        title.trim().to_string()
    };
    conn.execute(
        "INSERT INTO problems (unit_id, bank_node_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![unit_id, bank_node_id, title, now],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

pub fn create_problem_in_bank_node(
    state: &AppState,
    bank_node_id: i64,
    title: String,
) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let unit_id = super::tree::legacy_unit_for_bank_node(&conn, bank_node_id)?;
    let now = now_str();
    let title = if title.trim().is_empty() {
        "新しい問題".to_string()
    } else {
        title.trim().to_string()
    };
    conn.execute(
        "INSERT INTO problems (unit_id, bank_node_id, title, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![unit_id, bank_node_id, title, now],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

pub(crate) fn save_version(conn: &Connection, problem_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO problem_versions (problem_id, title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, common_flow_blocks_json, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, saved_at)
         SELECT id, title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, common_flow_blocks_json, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, ?2 FROM problems WHERE id=?1",
        params![problem_id, now_str()],
    )?;
    // 履歴は最大30件
    conn.execute(
        "DELETE FROM problem_versions WHERE problem_id=?1 AND id NOT IN (
            SELECT id FROM problem_versions WHERE problem_id=?1 ORDER BY id DESC LIMIT 30)",
        params![problem_id],
    )?;
    Ok(())
}

fn set_tags(conn: &Connection, problem_id: i64, tags: &[String]) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM problem_tags WHERE problem_id=?1",
        params![problem_id],
    )?;
    for t in tags {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![t])?;
        let tag_id: i64 = conn.query_row("SELECT id FROM tags WHERE name=?1", params![t], |r| {
            r.get(0)
        })?;
        conn.execute(
            "INSERT OR IGNORE INTO problem_tags (problem_id, tag_id) VALUES (?1, ?2)",
            params![problem_id, tag_id],
        )?;
    }
    Ok(())
}

/// 問題を更新して新しいversionを返す。expected_version 指定時は競合を検出し、
/// 競合時は "CONFLICT:<サーバー側version>" 形式のエラーを返す
pub fn update_problem(state: &AppState, payload: ProblemUpdate) -> Result<i64, String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let current: i64 = tx
        .query_row(
            "SELECT version FROM problems WHERE id=?1",
            params![payload.id],
            |r| r.get(0),
        )
        .map_err(err_str)?;
    if let Some(expected) = payload.expected_version {
        if expected != current {
            return Err(format!("CONFLICT:{}", current));
        }
    }
    // 更新前の内容をバージョンとして保存
    save_version(&tx, payload.id).map_err(err_str)?;
    let difficulty_rank = normalize_rank(payload.difficulty_rank);
    let common_flow_blocks = super::solution_flow::normalize_saved_flow_blocks(
        payload.common_flow_blocks,
        &HashSet::new(),
    )?;
    let common_pattern_ids: HashSet<i64> = common_flow_blocks
        .iter()
        .filter_map(|block| block.pattern_id)
        .collect();
    let solution_variants =
        normalize_solution_variants(payload.solution_variants, &common_pattern_ids)?;
    let has_structured_flow = !common_flow_blocks.is_empty()
        || solution_variants
            .iter()
            .any(|variant| !variant.flow_blocks.is_empty());
    let explanation_latex = if has_structured_flow {
        super::solution_flow::render_teaching_flow_latex(&common_flow_blocks, &solution_variants)
    } else {
        payload.explanation_latex
    };
    let common_flow_blocks_json = serde_json::to_string(&common_flow_blocks).map_err(err_str)?;
    let solution_variants_json = serde_json::to_string(&solution_variants).map_err(err_str)?;
    let (bank_node_id, unit_id) = if let Some(bank_node_id) = payload.bank_node_id {
        let unit_id = super::tree::legacy_unit_for_bank_node(&tx, bank_node_id)?;
        (bank_node_id, unit_id)
    } else {
        let bank_node_id = super::tree::bank_node_for_legacy_unit(&tx, payload.unit_id)
            .map_err(err_str)?
            .ok_or("対応する問題バンク階層が見つかりません")?;
        (bank_node_id, payload.unit_id)
    };
    tx.execute(
        "UPDATE problems SET unit_id=?1, bank_node_id=?2, title=?3, statement_latex=?4, statement_latex_two_column=?5, answer_latex=?6, explanation_latex=?7, common_flow_blocks_json=?8, solution_variants_json=?9, answer_completed=?10, explanation_completed=?11, difficulty=?12, difficulty_rank=?13, is_required=?14, memo=?15, updated_at=?16, version=version+1 WHERE id=?17",
        params![
            unit_id,
            bank_node_id,
            payload.title,
            payload.statement_latex,
            payload.statement_latex_two_column,
            payload.answer_latex,
            explanation_latex,
            common_flow_blocks_json,
            solution_variants_json,
            payload.answer_completed as i64,
            payload.explanation_completed as i64,
            payload.difficulty,
            difficulty_rank,
            payload.is_required as i64,
            payload.memo,
            now_str(),
            payload.id
        ],
    )
    .map_err(err_str)?;
    set_tags(&tx, payload.id, &payload.tags).map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(current + 1)
}

pub fn duplicate_problem(state: &AppState, id: i64) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    conn.execute(
        "INSERT INTO problems (unit_id, bank_node_id, title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, common_flow_blocks_json, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, created_at, updated_at)
         SELECT unit_id, bank_node_id, title || ' (コピー)', statement_latex, statement_latex_two_column, answer_latex, explanation_latex, common_flow_blocks_json, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, ?2, ?2 FROM problems WHERE id=?1",
        params![id, now],
    )
    .map_err(err_str)?;
    let new_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO problem_tags (problem_id, tag_id) SELECT ?2, tag_id FROM problem_tags WHERE problem_id=?1",
        params![id, new_id],
    )
    .map_err(err_str)?;
    // 添付もコピー（同じ実ファイルを参照）
    conn.execute(
        "INSERT INTO attachments (problem_id, file_name, stored_name, created_at)
         SELECT ?2, file_name, stored_name, ?3 FROM attachments WHERE problem_id=?1",
        params![id, new_id, now],
    )
    .map_err(err_str)?;
    Ok(new_id)
}

pub fn delete_problem(state: &AppState, id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute("DELETE FROM problems WHERE id=?1", params![id])
        .map_err(err_str)?;
    Ok(())
}

pub fn list_versions(state: &AppState, problem_id: i64) -> Result<Vec<VersionSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, saved_at FROM problem_versions WHERE problem_id=?1 ORDER BY id DESC",
        )
        .map_err(err_str)?;
    let rows = stmt
        .query_map(params![problem_id], |r| {
            Ok(VersionSummary {
                id: r.get(0)?,
                title: r.get(1)?,
                saved_at: r.get(2)?,
            })
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

pub fn get_version(state: &AppState, version_id: i64) -> Result<VersionFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.query_row(
        "SELECT id, problem_id, title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, common_flow_blocks_json, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, saved_at FROM problem_versions WHERE id=?1",
        params![version_id],
        |r| {
            Ok(VersionFull {
                id: r.get(0)?,
                problem_id: r.get(1)?,
                title: r.get(2)?,
                statement_latex: r.get(3)?,
                statement_latex_two_column: r.get(4)?,
                answer_latex: r.get(5)?,
                explanation_latex: r.get(6)?,
                common_flow_blocks: parse_flow_blocks(r.get(7)?),
                solution_variants: parse_solution_variants(r.get(8)?),
                answer_completed: r.get::<_, i64>(9)? != 0,
                explanation_completed: r.get::<_, i64>(10)? != 0,
                difficulty: r.get(11)?,
                difficulty_rank: r.get(12)?,
                is_required: r.get::<_, i64>(13)? != 0,
                memo: r.get(14)?,
                saved_at: r.get(15)?,
            })
        },
    )
    .map_err(err_str)
}

pub fn restore_version(state: &AppState, version_id: i64) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let problem_id: i64 = tx
        .query_row(
            "SELECT problem_id FROM problem_versions WHERE id=?1",
            params![version_id],
            |r| r.get(0),
        )
        .map_err(err_str)?;
    // 現在の内容を履歴に残してから復元
    save_version(&tx, problem_id).map_err(err_str)?;
    tx.execute(
        "UPDATE problems SET
            title=(SELECT title FROM problem_versions WHERE id=?1),
            statement_latex=(SELECT statement_latex FROM problem_versions WHERE id=?1),
            statement_latex_two_column=(SELECT statement_latex_two_column FROM problem_versions WHERE id=?1),
            answer_latex=(SELECT answer_latex FROM problem_versions WHERE id=?1),
            explanation_latex=(SELECT explanation_latex FROM problem_versions WHERE id=?1),
            common_flow_blocks_json=(SELECT common_flow_blocks_json FROM problem_versions WHERE id=?1),
            solution_variants_json=(SELECT solution_variants_json FROM problem_versions WHERE id=?1),
            answer_completed=(SELECT answer_completed FROM problem_versions WHERE id=?1),
            explanation_completed=(SELECT explanation_completed FROM problem_versions WHERE id=?1),
            difficulty=(SELECT difficulty FROM problem_versions WHERE id=?1),
            difficulty_rank=(SELECT difficulty_rank FROM problem_versions WHERE id=?1),
            is_required=(SELECT is_required FROM problem_versions WHERE id=?1),
            memo=(SELECT memo FROM problem_versions WHERE id=?1),
            updated_at=?2,
            version=version+1
         WHERE id=?3",
        params![version_id, now_str(), problem_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}

pub fn list_all_tags(state: &AppState) -> Result<Vec<String>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT name FROM tags ORDER BY name")
        .map_err(err_str)?;
    let rows = stmt
        .query_map([], |r| r.get(0))
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

pub fn bank_path(conn: &Connection, bank_node_id: i64) -> rusqlite::Result<String> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE ancestors(id, parent_id, name, depth) AS (
             SELECT id, parent_id, name, 0 FROM bank_nodes WHERE id=?1
             UNION ALL
             SELECT parent.id, parent.parent_id, parent.name, ancestors.depth+1
             FROM bank_nodes parent JOIN ancestors ON ancestors.parent_id=parent.id
         )
         SELECT name FROM ancestors ORDER BY depth DESC",
    )?;
    let names: Vec<String> = stmt
        .query_map(params![bank_node_id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(names.join(" / "))
}

pub fn search_problems(state: &AppState, query: SearchQuery) -> Result<Vec<SearchResult>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut sql = String::from(
        "SELECT DISTINCT p.id, p.title, p.difficulty, p.difficulty_rank, p.is_required, p.answer_completed, p.explanation_completed, p.updated_at,
                p.bank_node_id, p.unit_id, COALESCE(u.name,''), COALESCE(f.name,''), COALESCE(s.name,'')
         FROM problems p
         LEFT JOIN units u ON u.id = p.unit_id
         LEFT JOIN fields f ON f.id = u.field_id
         LEFT JOIN subjects s ON s.id = f.subject_id
         WHERE 1=1",
    );
    let mut args: Vec<Value> = vec![];
    let text = query.text.trim().to_string();
    if !text.is_empty() {
        let like = format!("%{}%", text);
        args.push(Value::Text(like));
        let idx = args.len();
        sql.push_str(&format!(
            " AND (p.title LIKE ?{0} OR p.statement_latex LIKE ?{0} OR p.statement_latex_two_column LIKE ?{0}
               OR EXISTS (WITH RECURSIVE ancestors(id, parent_id, name) AS (
                              SELECT id, parent_id, name FROM bank_nodes WHERE id=p.bank_node_id
                              UNION ALL
                              SELECT parent.id, parent.parent_id, parent.name
                              FROM bank_nodes parent JOIN ancestors ON ancestors.parent_id=parent.id
                          ) SELECT 1 FROM ancestors WHERE name LIKE ?{0})
               OR p.difficulty LIKE ?{0} OR p.difficulty_rank LIKE ?{0}
               OR EXISTS (SELECT 1 FROM problem_tags pt JOIN tags t ON t.id=pt.tag_id
                          WHERE pt.problem_id=p.id AND t.name LIKE ?{0}))",
            idx
        ));
    }
    if let Some(bank_node_id) = query.bank_node_id {
        args.push(Value::Integer(bank_node_id));
        let placeholder = args.len();
        if query.include_descendants.unwrap_or(true) {
            sql.push_str(&format!(
                " AND p.bank_node_id IN (
                     WITH RECURSIVE descendants(id) AS (
                         SELECT id FROM bank_nodes WHERE id=?{0}
                         UNION ALL
                         SELECT child.id FROM bank_nodes child JOIN descendants d ON child.parent_id=d.id
                     ) SELECT id FROM descendants
                 )",
                placeholder
            ));
        } else {
            sql.push_str(&format!(" AND p.bank_node_id=?{}", placeholder));
        }
    }
    if let Some(sid) = query.subject_id {
        args.push(Value::Integer(sid));
        sql.push_str(&format!(" AND s.id = ?{}", args.len()));
    }
    if let Some(fid) = query.field_id {
        args.push(Value::Integer(fid));
        sql.push_str(&format!(" AND f.id = ?{}", args.len()));
    }
    if let Some(uid) = query.unit_id {
        args.push(Value::Integer(uid));
        sql.push_str(&format!(" AND u.id = ?{}", args.len()));
    }
    if let Some(d) = &query.difficulty {
        if !d.is_empty() {
            args.push(Value::Text(d.clone()));
            sql.push_str(&format!(" AND p.difficulty = ?{}", args.len()));
        }
    }
    let requested_ranks = query.difficulty_ranks.clone().unwrap_or_default();
    let mut ranks: Vec<String> = requested_ranks
        .iter()
        .cloned()
        .into_iter()
        .filter_map(|r| normalize_rank(Some(r)))
        .collect();
    if ranks.is_empty() {
        if let Some(r) = normalize_rank(query.difficulty_rank) {
            ranks.push(r);
        }
    }
    let include_unset = requested_ranks.iter().any(|r| r == "__unset");
    if !ranks.is_empty() || include_unset {
        let mut clauses = vec![];
        if !ranks.is_empty() {
            let mut placeholders = vec![];
            for r in ranks {
                args.push(Value::Text(r));
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
    if let Some(tag) = &query.tag {
        if !tag.is_empty() {
            args.push(Value::Text(tag.clone()));
            sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM problem_tags pt JOIN tags t ON t.id=pt.tag_id
                  WHERE pt.problem_id=p.id AND t.name = ?{})",
                args.len()
            ));
        }
    }
    sql.push_str(" ORDER BY p.updated_at DESC LIMIT 500");

    let rows: Vec<(
        i64,
        String,
        String,
        Option<String>,
        bool,
        bool,
        bool,
        String,
        i64,
        i64,
        String,
        String,
        String,
    )> = {
        let mut stmt = conn.prepare(&sql).map_err(err_str)?;
        let rows = stmt
            .query_map(params_from_iter(args.iter()), |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, i64>(4)? != 0,
                    r.get::<_, i64>(5)? != 0,
                    r.get::<_, i64>(6)? != 0,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                ))
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    let mut out = vec![];
    for (
        id,
        title,
        difficulty,
        difficulty_rank,
        is_required,
        answer_completed,
        explanation_completed,
        updated_at,
        bank_node_id,
        unit_id,
        unit_name,
        field_name,
        subject_name,
    ) in rows
    {
        out.push(SearchResult {
            id,
            title,
            difficulty,
            difficulty_rank,
            is_required,
            answer_completed,
            explanation_completed,
            tags: tags_of(&conn, id).map_err(err_str)?,
            updated_at,
            usage_count: usage_count(&conn, id).map_err(err_str)?,
            bank_node_id,
            bank_path: bank_path(&conn, bank_node_id).map_err(err_str)?,
            subject_name,
            field_name,
            unit_name,
            unit_id,
        });
    }
    Ok(out)
}
