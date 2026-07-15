//! 問題バンクのインポート・エクスポートと整理（一括移動・削除）

use crate::db::now_str;
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
    pub answer_latex: String,
    pub explanation_latex: String,
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

#[derive(Serialize)]
pub struct ImportBankResult {
    pub subjects_created: i64,
    pub fields_created: i64,
    pub units_created: i64,
    pub problems_imported: i64,
}

fn problem_to_bank(conn: &Connection, attachments_dir: &Path, problem_id: i64) -> rusqlite::Result<BankProblem> {
    let mut p = conn.query_row(
        "SELECT title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo FROM problems WHERE id=?1",
        params![problem_id],
        |r| {
            Ok(BankProblem {
                title: r.get(0)?,
                statement_latex: r.get(1)?,
                answer_latex: r.get(2)?,
                explanation_latex: r.get(3)?,
                difficulty: r.get(4)?,
                difficulty_rank: r.get(5)?,
                is_required: r.get::<_, i64>(6)? != 0,
                memo: r.get(7)?,
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
        "subject" => (format!("{} WHERE s.id=?1{}", base_sql, order), Some(id.ok_or("IDが必要です")?)),
        "field" => (format!("{} WHERE f.id=?1{}", base_sql, order), Some(id.ok_or("IDが必要です")?)),
        "unit" => (format!("{} WHERE u.id=?1{}", base_sql, order), Some(id.ok_or("IDが必要です")?)),
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
                subjects.push(BankSubject { name: s_name.clone(), fields: vec![] });
                subjects.last_mut().unwrap()
            }
        };
        let field = match subject.fields.iter_mut().find(|f| f.name == f_name) {
            Some(f) => f,
            None => {
                subject.fields.push(BankField { name: f_name.clone(), units: vec![] });
                subject.fields.last_mut().unwrap()
            }
        };
        field.units.push(BankUnit { name: u_name, problems });
    }

    Ok(BankExport {
        kyozai_kobo_bank: 1,
        exported_at: now_str(),
        subjects,
    })
}

fn next_sort(conn: &Connection, table: &str, parent_col: Option<(&str, i64)>) -> rusqlite::Result<i64> {
    match parent_col {
        None => conn.query_row(
            &format!("SELECT COALESCE(MAX(sort_order),0)+1 FROM {}", table),
            [],
            |r| r.get(0),
        ),
        Some((col, id)) => conn.query_row(
            &format!("SELECT COALESCE(MAX(sort_order),0)+1 FROM {} WHERE {}=?1", table, col),
            params![id],
            |r| r.get(0),
        ),
    }
}

/// エクスポートデータを取り込む。同名の科目・分野・単元にはマージし、問題は常に新規作成する
pub fn apply_bank_import(
    conn: &Connection,
    attachments_dir: &Path,
    data: &BankExport,
) -> Result<ImportBankResult, String> {
    let mut result = ImportBankResult {
        subjects_created: 0,
        fields_created: 0,
        units_created: 0,
        problems_imported: 0,
    };
    let now = now_str();

    for s in &data.subjects {
        let subject_id: i64 = match conn
            .query_row("SELECT id FROM subjects WHERE name=?1 LIMIT 1", params![s.name], |r| r.get(0))
        {
            Ok(id) => id,
            Err(_) => {
                let order = next_sort(conn, "subjects", None).map_err(err_str)?;
                conn.execute("INSERT INTO subjects (name, sort_order) VALUES (?1, ?2)", params![s.name, order])
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
                    let order = next_sort(conn, "fields", Some(("subject_id", subject_id))).map_err(err_str)?;
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
                        let order = next_sort(conn, "units", Some(("field_id", field_id))).map_err(err_str)?;
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
                    let mut answer = p.answer_latex.clone();
                    let mut explanation = p.explanation_latex.clone();
                    let mut restored: Vec<(String, String)> = vec![]; // (file_name, new_stored)
                    for a in &p.attachments {
                        if a.data_base64.is_empty() {
                            continue;
                        }
                        let bytes = match base64::engine::general_purpose::STANDARD.decode(&a.data_base64) {
                            Ok(b) => b,
                            Err(_) => continue,
                        };
                        let ext = Path::new(&a.stored_name)
                            .extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_else(|| "png".into());
                        let new_stored = format!("img{}.{}", &uuid::Uuid::new_v4().simple().to_string()[..8], ext);
                        std::fs::create_dir_all(attachments_dir).ok();
                        std::fs::write(attachments_dir.join(&new_stored), bytes).map_err(err_str)?;
                        if !a.stored_name.is_empty() {
                            statement = statement.replace(&a.stored_name, &new_stored);
                            answer = answer.replace(&a.stored_name, &new_stored);
                            explanation = explanation.replace(&a.stored_name, &new_stored);
                        }
                        restored.push((a.file_name.clone(), new_stored));
                    }

                    let rank = super::problems::normalize_rank(p.difficulty_rank.clone());
                    conn.execute(
                        "INSERT INTO problems (unit_id, title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, memo, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                        params![unit_id, p.title, statement, answer, explanation, p.difficulty, rank, p.is_required as i64, p.memo, now],
                    )
                    .map_err(err_str)?;
                    let pid = conn.last_insert_rowid();
                    for t in &p.tags {
                        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![t])
                            .map_err(err_str)?;
                        let tag_id: i64 = conn
                            .query_row("SELECT id FROM tags WHERE name=?1", params![t], |r| r.get(0))
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
    Ok(result)
}

/// 問題バンクをJSONファイルへエクスポートする
/// scope_kind: "all" | "subject" | "field" | "unit" | "problems"
pub fn export_bank(
    state: &AppState,
    scope_kind: String,
    id: Option<i64>,
    problem_ids: Option<Vec<i64>>,
    dest_path: String,
) -> Result<String, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let data = build_bank_export(&conn, &state.attachments_dir(), &scope_kind, id, problem_ids)?;
    let json = serde_json::to_string_pretty(&data).map_err(err_str)?;
    std::fs::write(&dest_path, json).map_err(|e| format!("書き込みに失敗しました: {}", e))?;
    Ok(dest_path)
}

/// JSONファイルから問題バンクへインポートする
pub fn import_bank(state: &AppState, path: String) -> Result<ImportBankResult, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("読み込みに失敗しました: {}", e))?;
    let data: BankExport =
        serde_json::from_str(&text).map_err(|_| "教材工房の問題バンクファイルではありません")?;
    if data.kyozai_kobo_bank != 1 {
        return Err("対応していないバージョンのファイルです".into());
    }
    let mut conn = state.conn.lock().map_err(err_str)?;
    // 途中で失敗した場合に部分的な科目・問題が残らないよう全体を1トランザクションで行う。
    // 失敗時に書き込み済みの添付ファイルはDBから参照されない孤立ファイルとして残るだけで無害。
    let tx = conn.transaction().map_err(err_str)?;
    let result = apply_bank_import(&tx, &state.attachments_dir(), &data)?;
    tx.commit().map_err(err_str)?;
    Ok(result)
}

/// 複数の問題を別の単元へ移動する
pub fn move_problems(state: &AppState, problem_ids: Vec<i64>, unit_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    for pid in problem_ids {
        conn.execute("UPDATE problems SET unit_id=?1 WHERE id=?2", params![unit_id, pid])
            .map_err(err_str)?;
    }
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
