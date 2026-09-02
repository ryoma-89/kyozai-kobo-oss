//! 問題バンクのインポート・エクスポートと整理（一括移動・削除）

use crate::db::now_str;
use crate::models::ProblemSolutionVariant;
use crate::state::{err_str, AppState};
use base64::Engine;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub struct BankAttachment {
    pub file_name: String,
    pub stored_name: String,
    #[serde(default)]
    pub data_base64: String,
}

#[derive(Serialize, Deserialize)]
pub struct BankProblem {
    pub title: String,
    pub statement_latex: String,
    #[serde(default)]
    pub statement_latex_two_column: String,
    pub answer_latex: String,
    pub explanation_latex: String,
    #[serde(default)]
    pub solution_variants: Vec<ProblemSolutionVariant>,
    #[serde(default)]
    pub answer_completed: bool,
    #[serde(default)]
    pub explanation_completed: bool,
    pub difficulty: String,
    #[serde(default)]
    pub difficulty_rank: Option<String>,
    #[serde(default)]
    pub is_required: bool,
    pub memo: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<BankAttachment>,
}

#[derive(Serialize, Deserialize)]
pub struct BankUnit {
    pub name: String,
    pub problems: Vec<BankProblem>,
}

#[derive(Serialize, Deserialize)]
pub struct BankField {
    pub name: String,
    pub units: Vec<BankUnit>,
}

#[derive(Serialize, Deserialize)]
pub struct BankSubject {
    pub name: String,
    pub fields: Vec<BankField>,
}

#[derive(Serialize, Deserialize)]
pub struct BankExport {
    pub kyozai_kobo_bank: u32,
    pub exported_at: String,
    pub subjects: Vec<BankSubject>,
}

#[derive(Serialize, Deserialize)]
pub struct BankNodeExport {
    pub name: String,
    #[serde(default)]
    pub problems: Vec<BankProblem>,
    #[serde(default)]
    pub children: Vec<BankNodeExport>,
}

#[derive(Serialize, Deserialize)]
pub struct BankExportV2 {
    pub format_version: u32,
    pub kyozai_kobo_bank: u32,
    pub exported_at: String,
    pub nodes: Vec<BankNodeExport>,
}

#[derive(Serialize)]
pub struct ImportBankResult {
    pub nodes_created: i64,
    pub subjects_created: i64,
    pub fields_created: i64,
    pub units_created: i64,
    pub problems_imported: i64,
}

fn problem_to_bank(
    conn: &Connection,
    attachments_dir: &Path,
    problem_id: i64,
) -> rusqlite::Result<BankProblem> {
    let mut p = conn.query_row(
        "SELECT title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo FROM problems WHERE id=?1",
        params![problem_id],
        |r| {
            Ok(BankProblem {
                title: r.get(0)?,
                statement_latex: r.get(1)?,
                statement_latex_two_column: r.get(2)?,
                answer_latex: r.get(3)?,
                explanation_latex: r.get(4)?,
                solution_variants: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                answer_completed: r.get::<_, i64>(6)? != 0,
                explanation_completed: r.get::<_, i64>(7)? != 0,
                difficulty: r.get(8)?,
                difficulty_rank: r.get(9)?,
                is_required: r.get::<_, i64>(10)? != 0,
                memo: r.get(11)?,
                tags: vec![],
                attachments: vec![],
            })
        },
    )?;
    p.tags = super::problems::tags_of(conn, problem_id)?;
    for a in super::problems::attachments_of(conn, problem_id)? {
        let data = std::fs::read(attachments_dir.join(&a.stored_name))
            .map(|b| base64::engine::general_purpose::STANDARD.encode(b))
            .unwrap_or_default();
        p.attachments.push(BankAttachment {
            file_name: a.file_name,
            stored_name: a.stored_name,
            data_base64: data,
        });
    }
    Ok(p)
}

fn build_bank_node_export(
    conn: &Connection,
    attachments_dir: &Path,
    node_id: i64,
    only_problem_ids: Option<&[i64]>,
    prune_empty: bool,
) -> Result<Option<BankNodeExport>, String> {
    let name: String = conn
        .query_row(
            "SELECT name FROM bank_nodes WHERE id=?1",
            params![node_id],
            |row| row.get(0),
        )
        .map_err(err_str)?;
    let problem_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM problems WHERE bank_node_id=?1 ORDER BY id")
            .map_err(err_str)?;
        let all: Vec<i64> = stmt
            .query_map(params![node_id], |row| row.get(0))
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        match only_problem_ids {
            Some(selected) => all
                .into_iter()
                .filter(|problem_id| selected.contains(problem_id))
                .collect(),
            None => all,
        }
    };
    let mut problems = Vec::with_capacity(problem_ids.len());
    for problem_id in problem_ids {
        problems.push(problem_to_bank(conn, attachments_dir, problem_id).map_err(err_str)?);
    }
    let child_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM bank_nodes WHERE parent_id=?1 ORDER BY sort_order, id")
            .map_err(err_str)?;
        let rows = stmt
            .query_map(params![node_id], |row| row.get(0))
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        rows
    };
    let mut children = vec![];
    for child_id in child_ids {
        if let Some(child) = build_bank_node_export(
            conn,
            attachments_dir,
            child_id,
            only_problem_ids,
            prune_empty,
        )? {
            children.push(child);
        }
    }
    if prune_empty && problems.is_empty() && children.is_empty() {
        return Ok(None);
    }
    Ok(Some(BankNodeExport {
        name,
        problems,
        children,
    }))
}

pub fn build_bank_export_v2(
    conn: &Connection,
    attachments_dir: &Path,
    scope_kind: &str,
    id: Option<i64>,
    problem_ids: Option<Vec<i64>>,
) -> Result<BankExportV2, String> {
    let prune_empty = scope_kind == "problems";
    let only_problem_ids = problem_ids.filter(|ids| !ids.is_empty());
    let root_ids: Vec<i64> = match scope_kind {
        "all" | "problems" => {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM bank_nodes WHERE parent_id IS NULL ORDER BY sort_order, id",
                )
                .map_err(err_str)?;
            let rows = stmt
                .query_map([], |row| row.get(0))
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            rows
        }
        "node" => vec![id.ok_or("IDが必要です")?],
        "subject" | "field" | "unit" => {
            let legacy_id = id.ok_or("IDが必要です")?;
            let node_id = conn
                .query_row(
                    "SELECT id FROM bank_nodes WHERE legacy_kind=?1 AND legacy_id=?2",
                    params![scope_kind, legacy_id],
                    |row| row.get(0),
                )
                .map_err(|_| "対応する問題バンク階層が見つかりません".to_string())?;
            vec![node_id]
        }
        _ => return Err(format!("不明な範囲: {}", scope_kind)),
    };
    let mut nodes = vec![];
    for node_id in root_ids {
        if let Some(node) = build_bank_node_export(
            conn,
            attachments_dir,
            node_id,
            only_problem_ids.as_deref(),
            prune_empty,
        )? {
            nodes.push(node);
        }
    }
    Ok(BankExportV2 {
        format_version: 2,
        kyozai_kobo_bank: 2,
        exported_at: now_str(),
        nodes,
    })
}

/// 対象範囲の (subject名, field名, unit名, unit_id, 問題ID絞り込み) を集めてエクスポート構造を作る
pub fn build_bank_export(
    conn: &Connection,
    attachments_dir: &Path,
    scope_kind: &str,
    id: Option<i64>,
    problem_ids: Option<Vec<i64>>,
) -> Result<BankExport, String> {
    // unit単位の行を集める
    let base_sql = "SELECT s.name, f.name, u.name, u.id
                    FROM units u JOIN fields f ON f.id=u.field_id JOIN subjects s ON s.id=f.subject_id";
    let order = " ORDER BY s.sort_order, s.id, f.sort_order, f.id, u.sort_order, u.id";
    let (sql, param): (String, Option<i64>) = match scope_kind {
        "all" => (format!("{}{}", base_sql, order), None),
        "subject" => (
            format!("{} WHERE s.id=?1{}", base_sql, order),
            Some(id.ok_or("IDが必要です")?),
        ),
        "field" => (
            format!("{} WHERE f.id=?1{}", base_sql, order),
            Some(id.ok_or("IDが必要です")?),
        ),
        "unit" => (
            format!("{} WHERE u.id=?1{}", base_sql, order),
            Some(id.ok_or("IDが必要です")?),
        ),
        "problems" => (format!("{}{}", base_sql, order), None),
        _ => return Err(format!("不明な範囲: {}", scope_kind)),
    };

    let rows: Vec<(String, String, String, i64)> = {
        let mut stmt = conn.prepare(&sql).map_err(err_str)?;
        let map = |r: &rusqlite::Row| -> rusqlite::Result<(String, String, String, i64)> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        };
        let collected = if let Some(p) = param {
            stmt.query_map(params![p], map)
                .map_err(err_str)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err_str)?
        } else {
            stmt.query_map([], map)
                .map_err(err_str)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err_str)?
        };
        collected
    };

    let only_ids = problem_ids.filter(|v| !v.is_empty());
    let mut subjects: Vec<BankSubject> = vec![];

    for (s_name, f_name, u_name, unit_id) in rows {
        // この単元の問題ID一覧（problemsスコープなら指定IDのみ）
        let pids: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT id FROM problems WHERE unit_id=?1 ORDER BY id")
                .map_err(err_str)?;
            let all: Vec<i64> = stmt
                .query_map(params![unit_id], |r| r.get(0))
                .map_err(err_str)?
                .collect::<Result<_, _>>()
                .map_err(err_str)?;
            match &only_ids {
                Some(ids) => all.into_iter().filter(|i| ids.contains(i)).collect(),
                None => all,
            }
        };
        if scope_kind == "problems" && pids.is_empty() {
            continue; // 選択問題が無い単元は含めない
        }
        let mut problems = vec![];
        for pid in pids {
            problems.push(problem_to_bank(conn, attachments_dir, pid).map_err(err_str)?);
        }

        let subject = match subjects.iter_mut().find(|s| s.name == s_name) {
            Some(s) => s,
            None => {
                subjects.push(BankSubject {
                    name: s_name.clone(),
                    fields: vec![],
                });
                subjects.last_mut().unwrap()
            }
        };
        let field = match subject.fields.iter_mut().find(|f| f.name == f_name) {
            Some(f) => f,
            None => {
                subject.fields.push(BankField {
                    name: f_name.clone(),
                    units: vec![],
                });
                subject.fields.last_mut().unwrap()
            }
        };
        field.units.push(BankUnit {
            name: u_name,
            problems,
        });
    }

    Ok(BankExport {
        kyozai_kobo_bank: 1,
        exported_at: now_str(),
        subjects,
    })
}

fn next_sort(
    conn: &Connection,
    table: &str,
    parent_col: Option<(&str, i64)>,
) -> rusqlite::Result<i64> {
    match parent_col {
        None => conn.query_row(
            &format!("SELECT COALESCE(MAX(sort_order),0)+1 FROM {}", table),
            [],
            |r| r.get(0),
        ),
        Some((col, id)) => conn.query_row(
            &format!(
                "SELECT COALESCE(MAX(sort_order),0)+1 FROM {} WHERE {}=?1",
                table, col
            ),
            params![id],
            |r| r.get(0),
        ),
    }
}

fn import_problem_into_bank_node(
    conn: &Connection,
    attachments_dir: &Path,
    bank_node_id: i64,
    problem: &BankProblem,
    now: &str,
) -> Result<i64, String> {
    let unit_id = super::tree::legacy_unit_for_bank_node(conn, bank_node_id)?;
    let mut statement = problem.statement_latex.clone();
    let mut statement_two_column = if problem.statement_latex_two_column.trim().is_empty() {
        problem.statement_latex.clone()
    } else {
        problem.statement_latex_two_column.clone()
    };
    let mut answer = problem.answer_latex.clone();
    let mut explanation = problem.explanation_latex.clone();
    let mut solution_variants = problem.solution_variants.clone();
    let mut restored: Vec<(String, String)> = vec![];
    for attachment in &problem.attachments {
        if attachment.data_base64.is_empty() {
            continue;
        }
        let bytes = match base64::engine::general_purpose::STANDARD.decode(&attachment.data_base64)
        {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let extension = Path::new(&attachment.stored_name)
            .extension()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "png".into());
        let new_stored = format!(
            "img{}.{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8],
            extension
        );
        std::fs::create_dir_all(attachments_dir).ok();
        std::fs::write(attachments_dir.join(&new_stored), bytes).map_err(err_str)?;
        if !attachment.stored_name.is_empty() {
            statement = statement.replace(&attachment.stored_name, &new_stored);
            statement_two_column =
                statement_two_column.replace(&attachment.stored_name, &new_stored);
            answer = answer.replace(&attachment.stored_name, &new_stored);
            explanation = explanation.replace(&attachment.stored_name, &new_stored);
            for variant in &mut solution_variants {
                variant.solution = variant
                    .solution
                    .replace(&attachment.stored_name, &new_stored);
                if let Some(value) = variant.explanation.as_mut() {
                    *value = value.replace(&attachment.stored_name, &new_stored);
                }
                for block in &mut variant.solution_blocks {
                    block.content = block.content.replace(&attachment.stored_name, &new_stored);
                }
                for section in &mut variant.explanation_sections {
                    section.content = section
                        .content
                        .replace(&attachment.stored_name, &new_stored);
                }
            }
        }
        restored.push((attachment.file_name.clone(), new_stored));
    }

    let rank = super::problems::normalize_rank(problem.difficulty_rank.clone());
    conn.execute(
        "INSERT INTO problems (unit_id, bank_node_id, title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
        params![
            unit_id,
            bank_node_id,
            problem.title,
            statement,
            statement_two_column,
            answer,
            explanation,
            serde_json::to_string(&solution_variants).map_err(err_str)?,
            problem.answer_completed as i64,
            problem.explanation_completed as i64,
            problem.difficulty,
            rank,
            problem.is_required as i64,
            problem.memo,
            now,
        ],
    )
    .map_err(err_str)?;
    let problem_id = conn.last_insert_rowid();
    for tag in &problem.tags {
        conn.execute(
            "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
            params![tag],
        )
        .map_err(err_str)?;
        let tag_id: i64 = conn
            .query_row("SELECT id FROM tags WHERE name=?1", params![tag], |row| {
                row.get(0)
            })
            .map_err(err_str)?;
        conn.execute(
            "INSERT OR IGNORE INTO problem_tags (problem_id, tag_id) VALUES (?1, ?2)",
            params![problem_id, tag_id],
        )
        .map_err(err_str)?;
    }
    for (file_name, stored_name) in restored {
        conn.execute(
            "INSERT INTO attachments (problem_id, file_name, stored_name, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![problem_id, file_name, stored_name, now],
        )
        .map_err(err_str)?;
    }
    Ok(problem_id)
}

fn import_node_v2(
    conn: &Connection,
    attachments_dir: &Path,
    parent_id: Option<i64>,
    node: &BankNodeExport,
    depth: usize,
    now: &str,
    result: &mut ImportBankResult,
) -> Result<(), String> {
    let name = node.name.trim();
    if name.is_empty() {
        return Err("階層名が空のデータは取り込めません".into());
    }
    let existing = conn
        .query_row(
            "SELECT id FROM bank_nodes WHERE parent_id IS ?1 AND name=?2 LIMIT 1",
            params![parent_id, name],
            |row| row.get(0),
        )
        .ok();
    let node_id = match existing {
        Some(id) => id,
        None => {
            let order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(sort_order),0)+1 FROM bank_nodes WHERE parent_id IS ?1",
                    params![parent_id],
                    |row| row.get(0),
                )
                .map_err(err_str)?;
            conn.execute(
                "INSERT INTO bank_nodes(parent_id, name, sort_order) VALUES (?1, ?2, ?3)",
                params![parent_id, name, order],
            )
            .map_err(err_str)?;
            result.nodes_created += 1;
            match depth {
                0 => result.subjects_created += 1,
                1 => result.fields_created += 1,
                2 => result.units_created += 1,
                _ => {}
            }
            conn.last_insert_rowid()
        }
    };
    for problem in &node.problems {
        import_problem_into_bank_node(conn, attachments_dir, node_id, problem, now)?;
        result.problems_imported += 1;
    }
    for child in &node.children {
        import_node_v2(
            conn,
            attachments_dir,
            Some(node_id),
            child,
            depth + 1,
            now,
            result,
        )?;
    }
    Ok(())
}

pub fn apply_bank_import_v2(
    conn: &Connection,
    attachments_dir: &Path,
    data: &BankExportV2,
) -> Result<ImportBankResult, String> {
    if data.format_version != 2 || data.kyozai_kobo_bank != 2 {
        return Err("対応していない問題バンク形式です".into());
    }
    let mut result = ImportBankResult {
        nodes_created: 0,
        subjects_created: 0,
        fields_created: 0,
        units_created: 0,
        problems_imported: 0,
    };
    let now = now_str();
    for node in &data.nodes {
        import_node_v2(conn, attachments_dir, None, node, 0, &now, &mut result)?;
    }
    Ok(result)
}

/// エクスポートデータを取り込む。同名の科目・分野・単元にはマージし、問題は常に新規作成する
pub fn apply_bank_import(
    conn: &Connection,
    attachments_dir: &Path,
    data: &BankExport,
) -> Result<ImportBankResult, String> {
    let mut result = ImportBankResult {
        nodes_created: 0,
        subjects_created: 0,
        fields_created: 0,
        units_created: 0,
        problems_imported: 0,
    };
    let now = now_str();

    for s in &data.subjects {
        let subject_id: i64 = match conn.query_row(
            "SELECT id FROM subjects WHERE name=?1 LIMIT 1",
            params![s.name],
            |r| r.get(0),
        ) {
            Ok(id) => id,
            Err(_) => {
                let order = next_sort(conn, "subjects", None).map_err(err_str)?;
                conn.execute(
                    "INSERT INTO subjects (name, sort_order) VALUES (?1, ?2)",
                    params![s.name, order],
                )
                .map_err(err_str)?;
                result.subjects_created += 1;
                conn.last_insert_rowid()
            }
        };
        for f in &s.fields {
            let field_id: i64 = match conn.query_row(
                "SELECT id FROM fields WHERE subject_id=?1 AND name=?2 LIMIT 1",
                params![subject_id, f.name],
                |r| r.get(0),
            ) {
                Ok(id) => id,
                Err(_) => {
                    let order = next_sort(conn, "fields", Some(("subject_id", subject_id)))
                        .map_err(err_str)?;
                    conn.execute(
                        "INSERT INTO fields (subject_id, name, sort_order) VALUES (?1, ?2, ?3)",
                        params![subject_id, f.name, order],
                    )
                    .map_err(err_str)?;
                    result.fields_created += 1;
                    conn.last_insert_rowid()
                }
            };
            for u in &f.units {
                let unit_id: i64 = match conn.query_row(
                    "SELECT id FROM units WHERE field_id=?1 AND name=?2 LIMIT 1",
                    params![field_id, u.name],
                    |r| r.get(0),
                ) {
                    Ok(id) => id,
                    Err(_) => {
                        let order = next_sort(conn, "units", Some(("field_id", field_id)))
                            .map_err(err_str)?;
                        conn.execute(
                            "INSERT INTO units (field_id, name, sort_order) VALUES (?1, ?2, ?3)",
                            params![field_id, u.name, order],
                        )
                        .map_err(err_str)?;
                        result.units_created += 1;
                        conn.last_insert_rowid()
                    }
                };
                for p in &u.problems {
                    // 添付を復元し、LaTeX中の旧ファイル名を新ファイル名へ置換
                    let mut statement = p.statement_latex.clone();
                    let mut statement_two_column = if p.statement_latex_two_column.trim().is_empty()
                    {
                        p.statement_latex.clone()
                    } else {
                        p.statement_latex_two_column.clone()
                    };
                    let mut answer = p.answer_latex.clone();
                    let mut explanation = p.explanation_latex.clone();
                    let mut solution_variants = p.solution_variants.clone();
                    let mut restored: Vec<(String, String)> = vec![]; // (file_name, new_stored)
                    for a in &p.attachments {
                        if a.data_base64.is_empty() {
                            continue;
                        }
                        let bytes = match base64::engine::general_purpose::STANDARD
                            .decode(&a.data_base64)
                        {
                            Ok(b) => b,
                            Err(_) => continue,
                        };
                        let ext = Path::new(&a.stored_name)
                            .extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_else(|| "png".into());
                        let new_stored = format!(
                            "img{}.{}",
                            &uuid::Uuid::new_v4().simple().to_string()[..8],
                            ext
                        );
                        std::fs::create_dir_all(attachments_dir).ok();
                        std::fs::write(attachments_dir.join(&new_stored), bytes)
                            .map_err(err_str)?;
                        if !a.stored_name.is_empty() {
                            statement = statement.replace(&a.stored_name, &new_stored);
                            statement_two_column =
                                statement_two_column.replace(&a.stored_name, &new_stored);
                            answer = answer.replace(&a.stored_name, &new_stored);
                            explanation = explanation.replace(&a.stored_name, &new_stored);
                            for variant in &mut solution_variants {
                                variant.solution =
                                    variant.solution.replace(&a.stored_name, &new_stored);
                                if let Some(value) = variant.explanation.as_mut() {
                                    *value = value.replace(&a.stored_name, &new_stored);
                                }
                                for block in &mut variant.solution_blocks {
                                    block.content =
                                        block.content.replace(&a.stored_name, &new_stored);
                                }
                                for section in &mut variant.explanation_sections {
                                    section.content =
                                        section.content.replace(&a.stored_name, &new_stored);
                                }
                            }
                        }
                        restored.push((a.file_name.clone(), new_stored));
                    }

                    let rank = super::problems::normalize_rank(p.difficulty_rank.clone());
                    conn.execute(
                        "INSERT INTO problems (unit_id, title, statement_latex, statement_latex_two_column, answer_latex, explanation_latex, solution_variants_json, answer_completed, explanation_completed, difficulty, difficulty_rank, is_required, memo, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
                        params![
                            unit_id,
                            p.title,
                            statement,
                            statement_two_column,
                            answer,
                            explanation,
                            serde_json::to_string(&solution_variants).map_err(err_str)?,
                            p.answer_completed as i64,
                            p.explanation_completed as i64,
                            p.difficulty,
                            rank,
                            p.is_required as i64,
                            p.memo,
                            now
                        ],
                    )
                    .map_err(err_str)?;
                    let pid = conn.last_insert_rowid();
                    for t in &p.tags {
                        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![t])
                            .map_err(err_str)?;
                        let tag_id: i64 = conn
                            .query_row("SELECT id FROM tags WHERE name=?1", params![t], |r| {
                                r.get(0)
                            })
                            .map_err(err_str)?;
                        conn.execute(
                            "INSERT OR IGNORE INTO problem_tags (problem_id, tag_id) VALUES (?1, ?2)",
                            params![pid, tag_id],
                        )
                        .map_err(err_str)?;
                    }
                    for (file_name, stored) in restored {
                        conn.execute(
                            "INSERT INTO attachments (problem_id, file_name, stored_name, created_at) VALUES (?1, ?2, ?3, ?4)",
                            params![pid, file_name, stored, now],
                        )
                        .map_err(err_str)?;
                    }
                    result.problems_imported += 1;
                }
            }
        }
    }
    result.nodes_created = result.subjects_created + result.fields_created + result.units_created;
    Ok(result)
}

/// 問題バンクをJSONファイルへエクスポートする
/// scope_kind: "all" | "node" | "subject" | "field" | "unit" | "problems"
pub fn export_bank(
    state: &AppState,
    scope_kind: String,
    id: Option<i64>,
    problem_ids: Option<Vec<i64>>,
    dest_path: String,
) -> Result<String, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let data = build_bank_export_v2(
        &conn,
        &state.attachments_dir(),
        &scope_kind,
        id,
        problem_ids,
    )?;
    let json = serde_json::to_string_pretty(&data).map_err(err_str)?;
    std::fs::write(&dest_path, json).map_err(|e| format!("書き込みに失敗しました: {}", e))?;
    Ok(dest_path)
}

/// JSONファイルから問題バンクへインポートする
pub fn import_bank(state: &AppState, path: String) -> Result<ImportBankResult, String> {
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("読み込みに失敗しました: {}", e))?;
    let mut conn = state.conn.lock().map_err(err_str)?;
    // 途中で失敗した場合に部分的な科目・問題が残らないよう全体を1トランザクションで行う。
    // 失敗時に書き込み済みの添付ファイルはDBから参照されない孤立ファイルとして残るだけで無害。
    let tx = conn.transaction().map_err(err_str)?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "教材工房の問題バンクファイルではありません")?;
    let format_version = value
        .get("format_version")
        .and_then(|version| version.as_u64())
        .or_else(|| {
            value
                .get("kyozai_kobo_bank")
                .and_then(|version| version.as_u64())
        })
        .unwrap_or(0);
    let result = if format_version == 2 {
        let data: BankExportV2 = serde_json::from_value(value)
            .map_err(|_| "教材工房の問題バンクv2ファイルではありません")?;
        apply_bank_import_v2(&tx, &state.attachments_dir(), &data)?
    } else if format_version == 1 {
        let data: BankExport = serde_json::from_value(value)
            .map_err(|_| "教材工房の問題バンクv1ファイルではありません")?;
        apply_bank_import(&tx, &state.attachments_dir(), &data)?
    } else {
        return Err("対応していないバージョンのファイルです".into());
    };
    tx.commit().map_err(err_str)?;
    Ok(result)
}

/// 複数の問題を別の単元へ移動する
pub fn move_problems(state: &AppState, problem_ids: Vec<i64>, unit_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    for pid in problem_ids {
        conn.execute(
            "UPDATE problems SET unit_id=?1 WHERE id=?2",
            params![unit_id, pid],
        )
        .map_err(err_str)?;
    }
    Ok(())
}

/// 複数の問題を任意のBankNodeへ移動する。bank_node_id を正本として更新し、
/// unit_id は旧AI・旧クライアント用の互換値へ揃える。
pub fn move_problems_to_bank_node(
    state: &AppState,
    problem_ids: Vec<i64>,
    bank_node_id: i64,
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let unit_id = super::tree::legacy_unit_for_bank_node(&conn, bank_node_id)?;
    let tx = conn.transaction().map_err(err_str)?;
    for problem_id in problem_ids {
        let changed = tx
            .execute(
                "UPDATE problems SET bank_node_id=?1, unit_id=?2 WHERE id=?3",
                params![bank_node_id, unit_id, problem_id],
            )
            .map_err(err_str)?;
        if changed == 0 {
            return Err(format!("問題ID {} が見つかりません", problem_id));
        }
    }
    tx.commit().map_err(err_str)?;
    Ok(())
}

/// 複数の問題を削除する
pub fn delete_problems(state: &AppState, problem_ids: Vec<i64>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    for pid in problem_ids {
        conn.execute("DELETE FROM problems WHERE id=?1", params![pid])
            .map_err(err_str)?;
    }
    Ok(())
}
