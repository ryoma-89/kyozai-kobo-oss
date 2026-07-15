use crate::db::now_str;
use crate::models::*;
use crate::state::{err_str, AppState};
use rusqlite::{params, Connection};

fn touch_project(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE projects SET updated_at=?1 WHERE id=?2",
        params![now_str(), project_id],
    )?;
    Ok(())
}

fn next_sort_order(conn: &Connection, project_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(sort_order),0)+1 FROM project_items WHERE project_id=?1",
        params![project_id],
        |r| r.get(0),
    )
}

pub fn list_projects(state: &AppState) -> Result<Vec<ProjectSummary>, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut stmt = conn
        .prepare(
            "SELECT p.id, p.name, p.description, p.updated_at,
                    (SELECT COUNT(*) FROM project_items i WHERE i.project_id=p.id), p.version
             FROM projects p ORDER BY p.updated_at DESC",
        )
        .map_err(err_str)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProjectSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                updated_at: r.get(3)?,
                item_count: r.get(4)?,
                version: r.get(5)?,
            })
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

/// テンプレート内容をプロジェクトへスナップショット保存する
fn snapshot_template(conn: &Connection, project_id: i64, template_id: i64) -> Result<(), String> {
    let assets_json: String = {
        let mut stmt = conn
            .prepare("SELECT file_name, stored_name FROM template_assets WHERE template_id=?1")
            .map_err(err_str)?;
        let assets: Vec<crate::models::SnapAttachment> = stmt
            .query_map(params![template_id], |r| {
                Ok(crate::models::SnapAttachment {
                    file_name: r.get(0)?,
                    stored_name: r.get(1)?,
                })
            })
            .map_err(err_str)?
            .collect::<Result<_, _>>()
            .map_err(err_str)?;
        serde_json::to_string(&assets).map_err(err_str)?
    };
    conn.execute(
        "UPDATE projects SET
            template_id=?1,
            snap_tpl_name=(SELECT name FROM templates WHERE id=?1),
            snap_tpl_base=(SELECT base_template FROM templates WHERE id=?1),
            snap_tpl_problem=(SELECT problem_template FROM templates WHERE id=?1),
            snap_tpl_answer=(SELECT answer_template FROM templates WHERE id=?1),
            snap_tpl_compile=(SELECT compile_method FROM templates WHERE id=?1),
            snap_tpl_assets=?2
         WHERE id=?3",
        params![template_id, assets_json, project_id],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn create_project(state: &AppState, name: String, template_id: Option<i64>) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    let name = if name.trim().is_empty() { "新しい教材".to_string() } else { name.trim().to_string() };
    conn.execute(
        "INSERT INTO projects (name, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![name, now],
    )
    .map_err(err_str)?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO project_settings (project_id, booklet_title) VALUES (?1, ?2)",
        params![id, name],
    )
    .map_err(err_str)?;
    // テンプレート（指定が無ければ先頭のもの）をスナップショット
    let tid = match template_id {
        Some(t) => Some(t),
        None => conn
            .query_row("SELECT id FROM templates ORDER BY id LIMIT 1", [], |r| r.get::<_, i64>(0))
            .ok(),
    };
    if let Some(tid) = tid {
        snapshot_template(&conn, id, tid)?;
    }
    Ok(id)
}

/// プロジェクトの使用テンプレートを変更する（その時点の内容をスナップショット）
pub fn set_project_template(state: &AppState, project_id: i64, template_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    snapshot_template(&conn, project_id, template_id)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(())
}

/// テンプレートの最新内容でスナップショットを更新する
pub fn refresh_project_template(state: &AppState, project_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let template_id: Option<i64> = conn
        .query_row("SELECT template_id FROM projects WHERE id=?1", params![project_id], |r| r.get(0))
        .map_err(err_str)?;
    let template_id = template_id.ok_or("元のテンプレートが削除されているため更新できません")?;
    snapshot_template(&conn, project_id, template_id)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(())
}

pub fn update_project_meta(
    state: &AppState,
    id: i64,
    name: String,
    description: String,
    expected_version: Option<i64>,
) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let changed = if let Some(expected) = expected_version {
        conn.execute(
            "UPDATE projects SET name=?1, description=?2, updated_at=?3, version=version+1 WHERE id=?4 AND version=?5",
            params![name.trim(), description, now_str(), id, expected],
        )
    } else {
        conn.execute(
            "UPDATE projects SET name=?1, description=?2, updated_at=?3, version=version+1 WHERE id=?4",
            params![name.trim(), description, now_str(), id],
        )
    }
    .map_err(err_str)?;
    let current: i64 = conn
        .query_row("SELECT version FROM projects WHERE id=?1", params![id], |r| r.get(0))
        .map_err(err_str)?;
    if changed == 0 {
        return Err(format!("CONFLICT:{}", current));
    }
    Ok(current)
}

pub fn delete_project(state: &AppState, id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute("DELETE FROM projects WHERE id=?1", params![id]).map_err(err_str)?;
    Ok(())
}

pub fn duplicate_project(state: &AppState, id: i64) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let now = now_str();
    conn.execute(
        "INSERT INTO projects (name, description, created_at, updated_at, template_id, snap_tpl_name, snap_tpl_base, snap_tpl_problem, snap_tpl_answer, snap_tpl_assets, snap_tpl_compile)
         SELECT name || ' (コピー)', description, ?2, ?2, template_id, snap_tpl_name, snap_tpl_base, snap_tpl_problem, snap_tpl_answer, snap_tpl_assets, snap_tpl_compile
         FROM projects WHERE id=?1",
        params![id, now],
    )
    .map_err(err_str)?;
    let new_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO project_settings (project_id, booklet_title, subtitle, target, date_str, header_left, header_right, number_format, show_name_field, auto_number, page_break_per_problem, include_explanation, two_column_mode, show_title, show_header, show_toc, number_headings, include_statement_in_answers, box_statement_in_answers, reset_numbering_per_chapter, difficulty_display, required_display)
         SELECT ?2, booklet_title, subtitle, target, date_str, header_left, header_right, number_format, show_name_field, auto_number, page_break_per_problem, include_explanation, two_column_mode, show_title, show_header, show_toc, number_headings, include_statement_in_answers, box_statement_in_answers, reset_numbering_per_chapter, difficulty_display, required_display
         FROM project_settings WHERE project_id=?1",
        params![id, new_id],
    )
    .map_err(err_str)?;
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, problem_id, part_id, snap_title, snap_statement, snap_answer, snap_explanation, snap_difficulty, snap_difficulty_rank, snap_is_required, snap_attachments, content, snap_part_type, snap_part_category, snap_part_description, snap_part_output_target, snap_part_attachments, heading_level, heading_numbered, created_at)
         SELECT ?2, item_type, sort_order, problem_id, part_id, snap_title, snap_statement, snap_answer, snap_explanation, snap_difficulty, snap_difficulty_rank, snap_is_required, snap_attachments, content, snap_part_type, snap_part_category, snap_part_description, snap_part_output_target, snap_part_attachments, heading_level, heading_numbered, ?3
         FROM project_items WHERE project_id=?1",
        params![id, new_id, now],
    )
    .map_err(err_str)?;
    Ok(new_id)
}

pub fn settings_of(conn: &Connection, project_id: i64) -> rusqlite::Result<ProjectSettings> {
    conn.query_row(
        "SELECT booklet_title, subtitle, target, date_str, header_left, header_right, number_format,
                show_name_field, auto_number, page_break_per_problem, include_explanation,
                two_column_mode, show_title, show_header, show_toc, number_headings, include_statement_in_answers,
                box_statement_in_answers, reset_numbering_per_chapter, difficulty_display, required_display
         FROM project_settings WHERE project_id=?1",
        params![project_id],
        |r| {
            Ok(ProjectSettings {
                booklet_title: r.get(0)?,
                subtitle: r.get(1)?,
                target: r.get(2)?,
                date_str: r.get(3)?,
                header_left: r.get(4)?,
                header_right: r.get(5)?,
                number_format: r.get(6)?,
                show_name_field: r.get::<_, i64>(7)? != 0,
                auto_number: r.get::<_, i64>(8)? != 0,
                page_break_per_problem: r.get::<_, i64>(9)? != 0,
                include_explanation: r.get::<_, i64>(10)? != 0,
                two_column_mode: r.get(11)?,
                show_title: r.get::<_, i64>(12)? != 0,
                show_header: r.get::<_, i64>(13)? != 0,
                show_toc: r.get::<_, i64>(14)? != 0,
                number_headings: r.get::<_, i64>(15)? != 0,
                include_statement_in_answers: r.get::<_, i64>(16)? != 0,
                box_statement_in_answers: r.get::<_, i64>(17)? != 0,
                reset_numbering_per_chapter: r.get::<_, i64>(18)? != 0,
                difficulty_display: r.get(19)?,
                required_display: r.get(20)?,
            })
        },
    )
}

pub fn items_of(conn: &Connection, project_id: i64) -> Result<Vec<ProjectItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT i.id, i.project_id, i.item_type, i.sort_order, i.problem_id, i.part_id,
                    i.snap_title, i.snap_statement, i.snap_answer, i.snap_explanation,
                    i.snap_difficulty, i.snap_difficulty_rank, i.snap_is_required, i.snap_attachments, i.content,
                    i.snap_part_type, i.snap_part_category, i.snap_part_description, i.snap_part_output_target,
                    i.snap_part_attachments, i.heading_level, i.heading_numbered,
                    p.id IS NOT NULL,
                    COALESCE(p.title, ''), COALESCE(p.statement_latex, ''), COALESCE(p.answer_latex, ''),
                    COALESCE(p.explanation_latex, ''), COALESCE(p.difficulty, ''), p.difficulty_rank, COALESCE(p.is_required, 0),
                    pr.id IS NOT NULL,
                    COALESCE(pr.title, ''), COALESCE(pr.latex_source, ''), COALESCE(pr.part_type, ''),
                    COALESCE(pr.category, ''), COALESCE(pr.description, ''), COALESCE(pr.output_target, 'both'),
                    i.version
             FROM project_items i
             LEFT JOIN problems p ON p.id = i.problem_id
             LEFT JOIN parts pr ON pr.id = i.part_id
             WHERE i.project_id=?1 ORDER BY i.sort_order, i.id",
        )
        .map_err(err_str)?;
    let rows = stmt
        .query_map(params![project_id], |r| {
            let item_type: String = r.get(2)?;
            let snap_attachments_json: String = r.get(13)?;
            let snap_part_attachments_json: String = r.get(19)?;
            let problem_exists: bool = r.get(22)?;
            let bank_title: String = r.get(23)?;
            let bank_statement: String = r.get(24)?;
            let bank_answer: String = r.get(25)?;
            let bank_explanation: String = r.get(26)?;
            let bank_difficulty: String = r.get(27)?;
            let bank_rank: Option<String> = r.get(28)?;
            let bank_required: bool = r.get::<_, i64>(29)? != 0;
            let part_exists: bool = r.get(30)?;
            let part_title: String = r.get(31)?;
            let part_latex: String = r.get(32)?;
            let part_type: String = r.get(33)?;
            let part_category: String = r.get(34)?;
            let part_description: String = r.get(35)?;
            let part_output_target: String = r.get(36)?;
            let snap_title: String = r.get(6)?;
            let snap_statement: String = r.get(7)?;
            let snap_answer: String = r.get(8)?;
            let snap_explanation: String = r.get(9)?;
            let snap_difficulty: String = r.get(10)?;
            let snap_difficulty_rank: Option<String> = r.get(11)?;
            let snap_is_required = r.get::<_, i64>(12)? != 0;
            let content: String = r.get(14)?;
            let snap_part_type: String = r.get(15)?;
            let snap_part_category: String = r.get(16)?;
            let snap_part_description: String = r.get(17)?;
            let snap_part_output_target: String = r.get(18)?;
            let bank_updated = item_type == "problem"
                && problem_exists
                && (bank_title != snap_title
                    || bank_statement != snap_statement
                    || bank_answer != snap_answer
                    || bank_explanation != snap_explanation
                    || bank_difficulty != snap_difficulty
                    || bank_rank != snap_difficulty_rank
                    || bank_required != snap_is_required);
            let part_updated = item_type == "part"
                && part_exists
                && (part_title != snap_title
                    || part_latex != content
                    || part_type != snap_part_type
                    || part_category != snap_part_category
                    || part_description != snap_part_description
                    || part_output_target != snap_part_output_target);
            let source_exists = if item_type == "problem" {
                problem_exists
            } else if item_type == "part" {
                part_exists
            } else {
                true
            };
            Ok(ProjectItem {
                id: r.get(0)?,
                project_id: r.get(1)?,
                item_type,
                sort_order: r.get(3)?,
                problem_id: r.get(4)?,
                part_id: r.get(5)?,
                snap_title,
                snap_statement,
                snap_answer,
                snap_explanation,
                snap_difficulty,
                snap_difficulty_rank,
                snap_is_required,
                snap_attachments: serde_json::from_str(&snap_attachments_json).unwrap_or_default(),
                content,
                snap_part_type,
                snap_part_category,
                snap_part_description,
                snap_part_output_target,
                snap_part_attachments: serde_json::from_str(&snap_part_attachments_json).unwrap_or_default(),
                heading_level: r.get(20)?,
                heading_numbered: r.get::<_, i64>(21)? != 0,
                bank_updated,
                source_exists,
                part_updated,
                version: r.get(37)?,
            })
        })
        .map_err(err_str)?
        .collect::<Result<_, _>>()
        .map_err(err_str)?;
    Ok(rows)
}

pub fn get_project(state: &AppState, id: i64) -> Result<ProjectFull, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let (name, description, created_at, updated_at, template_id, snap_name, snap_base, snap_problem, snap_answer, version): (
        String,
        String,
        String,
        String,
        Option<i64>,
        String,
        String,
        String,
        String,
        i64,
    ) = conn
        .query_row(
            "SELECT name, description, created_at, updated_at, template_id, snap_tpl_name, snap_tpl_base, snap_tpl_problem, snap_tpl_answer, version
             FROM projects WHERE id=?1",
            params![id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            },
        )
        .map_err(err_str)?;
    let settings = settings_of(&conn, id).map_err(err_str)?;
    let items = items_of(&conn, id)?;

    // テンプレート本体がスナップショット以降に更新されているか
    let mut template_updated = false;
    let mut template_name = snap_name.clone();
    if let Some(tid) = template_id {
        if let Ok((cur_name, cur_base, cur_problem, cur_answer)) = conn.query_row(
            "SELECT name, base_template, problem_template, answer_template FROM templates WHERE id=?1",
            params![tid],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        ) {
            template_name = cur_name;
            template_updated = cur_base != snap_base || cur_problem != snap_problem || cur_answer != snap_answer;
        }
    }

    Ok(ProjectFull {
        id,
        version,
        name,
        description,
        created_at,
        updated_at,
        settings,
        items,
        template_id,
        template_name,
        template_updated,
    })
}

/// 問題を教材に追加（現在の内容をスナップショットとして保存）
pub fn add_problem_to_project(state: &AppState, project_id: i64, problem_id: i64) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let attachments = super::problems::attachments_of(&conn, problem_id).map_err(err_str)?;
    let snap: Vec<SnapAttachment> = attachments
        .iter()
        .map(|a| SnapAttachment {
            file_name: a.file_name.clone(),
            stored_name: a.stored_name.clone(),
        })
        .collect();
    let snap_json = serde_json::to_string(&snap).map_err(err_str)?;
    let order = next_sort_order(&conn, project_id).map_err(err_str)?;
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, problem_id, snap_title, snap_statement, snap_answer, snap_explanation, snap_difficulty, snap_difficulty_rank, snap_is_required, snap_attachments, created_at)
         SELECT ?1, 'problem', ?2, id, title, statement_latex, answer_latex, explanation_latex, difficulty, difficulty_rank, is_required, ?3, ?4
         FROM problems WHERE id=?5",
        params![project_id, order, snap_json, now_str(), problem_id],
    )
    .map_err(err_str)?;
    if conn.changes() == 0 {
        return Err("問題が見つかりません".into());
    }
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

/// 部品を教材に追加（ライブラリの現在内容をスナップショットとして保存）
pub fn add_part_to_project(state: &AppState, project_id: i64, part_id: i64) -> Result<i64, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let attachments = super::parts::attachments_of(&conn, part_id).map_err(err_str)?;
    let snap: Vec<SnapAttachment> = attachments
        .iter()
        .map(|a| SnapAttachment {
            file_name: a.file_name.clone(),
            stored_name: a.stored_name.clone(),
        })
        .collect();
    let snap_json = serde_json::to_string(&snap).map_err(err_str)?;
    let order = next_sort_order(&conn, project_id).map_err(err_str)?;
    conn.execute(
        "INSERT INTO project_items (
            project_id, item_type, sort_order, part_id, snap_title, content,
            snap_difficulty_rank, snap_is_required, snap_part_type, snap_part_category,
            snap_part_description, snap_part_output_target, snap_part_attachments, created_at
         )
         SELECT ?1, 'part', ?2, id, title, latex_source, difficulty_rank, is_required, part_type,
                category, description, output_target, ?3, ?4
         FROM parts WHERE id=?5",
        params![project_id, order, snap_json, now_str(), part_id],
    )
    .map_err(err_str)?;
    if conn.changes() == 0 {
        return Err("部品が見つかりません".into());
    }
    conn.execute("UPDATE parts SET usage_count=usage_count+1 WHERE id=?1", params![part_id])
        .map_err(err_str)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

/// 見出し・説明文・改ページを追加（heading_level: 1=章, 2=節）
pub fn add_content_item(
    state: &AppState,
    project_id: i64,
    item_type: String,
    content: String,
    heading_level: Option<i64>,
) -> Result<i64, String> {
    if !["heading", "text", "pagebreak"].contains(&item_type.as_str()) {
        return Err(format!("不明な項目種別: {}", item_type));
    }
    let conn = state.conn.lock().map_err(err_str)?;
    let order = next_sort_order(&conn, project_id).map_err(err_str)?;
    conn.execute(
        "INSERT INTO project_items (project_id, item_type, sort_order, content, heading_level, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![project_id, item_type, order, content, heading_level.unwrap_or(1), now_str()],
    )
    .map_err(err_str)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(conn.last_insert_rowid())
}

/// 項目の内容を更新（見出し/説明文のテキスト、問題のスナップショット個別編集）。
/// 新しいversionを返す。競合時は "CONFLICT:<サーバー側version>"
pub fn update_project_item(state: &AppState, payload: ProjectItemUpdate) -> Result<i64, String> {
    let ProjectItemUpdate {
        item_id,
        content,
        snap_title,
        snap_statement,
        snap_answer,
        snap_explanation,
        snap_difficulty_rank,
        snap_is_required,
        snap_part_type,
        snap_part_category,
        snap_part_description,
        snap_part_output_target,
        heading_level,
        heading_numbered,
        expected_version,
    } = payload;
    let conn = state.conn.lock().map_err(err_str)?;
    let current: i64 = conn
        .query_row("SELECT version FROM project_items WHERE id=?1", params![item_id], |r| r.get(0))
        .map_err(err_str)?;
    if let Some(expected) = expected_version {
        if expected != current {
            return Err(format!("CONFLICT:{}", current));
        }
    }
    if let Some(c) = content {
        conn.execute("UPDATE project_items SET content=?1 WHERE id=?2", params![c, item_id])
            .map_err(err_str)?;
    }
    if let Some(lv) = heading_level {
        conn.execute("UPDATE project_items SET heading_level=?1 WHERE id=?2", params![lv, item_id])
            .map_err(err_str)?;
    }
    if let Some(hn) = heading_numbered {
        conn.execute(
            "UPDATE project_items SET heading_numbered=?1 WHERE id=?2",
            params![hn as i64, item_id],
        )
        .map_err(err_str)?;
    }
    if let Some(v) = snap_title {
        conn.execute("UPDATE project_items SET snap_title=?1 WHERE id=?2", params![v, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_statement {
        conn.execute("UPDATE project_items SET snap_statement=?1 WHERE id=?2", params![v, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_answer {
        conn.execute("UPDATE project_items SET snap_answer=?1 WHERE id=?2", params![v, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_explanation {
        conn.execute("UPDATE project_items SET snap_explanation=?1 WHERE id=?2", params![v, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_difficulty_rank {
        let rank = super::problems::normalize_rank(Some(v));
        conn.execute("UPDATE project_items SET snap_difficulty_rank=?1 WHERE id=?2", params![rank, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_is_required {
        conn.execute("UPDATE project_items SET snap_is_required=?1 WHERE id=?2", params![v as i64, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_part_type {
        conn.execute("UPDATE project_items SET snap_part_type=?1 WHERE id=?2", params![v, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_part_category {
        conn.execute("UPDATE project_items SET snap_part_category=?1 WHERE id=?2", params![v, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_part_description {
        conn.execute("UPDATE project_items SET snap_part_description=?1 WHERE id=?2", params![v, item_id])
            .map_err(err_str)?;
    }
    if let Some(v) = snap_part_output_target {
        conn.execute(
            "UPDATE project_items SET snap_part_output_target=?1 WHERE id=?2",
            params![super::parts::normalize_output_target(&v), item_id],
        )
        .map_err(err_str)?;
    }
    conn.execute("UPDATE project_items SET version=version+1 WHERE id=?1", params![item_id])
        .map_err(err_str)?;
    let project_id: i64 = conn
        .query_row("SELECT project_id FROM project_items WHERE id=?1", params![item_id], |r| r.get(0))
        .map_err(err_str)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(current + 1)
}

/// 問題バンクの最新内容でスナップショットを更新
pub fn refresh_item_from_bank(state: &AppState, item_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let problem_id: Option<i64> = conn
        .query_row("SELECT problem_id FROM project_items WHERE id=?1", params![item_id], |r| r.get(0))
        .map_err(err_str)?;
    let problem_id = problem_id.ok_or("元の問題が削除されているため更新できません")?;
    let attachments = super::problems::attachments_of(&conn, problem_id).map_err(err_str)?;
    let snap: Vec<SnapAttachment> = attachments
        .iter()
        .map(|a| SnapAttachment {
            file_name: a.file_name.clone(),
            stored_name: a.stored_name.clone(),
        })
        .collect();
    let snap_json = serde_json::to_string(&snap).map_err(err_str)?;
    conn.execute(
        "UPDATE project_items SET
            snap_title=(SELECT title FROM problems WHERE id=?1),
            snap_statement=(SELECT statement_latex FROM problems WHERE id=?1),
            snap_answer=(SELECT answer_latex FROM problems WHERE id=?1),
            snap_explanation=(SELECT explanation_latex FROM problems WHERE id=?1),
            snap_difficulty=(SELECT difficulty FROM problems WHERE id=?1),
            snap_difficulty_rank=(SELECT difficulty_rank FROM problems WHERE id=?1),
            snap_is_required=(SELECT is_required FROM problems WHERE id=?1),
            snap_attachments=?2,
            version=version+1
         WHERE id=?3",
        params![problem_id, snap_json, item_id],
    )
    .map_err(err_str)?;
    let project_id: i64 = conn
        .query_row("SELECT project_id FROM project_items WHERE id=?1", params![item_id], |r| r.get(0))
        .map_err(err_str)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(())
}

/// 部品ライブラリの最新内容で部品スナップショットを更新
pub fn refresh_part_item_from_library(state: &AppState, item_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let part_id: Option<i64> = conn
        .query_row("SELECT part_id FROM project_items WHERE id=?1", params![item_id], |r| r.get(0))
        .map_err(err_str)?;
    let part_id = part_id.ok_or("元の部品が削除されているため更新できません")?;
    let attachments = super::parts::attachments_of(&conn, part_id).map_err(err_str)?;
    let snap: Vec<SnapAttachment> = attachments
        .iter()
        .map(|a| SnapAttachment {
            file_name: a.file_name.clone(),
            stored_name: a.stored_name.clone(),
        })
        .collect();
    let snap_json = serde_json::to_string(&snap).map_err(err_str)?;
    conn.execute(
        "UPDATE project_items SET
            snap_title=(SELECT title FROM parts WHERE id=?1),
            content=(SELECT latex_source FROM parts WHERE id=?1),
            snap_difficulty_rank=(SELECT difficulty_rank FROM parts WHERE id=?1),
            snap_is_required=(SELECT is_required FROM parts WHERE id=?1),
            snap_part_type=(SELECT part_type FROM parts WHERE id=?1),
            snap_part_category=(SELECT category FROM parts WHERE id=?1),
            snap_part_description=(SELECT description FROM parts WHERE id=?1),
            snap_part_output_target=(SELECT output_target FROM parts WHERE id=?1),
            snap_part_attachments=?2,
            version=version+1
         WHERE id=?3",
        params![part_id, snap_json, item_id],
    )
    .map_err(err_str)?;
    let project_id: i64 = conn
        .query_row("SELECT project_id FROM project_items WHERE id=?1", params![item_id], |r| r.get(0))
        .map_err(err_str)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(())
}

pub fn remove_project_item(state: &AppState, item_id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let project_id: i64 = conn
        .query_row("SELECT project_id FROM project_items WHERE id=?1", params![item_id], |r| r.get(0))
        .map_err(err_str)?;
    conn.execute("DELETE FROM project_items WHERE id=?1", params![item_id])
        .map_err(err_str)?;
    touch_project(&conn, project_id).map_err(err_str)?;
    Ok(())
}

pub fn reorder_project_items(state: &AppState, project_id: i64, ordered_ids: Vec<i64>) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    for (i, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE project_items SET sort_order=?1 WHERE id=?2 AND project_id=?3",
            params![i as i64 + 1, id, project_id],
        )
        .map_err(err_str)?;
    }
    touch_project(&tx, project_id).map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(())
}

pub fn update_project_settings(
    state: &AppState,
    project_id: i64,
    settings: ProjectSettings,
    expected_version: Option<i64>,
) -> Result<i64, String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let tx = conn.transaction().map_err(err_str)?;
    let current: i64 = tx
        .query_row("SELECT version FROM projects WHERE id=?1", params![project_id], |r| r.get(0))
        .map_err(err_str)?;
    if expected_version.is_some_and(|expected| expected != current) {
        return Err(format!("CONFLICT:{}", current));
    }
    tx.execute(
        "UPDATE project_settings SET booklet_title=?1, subtitle=?2, target=?3, date_str=?4, header_left=?5, header_right=?6, number_format=?7,
                show_name_field=?8, auto_number=?9, page_break_per_problem=?10, include_explanation=?11,
                two_column_mode=?12, show_title=?13, show_header=?14, show_toc=?15, number_headings=?16,
                include_statement_in_answers=?17, box_statement_in_answers=?18, reset_numbering_per_chapter=?19,
                difficulty_display=?20, required_display=?21
         WHERE project_id=?22",
        params![
            settings.booklet_title,
            settings.subtitle,
            settings.target,
            settings.date_str,
            settings.header_left,
            settings.header_right,
            settings.number_format,
            settings.show_name_field as i64,
            settings.auto_number as i64,
            settings.page_break_per_problem as i64,
            settings.include_explanation as i64,
            settings.two_column_mode,
            settings.show_title as i64,
            settings.show_header as i64,
            settings.show_toc as i64,
            settings.number_headings as i64,
            settings.include_statement_in_answers as i64,
            settings.box_statement_in_answers as i64,
            settings.reset_numbering_per_chapter as i64,
            settings.difficulty_display,
            settings.required_display,
            project_id
        ],
    )
    .map_err(err_str)?;
    tx.execute(
        "UPDATE projects SET updated_at=?1, version=version+1 WHERE id=?2",
        params![now_str(), project_id],
    )
    .map_err(err_str)?;
    tx.commit().map_err(err_str)?;
    Ok(current + 1)
}
