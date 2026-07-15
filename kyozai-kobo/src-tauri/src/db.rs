use rusqlite::Connection;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 4;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS subjects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS fields (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject_id INTEGER NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS units (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    field_id INTEGER NOT NULL REFERENCES fields(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS problems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    unit_id INTEGER NOT NULL REFERENCES units(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    statement_latex TEXT NOT NULL DEFAULT '',
    answer_latex TEXT NOT NULL DEFAULT '',
    explanation_latex TEXT NOT NULL DEFAULT '',
    difficulty TEXT NOT NULL DEFAULT '標準',
    difficulty_rank TEXT DEFAULT NULL,
    is_required INTEGER NOT NULL DEFAULT 0,
    memo TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS problem_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    problem_id INTEGER NOT NULL REFERENCES problems(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    statement_latex TEXT NOT NULL DEFAULT '',
    answer_latex TEXT NOT NULL DEFAULT '',
    explanation_latex TEXT NOT NULL DEFAULT '',
    difficulty TEXT NOT NULL DEFAULT '標準',
    difficulty_rank TEXT DEFAULT NULL,
    is_required INTEGER NOT NULL DEFAULT 0,
    memo TEXT NOT NULL DEFAULT '',
    saved_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS problem_tags (
    problem_id INTEGER NOT NULL REFERENCES problems(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (problem_id, tag_id)
);

CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS project_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    item_type TEXT NOT NULL DEFAULT 'problem',
    sort_order INTEGER NOT NULL DEFAULT 0,
    problem_id INTEGER REFERENCES problems(id) ON DELETE SET NULL,
    part_id INTEGER REFERENCES parts(id) ON DELETE SET NULL,
    snap_title TEXT NOT NULL DEFAULT '',
    snap_statement TEXT NOT NULL DEFAULT '',
    snap_answer TEXT NOT NULL DEFAULT '',
    snap_explanation TEXT NOT NULL DEFAULT '',
    snap_difficulty TEXT NOT NULL DEFAULT '標準',
    snap_difficulty_rank TEXT DEFAULT NULL,
    snap_is_required INTEGER NOT NULL DEFAULT 0,
    snap_attachments TEXT NOT NULL DEFAULT '[]',
    content TEXT NOT NULL DEFAULT '',
    snap_part_type TEXT NOT NULL DEFAULT '',
    snap_part_category TEXT NOT NULL DEFAULT '',
    snap_part_description TEXT NOT NULL DEFAULT '',
    snap_part_output_target TEXT NOT NULL DEFAULT 'both',
    snap_part_attachments TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_settings (
    project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    booklet_title TEXT NOT NULL DEFAULT '',
    target TEXT NOT NULL DEFAULT '',
    date_str TEXT NOT NULL DEFAULT '',
    show_name_field INTEGER NOT NULL DEFAULT 1,
    auto_number INTEGER NOT NULL DEFAULT 1,
    page_break_per_problem INTEGER NOT NULL DEFAULT 0,
    include_explanation INTEGER NOT NULL DEFAULT 1,
    box_statement_in_answers INTEGER NOT NULL DEFAULT 0,
    difficulty_display TEXT NOT NULL DEFAULT 'number_side',
    required_display TEXT NOT NULL DEFAULT 'required_only'
);

CREATE TABLE IF NOT EXISTS attachments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    problem_id INTEGER NOT NULL REFERENCES problems(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    stored_name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS graph_assets (
    asset_id TEXT PRIMARY KEY,
    graph_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    project_id INTEGER,
    problem_id INTEGER,
    item_id INTEGER,
    source_application TEXT NOT NULL DEFAULT 'MathGraph PDF Studio',
    editable_source_path TEXT NOT NULL,
    primary_asset_path TEXT NOT NULL,
    preview_asset_path TEXT NOT NULL DEFAULT '',
    latex_source_path TEXT NOT NULL DEFAULT '',
    inserted_latex TEXT NOT NULL DEFAULT '',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);

-- ブラウザ／Windowsの双方から編集するグラフの正本。
-- graph_json は MathGraph PDF Studio の既存 Project JSON をそのまま保持する。
CREATE TABLE IF NOT EXISTS graphs (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    graph_json TEXT NOT NULL,
    graph_type TEXT NOT NULL DEFAULT 'function_graph',
    source_type TEXT NOT NULL DEFAULT 'manual',
    warnings_json TEXT NOT NULL DEFAULT '[]',
    thumbnail_path TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT NOT NULL DEFAULT '',
    version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS graph_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    graph_id TEXT NOT NULL REFERENCES graphs(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    graph_json TEXT NOT NULL,
    graph_type TEXT NOT NULL DEFAULT 'function_graph',
    source_type TEXT NOT NULL DEFAULT 'manual',
    warnings_json TEXT NOT NULL DEFAULT '[]',
    version INTEGER NOT NULL,
    saved_at TEXT NOT NULL
);

-- Web教材編集画面とグラフ編集overlayを結ぶ期限付きserver session。
-- URLやブラウザstateだけで挿入先を決めない。
CREATE TABLE IF NOT EXISTS graph_web_sessions (
    id TEXT PRIMARY KEY,
    project_id INTEGER,
    problem_id INTEGER,
    item_id INTEGER,
    target_field TEXT NOT NULL,
    selection_start INTEGER NOT NULL DEFAULT 0,
    selection_end INTEGER NOT NULL DEFAULT 0,
    expected_target_version INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    graph_id TEXT NOT NULL DEFAULT '',
    asset_id TEXT NOT NULL DEFAULT '',
    inserted_latex TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    base_template TEXT NOT NULL DEFAULT '',
    problem_template TEXT NOT NULL DEFAULT '',
    answer_template TEXT NOT NULL DEFAULT '',
    compile_method TEXT NOT NULL DEFAULT 'uplatex+dvipdfmx',
    packages_memo TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS template_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id INTEGER NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    base_template TEXT NOT NULL DEFAULT '',
    problem_template TEXT NOT NULL DEFAULT '',
    answer_template TEXT NOT NULL DEFAULT '',
    compile_method TEXT NOT NULL DEFAULT 'uplatex+dvipdfmx',
    packages_memo TEXT NOT NULL DEFAULT '',
    saved_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS template_assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id INTEGER NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    stored_name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS parts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL DEFAULT '',
    part_type TEXT NOT NULL DEFAULT 'text',
    category TEXT NOT NULL DEFAULT '',
    latex_source TEXT NOT NULL DEFAULT '',
    plain_text_preview TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    difficulty_rank TEXT DEFAULT NULL,
    is_required INTEGER NOT NULL DEFAULT 0,
    output_target TEXT NOT NULL DEFAULT 'both',
    usage_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS part_tags (
    part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (part_id, tag)
);

CREATE TABLE IF NOT EXISTS part_attachments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    stored_name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS part_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    part_type TEXT NOT NULL DEFAULT 'text',
    category TEXT NOT NULL DEFAULT '',
    tags_json TEXT NOT NULL DEFAULT '[]',
    latex_source TEXT NOT NULL DEFAULT '',
    plain_text_preview TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    difficulty_rank TEXT DEFAULT NULL,
    is_required INTEGER NOT NULL DEFAULT 0,
    output_target TEXT NOT NULL DEFAULT 'both',
    version INTEGER NOT NULL DEFAULT 1,
    saved_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_fields_subject ON fields(subject_id);
CREATE INDEX IF NOT EXISTS idx_units_field ON units(field_id);
CREATE INDEX IF NOT EXISTS idx_problems_unit ON problems(unit_id);
CREATE INDEX IF NOT EXISTS idx_versions_problem ON problem_versions(problem_id);
CREATE INDEX IF NOT EXISTS idx_items_project ON project_items(project_id);
CREATE INDEX IF NOT EXISTS idx_attachments_problem ON attachments(problem_id);
CREATE INDEX IF NOT EXISTS idx_graph_assets_project ON graph_assets(project_id);
CREATE INDEX IF NOT EXISTS idx_graph_assets_problem ON graph_assets(problem_id);
CREATE INDEX IF NOT EXISTS idx_graphs_updated ON graphs(deleted_at, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_graph_versions_graph ON graph_versions(graph_id, version DESC);
CREATE INDEX IF NOT EXISTS idx_graph_web_sessions_expiry ON graph_web_sessions(status, expires_at);
CREATE INDEX IF NOT EXISTS idx_parts_type ON parts(part_type);
CREATE INDEX IF NOT EXISTS idx_parts_category ON parts(category);
CREATE INDEX IF NOT EXISTS idx_part_tags_part ON part_tags(part_id);
CREATE INDEX IF NOT EXISTS idx_part_attachments_part ON part_attachments(part_id);
"#;

/// 既存テーブルに列が無ければ追加する（マイグレーション用）
fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let exists = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|c| c.ok())
        .any(|c| c == column);
    if !exists {
        conn.execute(&format!("ALTER TABLE {} ADD COLUMN {}", table, ddl), [])?;
    }
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    // 問題の新難易度分類（既存 difficulty は保持）
    ensure_column(conn, "problems", "difficulty_rank", "difficulty_rank TEXT DEFAULT NULL")?;
    ensure_column(conn, "problems", "is_required", "is_required INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(conn, "problem_versions", "difficulty_rank", "difficulty_rank TEXT DEFAULT NULL")?;
    ensure_column(conn, "problem_versions", "is_required", "is_required INTEGER NOT NULL DEFAULT 0")?;
    // 教材プロジェクト: 使用テンプレートとそのスナップショット
    ensure_column(conn, "projects", "template_id", "template_id INTEGER REFERENCES templates(id) ON DELETE SET NULL")?;
    ensure_column(conn, "projects", "snap_tpl_name", "snap_tpl_name TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "projects", "snap_tpl_base", "snap_tpl_base TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "projects", "snap_tpl_problem", "snap_tpl_problem TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "projects", "snap_tpl_answer", "snap_tpl_answer TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "projects", "snap_tpl_assets", "snap_tpl_assets TEXT NOT NULL DEFAULT '[]'")?;
    ensure_column(conn, "projects", "snap_tpl_compile", "snap_tpl_compile TEXT NOT NULL DEFAULT 'uplatex+dvipdfmx'")?;
    // 出力設定の拡張
    ensure_column(conn, "project_settings", "subtitle", "subtitle TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "project_settings", "header_left", "header_left TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "project_settings", "header_right", "header_right TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "project_settings", "number_format", "number_format TEXT NOT NULL DEFAULT '問題{n}'")?;
    ensure_column(conn, "project_settings", "answers_two_column", "answers_two_column INTEGER NOT NULL DEFAULT 0")?;
    // 2段組の範囲: none / all（問題＋解答全体） / answer_only（解答部分のみ）
    ensure_column(conn, "project_settings", "two_column_mode", "two_column_mode TEXT NOT NULL DEFAULT 'none'")?;
    ensure_column(conn, "project_settings", "show_title", "show_title INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "project_settings", "show_header", "show_header INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "project_settings", "show_toc", "show_toc INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(conn, "project_settings", "number_headings", "number_headings INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(
        conn,
        "project_settings",
        "include_statement_in_answers",
        "include_statement_in_answers INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "box_statement_in_answers",
        "box_statement_in_answers INTEGER NOT NULL DEFAULT 0",
    )?;
    // 見出しのレベル: 1=章(section), 2=節(subsection)
    ensure_column(conn, "project_items", "heading_level", "heading_level INTEGER NOT NULL DEFAULT 1")?;
    // この見出しに番号を振るか（全体設定 number_headings がONのときのみ有効）
    ensure_column(conn, "project_items", "heading_numbered", "heading_numbered INTEGER NOT NULL DEFAULT 1")?;
    // 章ごとに問題番号をリセットするか（番号付き章では「2-1」形式）
    ensure_column(
        conn,
        "project_settings",
        "reset_numbering_per_chapter",
        "reset_numbering_per_chapter INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "difficulty_display",
        "difficulty_display TEXT NOT NULL DEFAULT 'number_side'",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "required_display",
        "required_display TEXT NOT NULL DEFAULT 'required_only'",
    )?;
    ensure_column(conn, "project_items", "snap_difficulty_rank", "snap_difficulty_rank TEXT DEFAULT NULL")?;
    ensure_column(conn, "project_items", "snap_is_required", "snap_is_required INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(conn, "project_items", "part_id", "part_id INTEGER REFERENCES parts(id) ON DELETE SET NULL")?;
    ensure_column(conn, "project_items", "snap_part_type", "snap_part_type TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "project_items", "snap_part_category", "snap_part_category TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "project_items", "snap_part_description", "snap_part_description TEXT NOT NULL DEFAULT ''")?;
    ensure_column(
        conn,
        "project_items",
        "snap_part_output_target",
        "snap_part_output_target TEXT NOT NULL DEFAULT 'both'",
    )?;
    ensure_column(conn, "project_items", "snap_part_attachments", "snap_part_attachments TEXT NOT NULL DEFAULT '[]'")?;
    // 旧 answers_two_column フラグを新形式へ移行（一度だけ実行される）
    conn.execute(
        "UPDATE project_settings SET two_column_mode='all', answers_two_column=0 WHERE answers_two_column=1",
        [],
    )?;
    // ---- Web版・同時編集対応: 楽観的ロック用のversion列 ----
    ensure_column(conn, "problems", "version", "version INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "project_items", "version", "version INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "projects", "version", "version INTEGER NOT NULL DEFAULT 1")?;
    ensure_column(conn, "templates", "version", "version INTEGER NOT NULL DEFAULT 1")?;
    // ---- Webサーバー・AI変換用テーブル ----
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS server_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS trusted_devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_name TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL DEFAULT '',
    revoked INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS web_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_hash TEXT NOT NULL UNIQUE,
    device_id INTEGER REFERENCES trusted_devices(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS ai_provider_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_conversion_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_uuid TEXT NOT NULL UNIQUE,
    source_type TEXT NOT NULL DEFAULT 'image',
    conversion_mode TEXT NOT NULL DEFAULT 'auto',
    options_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'queued',
    progress_message TEXT NOT NULL DEFAULT '',
    input_text TEXT NOT NULL DEFAULT '',
    input_asset_paths TEXT NOT NULL DEFAULT '[]',
    output_latex TEXT NOT NULL DEFAULT '',
    structured_result_json TEXT NOT NULL DEFAULT '',
    warnings_json TEXT NOT NULL DEFAULT '[]',
    uncertain_fragments_json TEXT NOT NULL DEFAULT '[]',
    compile_status TEXT NOT NULL DEFAULT 'none',
    compile_log TEXT NOT NULL DEFAULT '',
    preview_pdf_path TEXT NOT NULL DEFAULT '',
    target_entity_type TEXT NOT NULL DEFAULT '',
    target_entity_id INTEGER,
    target_field TEXT NOT NULL DEFAULT '',
    error_code TEXT NOT NULL DEFAULT '',
    error_message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS ai_conversion_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id INTEGER NOT NULL REFERENCES ai_conversion_jobs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_events_job ON ai_conversion_events(job_id);
CREATE INDEX IF NOT EXISTS idx_web_sessions_hash ON web_sessions(token_hash);
"#,
    )?;
    Ok(())
}

fn pre_migration_backup(
    conn: &Connection,
    data_dir: &Path,
    from_version: i64,
) -> rusqlite::Result<()> {
    let backup_dir = data_dir.join("backups");
    std::fs::create_dir_all(&backup_dir)
        .map_err(|_| rusqlite::Error::InvalidPath(backup_dir.clone()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dest_path = backup_dir.join(format!(
        "kyozai-kobo-pre-migration-v{}-{}.db",
        from_version, stamp
    ));
    let mut dest = Connection::open(&dest_path)?;
    {
        let backup = rusqlite::backup::Backup::new(conn, &mut dest)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(5), None)?;
    }
    let integrity: String = dest.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        std::fs::remove_file(&dest_path).ok();
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

pub fn open_db(data_dir: &Path) -> rusqlite::Result<Connection> {
    std::fs::create_dir_all(data_dir).ok();
    let db_path = data_dir.join("kyozai-kobo.db");
    let existed = db_path.exists();
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let current_version: i64 =
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if existed && current_version < SCHEMA_VERSION {
        pre_migration_backup(&conn, data_dir, current_version)?;
    }
    {
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA)?;
        migrate(&tx)?;
        tx.execute_batch(&format!("PRAGMA user_version={};", SCHEMA_VERSION))?;
        tx.commit()?;
    }
    Ok(conn)
}

pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// バックアップ: data_dir/backups/ に日付付きコピーを作成し、古いものを整理する
pub fn backup_db(data_dir: &PathBuf) {
    let db_path = data_dir.join("kyozai-kobo.db");
    if !db_path.exists() {
        return;
    }
    let backup_dir = data_dir.join("backups");
    if std::fs::create_dir_all(&backup_dir).is_err() {
        return;
    }
    let stamp = chrono::Local::now().format("%Y%m%d").to_string();
    let dest = backup_dir.join(format!("kyozai-kobo-{}.db", stamp));
    if !dest.exists() {
        let result: rusqlite::Result<()> = (|| {
            let source = Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let mut target = Connection::open(&dest)?;
            {
                let backup = rusqlite::backup::Backup::new(&source, &mut target)?;
                backup.run_to_completion(
                    64,
                    std::time::Duration::from_millis(5),
                    None,
                )?;
            }
            let integrity: String =
                target.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
            if integrity != "ok" {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(())
        })();
        if result.is_err() {
            std::fs::remove_file(&dest).ok();
            return;
        }
    }
    // 日次バックアップだけを10件までに整理する。
    // manual / pre-restore / pre-migration は別の保持規則なので削除しない。
    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                let Some(name) = p.file_name().map(|n| n.to_string_lossy()) else {
                    return false;
                };
                let Some(date) = name
                    .strip_prefix("kyozai-kobo-")
                    .and_then(|s| s.strip_suffix(".db"))
                else {
                    return false;
                };
                date.len() == 8 && date.chars().all(|c| c.is_ascii_digit())
            })
            .collect();
        files.sort();
        while files.len() > 10 {
            let old = files.remove(0);
            std::fs::remove_file(old).ok();
        }
    }
}
