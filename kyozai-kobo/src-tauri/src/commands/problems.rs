use crate::db::now_str;
use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::{params, params_from_iter, Connection};
use rusqlite::types::Value;

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
    let conn = state.conn.lock().map_err(err_str)?;
    let rows: Vec<(i64, i64, String, String, Option<String>, bool, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, unit_id, title, difficulty, difficulty_rank, is_required, updated_at FROM problems WHERE unit_id=?1 ORDER BY id")
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![unit_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get::<_, i64>(5)? != 0, r.get(6)?))
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    let mut out = vec![];
    for (id, unit_id, title, difficulty, difficulty_rank, is_required, updated_at) in rows {
        out.push(ProblemSummary {
            id,
            unit_id,
            title,
            difficulty,
            difficulty_rank,
            is_required,
            tags: tags_of(&conn, id).map_err(err_str)?,
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
            "SELECT id, unit_id, title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo, created_at, updated_at, version FROM problems WHERE id=?1",
            params![id],
            |r| {
                Ok(ProblemFull {
                    id: r.get(0)?,
                    unit_id: r.get(1)?,
                    title: r.get(2)?,
                    statement_latex: r.get(3)?,
                    answer_latex: r.get(4)?,
                    explanation_latex: r.get(5)?,
                    difficulty: r.get(6)?,
                    difficulty_rank: r.get(7)?,
                    is_required: r.get::<_, i64>(8)? != 0,
                    memo: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                    tags: vec![],
                    attachments: vec![],
                    version: r.get(12)?,
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
    let now = now_str();
    let title = if title.trim().is_empty() { "新しい問題".to_string() } else { title.trim().to_string() };
    conn.execute(
        "INSERT INTO problems (unit_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![unit_id, title, now],
    )
    .map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

fn save_version(conn: &Connection, problem_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO problem_versions (problem_id, title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo, saved_at)
         SELECT id, title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo, ?2 FROM problems WHERE id=?1",
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
    conn.execute("DELETE FROM problem_tags WHERE problem_id=?1", params![problem_id])?;
    for t in tags {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![t])?;
        let tag_id: i64 = conn.query_row("SELECT id FROM tags WHERE name=?1", params![t], |r| r.get(0))?;
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
        .query_row("SELECT version FROM problems WHERE id=?1", params![payload.id], |r| r.get(0))
        .map_err(err_str)?;
    if let Some(expected) = payload.expected_version {
        if expected != current {
            return Err(format!("CONFLICT:{}", current));
        }
    }
    // 更新前の内容をバージョンとして保存
    save_version(&tx, payload.id).map_err(err_str)?;
    let difficulty_rank = normalize_rank(payload.difficulty_rank);
    tx.execute(
        "UPDATE problems SET unit_id=?1, title=?2, statement_latex=?3, answer_latex=?4, explanation_latex=?5, difficulty=?6, difficulty_rank=?7, is_required=?8, memo=?9, updated_at=?10, version=version+1 WHERE id=?11",
        params![
            payload.unit_id,
            payload.title,
            payload.statement_latex,
            payload.answer_latex,
            payload.explanation_latex,
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
        "INSERT INTO problems (unit_id, title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo, created_at, updated_at)
         SELECT unit_id, title || ' (コピー)', statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo, ?2, ?2 FROM problems WHERE id=?1",
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
    conn.execute("DELETE FROM problems WHERE id=?1", params![id]).map_err(err_str)?;
    Ok(())
}

pub fn list_versions(state: &AppState, problem_id: i64) -> Result<Vec<VersionSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare("SELECT id, title, saved_at FROM problem_versions WHERE problem_id=?1 ORDER BY id DESC")
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
        "SELECT id, problem_id, title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo, saved_at FROM problem_versions WHERE id=?1",
        params![version_id],
        |r| {
            Ok(VersionFull {
                id: r.get(0)?,
                problem_id: r.get(1)?,
                title: r.get(2)?,
                statement_latex: r.get(3)?,
                answer_latex: r.get(4)?,
                explanation_latex: r.get(5)?,
                difficulty: r.get(6)?,
                difficulty_rank: r.get(7)?,
                is_required: r.get::<_, i64>(8)? != 0,
                memo: r.get(9)?,
                saved_at: r.get(10)?,
            })
        },
    )
    .map_err(err_str)
}

pub fn restore_version(state: &AppState, version_id: i64) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let problem_id: i64 = tx
        .query_row("SELECT problem_id FROM problem_versions WHERE id=?1", params![version_id], |r| r.get(0))
        .map_err(err_str)?;
    // 現在の内容を履歴に残してから復元
    save_version(&tx, problem_id).map_err(err_str)?;
    tx.execute(
        "UPDATE problems SET
            title=(SELECT title FROM problem_versions WHERE id=?1),
            statement_latex=(SELECT statement_latex FROM problem_versions WHERE id=?1),
            answer_latex=(SELECT answer_latex FROM problem_versions WHERE id=?1),
            explanation_latex=(SELECT explanation_latex FROM problem_versions WHERE id=?1),
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
    let mut stmt = conn.prepare("SELECT name FROM tags ORDER BY name").map_err(err_str)?;
    let rows = stmt
        .query_map([], |r| r.get(0))
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

pub fn search_problems(state: &AppState, query: SearchQuery) -> Result<Vec<SearchResult>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut sql = String::from(
        "SELECT DISTINCT p.id, p.title, p.difficulty, p.difficulty_rank, p.is_required, p.updated_at, u.id, u.name, f.name, s.name
         FROM problems p
         JOIN units u ON u.id = p.unit_id
         JOIN fields f ON f.id = u.field_id
         JOIN subjects s ON s.id = f.subject_id
         WHERE 1=1",
    );
    let mut args: Vec<Value> = vec![];
    let text = query.text.trim().to_string();
    if !text.is_empty() {
        let like = format!("%{}%", text);
        args.push(Value::Text(like));
        let idx = args.len();
        sql.push_str(&format!(
            " AND (p.title LIKE ?{0} OR p.statement_latex LIKE ?{0} OR u.name LIKE ?{0}
               OR p.difficulty LIKE ?{0} OR p.difficulty_rank LIKE ?{0}
               OR EXISTS (SELECT 1 FROM problem_tags pt JOIN tags t ON t.id=pt.tag_id
                          WHERE pt.problem_id=p.id AND t.name LIKE ?{0}))",
            idx
        ));
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
            sql.push_str(
                &format!(
                    " AND EXISTS (SELECT 1 FROM problem_tags pt JOIN tags t ON t.id=pt.tag_id
                  WHERE pt.problem_id=p.id AND t.name = ?{})",
                    args.len()
                ),
            );
        }
    }
    sql.push_str(" ORDER BY p.updated_at DESC LIMIT 500");

    let rows: Vec<(i64, String, String, Option<String>, bool, String, i64, String, String, String)> = {
        let mut stmt = conn.prepare(&sql).map_err(err_str)?;
        let rows = stmt
            .query_map(params_from_iter(args.iter()), |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, i64>(4)? != 0,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    let mut out = vec![];
    for (id, title, difficulty, difficulty_rank, is_required, updated_at, unit_id, unit_name, field_name, subject_name) in rows {
        out.push(SearchResult {
            id,
            title,
            difficulty,
            difficulty_rank,
            is_required,
            tags: tags_of(&conn, id).map_err(err_str)?,
            updated_at,
            usage_count: usage_count(&conn, id).map_err(err_str)?,
            subject_name,
            field_name,
            unit_name,
            unit_id,
        });
    }
    Ok(out)
}
