//! Web教材編集画面と共有グラフeditorを結ぶ期限付きserver session。
//! 挿入先はsession作成時にDBへ固定し、URL queryやクライアント申告だけを信用しない。

use super::graphs::{
    prepare_graph_snapshot, register_graph_snapshot, GraphAssetTarget, GraphSnapshotResult,
};
use crate::db::now_str;
use crate::state::{err_str, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;

const SESSION_TTL_SECONDS: i64 = 30 * 60;
const MAX_CURSOR: usize = 2_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGraphWebSessionPayload {
    pub project_id: Option<i64>,
    pub problem_id: Option<i64>,
    pub item_id: Option<i64>,
    pub target_field: String,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphWebSession {
    pub session_id: String,
    pub status: String,
    pub project_id: Option<i64>,
    pub problem_id: Option<i64>,
    pub item_id: Option<i64>,
    pub target_field: String,
    pub selection_start: usize,
    pub selection_end: usize,
    pub expected_target_version: i64,
    pub graph_id: String,
    pub asset_id: String,
    pub inserted_latex: String,
    pub created_at: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteGraphWebSessionResult {
    pub session: GraphWebSession,
    #[serde(flatten)]
    pub snapshot: GraphSnapshotResult,
}

fn safe_session_id(value: &str) -> bool {
    value
        .strip_prefix("graphsession_")
        .is_some_and(|suffix| suffix.len() == 32 && suffix.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn validate_target_field(payload: &CreateGraphWebSessionPayload) -> Result<(), String> {
    let allowed = if payload.item_id.is_some() {
        [
            "project_item_statement",
            "project_item_answer",
            "project_item_explanation",
            "project_item_content",
        ]
        .as_slice()
    } else if payload.problem_id.is_some() {
        ["problem_statement", "problem_answer", "problem_explanation"].as_slice()
    } else if payload.project_id.is_some() {
        ["project_text"].as_slice()
    } else {
        return Err("グラフ挿入先が指定されていません".into());
    };
    if !allowed.contains(&payload.target_field.as_str()) {
        return Err("グラフ挿入先フィールドが不正です".into());
    }
    Ok(())
}

fn target_version(
    conn: &Connection,
    project_id: Option<i64>,
    problem_id: Option<i64>,
    item_id: Option<i64>,
) -> Result<i64, String> {
    if let Some(item_id) = item_id {
        let row: Option<(i64, i64)> = conn
            .query_row(
                "SELECT i.project_id,p.version FROM project_items i JOIN projects p ON p.id=i.project_id WHERE i.id=?1",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(err_str)?;
        let (actual_project_id, version) = row.ok_or_else(|| "教材項目が見つかりません".to_string())?;
        if project_id.is_some_and(|id| id != actual_project_id) {
            return Err("教材項目と教材プロジェクトが一致しません".into());
        }
        return Ok(version);
    }
    if let Some(problem_id) = problem_id {
        return conn
            .query_row("SELECT version FROM problems WHERE id=?1", params![problem_id], |row| row.get(0))
            .optional()
            .map_err(err_str)?
            .ok_or_else(|| "問題が見つかりません".into());
    }
    if let Some(project_id) = project_id {
        return conn
            .query_row("SELECT version FROM projects WHERE id=?1", params![project_id], |row| row.get(0))
            .optional()
            .map_err(err_str)?
            .ok_or_else(|| "教材プロジェクトが見つかりません".into());
    }
    Err("グラフ挿入先が指定されていません".into())
}

fn load_session(conn: &Connection, session_id: &str) -> Result<GraphWebSession, String> {
    if !safe_session_id(session_id) {
        return Err("不正なグラフ連携sessionです".into());
    }
    conn.query_row(
        "SELECT id,status,project_id,problem_id,item_id,target_field,selection_start,selection_end,
                expected_target_version,graph_id,asset_id,inserted_latex,created_at,expires_at
         FROM graph_web_sessions WHERE id=?1",
        params![session_id],
        |row| {
            Ok(GraphWebSession {
                session_id: row.get(0)?,
                status: row.get(1)?,
                project_id: row.get(2)?,
                problem_id: row.get(3)?,
                item_id: row.get(4)?,
                target_field: row.get(5)?,
                selection_start: row.get::<_, i64>(6)?.max(0) as usize,
                selection_end: row.get::<_, i64>(7)?.max(0) as usize,
                expected_target_version: row.get(8)?,
                graph_id: row.get(9)?,
                asset_id: row.get(10)?,
                inserted_latex: row.get(11)?,
                created_at: row.get(12)?,
                expires_at: row.get(13)?,
            })
        },
    )
    .optional()
    .map_err(err_str)?
    .ok_or_else(|| "グラフ連携sessionが見つかりません".into())
}

pub fn create_graph_web_session(
    state: &AppState,
    payload: CreateGraphWebSessionPayload,
) -> Result<GraphWebSession, String> {
    validate_target_field(&payload)?;
    let selection_start = payload.selection_start.unwrap_or(0);
    let selection_end = payload.selection_end.unwrap_or(selection_start);
    if selection_start > selection_end || selection_end > MAX_CURSOR {
        return Err("グラフ挿入位置が不正です".into());
    }
    let now_epoch = chrono::Utc::now().timestamp();
    let expires_at = now_epoch + SESSION_TTL_SECONDS;
    let session_id = format!("graphsession_{}", uuid::Uuid::new_v4().simple());
    let conn = state.conn.lock().map_err(err_str)?;
    conn.execute(
        "UPDATE graph_web_sessions SET status='expired' WHERE status='pending' AND expires_at<?1",
        params![now_epoch],
    )
    .map_err(err_str)?;
    conn.execute(
        "DELETE FROM graph_web_sessions WHERE expires_at<?1",
        params![now_epoch - 86_400],
    )
    .map_err(err_str)?;
    let expected_target_version = target_version(
        &conn,
        payload.project_id,
        payload.problem_id,
        payload.item_id,
    )?;
    let created_at = now_str();
    conn.execute(
        "INSERT INTO graph_web_sessions
         (id,project_id,problem_id,item_id,target_field,selection_start,selection_end,
          expected_target_version,status,created_at,expires_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'pending',?9,?10)",
        params![
            session_id,
            payload.project_id,
            payload.problem_id,
            payload.item_id,
            payload.target_field,
            selection_start as i64,
            selection_end as i64,
            expected_target_version,
            created_at,
            expires_at,
        ],
    )
    .map_err(err_str)?;
    load_session(&conn, &session_id)
}

pub fn get_graph_web_session(state: &AppState, session_id: String) -> Result<GraphWebSession, String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let mut session = load_session(&conn, &session_id)?;
    if session.status == "pending" && session.expires_at < chrono::Utc::now().timestamp() {
        conn.execute(
            "UPDATE graph_web_sessions SET status='expired' WHERE id=?1 AND status='pending'",
            params![session_id],
        )
        .map_err(err_str)?;
        session.status = "expired".into();
    }
    Ok(session)
}

pub fn cancel_graph_web_session(state: &AppState, session_id: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(err_str)?;
    let session = load_session(&conn, &session_id)?;
    if session.status != "pending" {
        return Err("このグラフ連携sessionは既に終了しています".into());
    }
    conn.execute(
        "UPDATE graph_web_sessions SET status='cancelled' WHERE id=?1 AND status='pending'",
        params![session_id],
    )
    .map_err(err_str)?;
    Ok(())
}

pub fn complete_graph_web_session(
    state: &AppState,
    session_id: String,
    graph_id: String,
    expected_graph_version: i64,
) -> Result<CompleteGraphWebSessionResult, String> {
    let mut conn = state.conn.lock().map_err(err_str)?;
    let session = load_session(&conn, &session_id)?;
    if session.status != "pending" {
        return Err("このグラフ連携sessionは既に終了しています".into());
    }
    if session.expires_at < chrono::Utc::now().timestamp() {
        conn.execute("UPDATE graph_web_sessions SET status='expired' WHERE id=?1", params![session_id])
            .map_err(err_str)?;
        return Err("グラフ連携sessionの有効期限が切れました".into());
    }
    let current_target_version = target_version(&conn, session.project_id, session.problem_id, session.item_id)?;
    if current_target_version != session.expected_target_version {
        return Err(format!("CONFLICT:{current_target_version}"));
    }
    let current_graph_version: i64 = conn
        .query_row(
            "SELECT version FROM graphs WHERE id=?1 AND deleted_at=''",
            params![graph_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(err_str)?
        .ok_or_else(|| "グラフが見つかりません".to_string())?;
    if current_graph_version != expected_graph_version {
        return Err(format!("CONFLICT:{current_graph_version}"));
    }
    let prepared = prepare_graph_snapshot(state, &conn, &graph_id)?;
    let completion = (|| -> Result<GraphWebSession, String> {
        let tx = conn.transaction().map_err(err_str)?;
        let current = load_session(&tx, &session_id)?;
        if current.status != "pending" || current.expires_at < chrono::Utc::now().timestamp() {
            return Err("このグラフ連携sessionは終了または期限切れです".into());
        }
        let checked_target_version = target_version(&tx, current.project_id, current.problem_id, current.item_id)?;
        if checked_target_version != current.expected_target_version {
            return Err(format!("CONFLICT:{checked_target_version}"));
        }
        register_graph_snapshot(
            &tx,
            &prepared,
            GraphAssetTarget {
                project_id: current.project_id,
                problem_id: current.problem_id,
                item_id: current.item_id,
            },
        )?;
        let changed = tx.execute(
            "UPDATE graph_web_sessions SET status='completed',graph_id=?1,asset_id=?2,inserted_latex=?3
             WHERE id=?4 AND status='pending'",
            params![graph_id, prepared.result.asset_id, prepared.result.inserted_latex, session_id],
        ).map_err(err_str)?;
        if changed != 1 {
            return Err("グラフ連携sessionを完了できませんでした".into());
        }
        let result = load_session(&tx, &session_id)?;
        tx.commit().map_err(err_str)?;
        Ok(result)
    })();
    match completion {
        Ok(session) => Ok(CompleteGraphWebSessionResult {
            session,
            snapshot: prepared.result,
        }),
        Err(error) => {
            fs::remove_dir_all(&prepared.snapshot_dir).ok();
            Err(error)
        }
    }
}
