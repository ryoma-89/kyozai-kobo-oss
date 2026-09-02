use rusqlite::Connection;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 15;

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

-- 問題バンクの正本。旧 subjects / fields / units は Parts と旧クライアントの
-- 互換レイヤーとして残し、legacy_* で移行元との対応を追跡する。
CREATE TABLE IF NOT EXISTS bank_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id INTEGER REFERENCES bank_nodes(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    legacy_kind TEXT,
    legacy_id INTEGER,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS problems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    unit_id INTEGER NOT NULL REFERENCES units(id) ON DELETE CASCADE,
    bank_node_id INTEGER REFERENCES bank_nodes(id) ON DELETE RESTRICT,
    title TEXT NOT NULL DEFAULT '',
    statement_latex TEXT NOT NULL DEFAULT '',
    statement_latex_two_column TEXT NOT NULL DEFAULT '',
    answer_latex TEXT NOT NULL DEFAULT '',
    explanation_latex TEXT NOT NULL DEFAULT '',
    solution_variants_json TEXT NOT NULL DEFAULT '[]',
    answer_completed INTEGER NOT NULL DEFAULT 0,
    explanation_completed INTEGER NOT NULL DEFAULT 0,
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
    statement_latex_two_column TEXT NOT NULL DEFAULT '',
    answer_latex TEXT NOT NULL DEFAULT '',
    explanation_latex TEXT NOT NULL DEFAULT '',
    solution_variants_json TEXT NOT NULL DEFAULT '[]',
    answer_completed INTEGER NOT NULL DEFAULT 0,
    explanation_completed INTEGER NOT NULL DEFAULT 0,
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
    snap_statement_two_column TEXT NOT NULL DEFAULT '',
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
    snap_part_layout_mode TEXT NOT NULL DEFAULT 'single_column',
    snap_part_attachments TEXT NOT NULL DEFAULT '[]',
    pattern_id INTEGER REFERENCES patterns(id) ON DELETE SET NULL,
    snap_pattern_json TEXT NOT NULL DEFAULT '',
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
    problem_two_column INTEGER NOT NULL DEFAULT 0,
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
    unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL,
    bank_node_id INTEGER REFERENCES bank_nodes(id) ON DELETE SET NULL,
    title TEXT NOT NULL DEFAULT '',
    part_type TEXT NOT NULL DEFAULT 'text',
    category TEXT NOT NULL DEFAULT '',
    latex_source TEXT NOT NULL DEFAULT '',
    plain_text_preview TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    difficulty_rank TEXT DEFAULT NULL,
    is_required INTEGER NOT NULL DEFAULT 0,
    output_target TEXT NOT NULL DEFAULT 'both',
    layout_mode TEXT NOT NULL DEFAULT 'single_column',
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
    unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL,
    bank_node_id INTEGER REFERENCES bank_nodes(id) ON DELETE SET NULL,
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
    layout_mode TEXT NOT NULL DEFAULT 'single_column',
    version INTEGER NOT NULL DEFAULT 1,
    saved_at TEXT NOT NULL
);

-- 数学的な判断・方針を管理する定石Knowledge Base。
-- 問題バンク、部品、教材プロジェクトとは独立した正本として保持する。
CREATE TABLE IF NOT EXISTS patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    pattern_type TEXT NOT NULL DEFAULT 'strategy',
    situation TEXT NOT NULL DEFAULT '',
    principle TEXT NOT NULL DEFAULT '',
    cautions TEXT NOT NULL DEFAULT '',
    examples TEXT NOT NULL DEFAULT '',
    source_note TEXT NOT NULL DEFAULT '',
    -- 作成経路（manual / problem_solution / problem_ai_inferred / image_import / ai_chat）。
    source_kind TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS pattern_strategies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_id INTEGER NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    parent_strategy_id INTEGER REFERENCES pattern_strategies(id) ON DELETE SET NULL,
    title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    condition_text TEXT NOT NULL DEFAULT '',
    reasoning TEXT NOT NULL DEFAULT '',
    branch_label TEXT NOT NULL DEFAULT '',
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS pattern_tags (
    pattern_id INTEGER NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (pattern_id, tag)
);

CREATE TABLE IF NOT EXISTS pattern_facets (
    pattern_id INTEGER NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    facet_type TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (pattern_id, facet_type, value)
);

CREATE TABLE IF NOT EXISTS problem_patterns (
    problem_id INTEGER NOT NULL REFERENCES problems(id) ON DELETE CASCADE,
    pattern_id INTEGER NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL DEFAULT 'applicable',
    created_at TEXT NOT NULL,
    PRIMARY KEY (problem_id, pattern_id)
);

CREATE TABLE IF NOT EXISTS pattern_relations (
    from_pattern_id INTEGER NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    to_pattern_id INTEGER NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL DEFAULT 'related',
    created_at TEXT NOT NULL,
    PRIMARY KEY (from_pattern_id, to_pattern_id, relation_type),
    CHECK (from_pattern_id <> to_pattern_id)
);

CREATE TABLE IF NOT EXISTS pattern_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_id INTEGER NOT NULL REFERENCES patterns(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL,
    saved_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_fields_subject ON fields(subject_id);
CREATE INDEX IF NOT EXISTS idx_units_field ON units(field_id);
CREATE INDEX IF NOT EXISTS idx_problems_unit ON problems(unit_id);
CREATE INDEX IF NOT EXISTS idx_bank_nodes_parent ON bank_nodes(parent_id, sort_order, id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_bank_nodes_legacy
    ON bank_nodes(legacy_kind, legacy_id)
    WHERE legacy_kind IS NOT NULL AND legacy_id IS NOT NULL;
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
CREATE INDEX IF NOT EXISTS idx_patterns_type_updated ON patterns(pattern_type, updated_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_pattern_strategies_pattern ON pattern_strategies(pattern_id, sort_order, id);
CREATE INDEX IF NOT EXISTS idx_pattern_tags_tag ON pattern_tags(tag, pattern_id);
CREATE INDEX IF NOT EXISTS idx_pattern_facets_filter ON pattern_facets(facet_type, value, pattern_id);
CREATE INDEX IF NOT EXISTS idx_problem_patterns_pattern ON problem_patterns(pattern_id, relation_type, problem_id);
CREATE INDEX IF NOT EXISTS idx_problem_patterns_problem ON problem_patterns(problem_id, relation_type, pattern_id);
CREATE INDEX IF NOT EXISTS idx_pattern_relations_to ON pattern_relations(to_pattern_id, relation_type, from_pattern_id);
CREATE INDEX IF NOT EXISTS idx_pattern_versions_pattern ON pattern_versions(pattern_id, version DESC, id DESC);

-- 旧API・サンプル投入・既存AIが3階層テーブルへ直接追加しても、新しい正本へ同期する。
CREATE TRIGGER IF NOT EXISTS sync_subject_to_bank_node_insert
AFTER INSERT ON subjects
WHEN NEW.name NOT LIKE '__kyozai_bank_%'
BEGIN
    INSERT OR IGNORE INTO bank_nodes(parent_id, name, sort_order, legacy_kind, legacy_id)
    VALUES (NULL, NEW.name, NEW.sort_order, 'subject', NEW.id);
END;
CREATE TRIGGER IF NOT EXISTS sync_subject_to_bank_node_update
AFTER UPDATE OF name, sort_order ON subjects
BEGIN
    UPDATE bank_nodes SET name=NEW.name, sort_order=NEW.sort_order
    WHERE legacy_kind='subject' AND legacy_id=NEW.id;
END;
CREATE TRIGGER IF NOT EXISTS sync_subject_to_bank_node_delete
AFTER DELETE ON subjects
BEGIN
    DELETE FROM bank_nodes WHERE legacy_kind='subject' AND legacy_id=OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS sync_field_to_bank_node_insert
AFTER INSERT ON fields
WHEN EXISTS(SELECT 1 FROM bank_nodes WHERE legacy_kind='subject' AND legacy_id=NEW.subject_id)
BEGIN
    INSERT OR IGNORE INTO bank_nodes(parent_id, name, sort_order, legacy_kind, legacy_id)
    SELECT id, NEW.name, NEW.sort_order, 'field', NEW.id
    FROM bank_nodes WHERE legacy_kind='subject' AND legacy_id=NEW.subject_id;
END;
CREATE TRIGGER IF NOT EXISTS sync_field_to_bank_node_update
AFTER UPDATE OF subject_id, name, sort_order ON fields
BEGIN
    UPDATE bank_nodes
    SET parent_id=(SELECT id FROM bank_nodes WHERE legacy_kind='subject' AND legacy_id=NEW.subject_id),
        name=NEW.name,
        sort_order=NEW.sort_order
    WHERE legacy_kind='field' AND legacy_id=NEW.id;
END;
CREATE TRIGGER IF NOT EXISTS sync_field_to_bank_node_delete
AFTER DELETE ON fields
BEGIN
    DELETE FROM bank_nodes WHERE legacy_kind='field' AND legacy_id=OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS sync_unit_to_bank_node_insert
AFTER INSERT ON units
WHEN EXISTS(SELECT 1 FROM bank_nodes WHERE legacy_kind='field' AND legacy_id=NEW.field_id)
BEGIN
    INSERT OR IGNORE INTO bank_nodes(parent_id, name, sort_order, legacy_kind, legacy_id)
    SELECT id, NEW.name, NEW.sort_order, 'unit', NEW.id
    FROM bank_nodes WHERE legacy_kind='field' AND legacy_id=NEW.field_id;
END;
CREATE TRIGGER IF NOT EXISTS sync_unit_to_bank_node_update
AFTER UPDATE OF field_id, name, sort_order ON units
BEGIN
    UPDATE bank_nodes
    SET parent_id=(SELECT id FROM bank_nodes WHERE legacy_kind='field' AND legacy_id=NEW.field_id),
        name=NEW.name,
        sort_order=NEW.sort_order
    WHERE legacy_kind='unit' AND legacy_id=NEW.id;
END;
CREATE TRIGGER IF NOT EXISTS sync_unit_to_bank_node_delete
AFTER DELETE ON units
BEGIN
    DELETE FROM bank_nodes WHERE legacy_kind='unit' AND legacy_id=OLD.id;
END;

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
    // ---- v12: 問題バンクを任意深度の自己参照ツリーへ一般化 ----
    // 旧3階層は削除せず、legacy_kind / legacy_id を介して追跡可能な形で複製する。
    conn.execute(
        "INSERT OR IGNORE INTO bank_nodes(parent_id, name, sort_order, legacy_kind, legacy_id)
         SELECT NULL, name, sort_order, 'subject', id
         FROM subjects WHERE name NOT LIKE '__kyozai_bank_%'",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO bank_nodes(parent_id, name, sort_order, legacy_kind, legacy_id)
         SELECT parent.id, f.name, f.sort_order, 'field', f.id
         FROM fields f
         JOIN bank_nodes parent ON parent.legacy_kind='subject' AND parent.legacy_id=f.subject_id",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO bank_nodes(parent_id, name, sort_order, legacy_kind, legacy_id)
         SELECT parent.id, u.name, u.sort_order, 'unit', u.id
         FROM units u
         JOIN bank_nodes parent ON parent.legacy_kind='field' AND parent.legacy_id=u.field_id",
        [],
    )?;
    ensure_column(
        conn,
        "problems",
        "bank_node_id",
        "bank_node_id INTEGER REFERENCES bank_nodes(id) ON DELETE RESTRICT",
    )?;
    conn.execute(
        "UPDATE problems
         SET bank_node_id=(SELECT id FROM bank_nodes
                           WHERE legacy_kind='unit' AND legacy_id=problems.unit_id)
         WHERE bank_node_id IS NULL",
        [],
    )?;
    // 壊れた旧外部キーを含むDBでもProblemを失わない。通常は作成されない退避先。
    let unassigned: i64 = conn.query_row(
        "SELECT COUNT(*) FROM problems WHERE bank_node_id IS NULL",
        [],
        |row| row.get(0),
    )?;
    if unassigned > 0 {
        conn.execute(
            "INSERT INTO bank_nodes(parent_id, name, sort_order, metadata_json)
             SELECT NULL, '移行済み未分類', COALESCE(MAX(sort_order),0)+1, '{\"migration_fallback\":true}'
             FROM bank_nodes
             WHERE NOT EXISTS(SELECT 1 FROM bank_nodes WHERE metadata_json='{\"migration_fallback\":true}')",
            [],
        )?;
        conn.execute(
            "UPDATE problems SET bank_node_id=(SELECT id FROM bank_nodes
                                                WHERE metadata_json='{\"migration_fallback\":true}' LIMIT 1)
             WHERE bank_node_id IS NULL",
            [],
        )?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_problems_bank_node ON problems(bank_node_id)",
        [],
    )?;
    conn.execute_batch(
        r#"
CREATE TRIGGER IF NOT EXISTS sync_problem_bank_node_insert
AFTER INSERT ON problems
WHEN NEW.bank_node_id IS NULL
BEGIN
    UPDATE problems
    SET bank_node_id=(SELECT id FROM bank_nodes WHERE legacy_kind='unit' AND legacy_id=NEW.unit_id)
    WHERE id=NEW.id;
END;
CREATE TRIGGER IF NOT EXISTS sync_problem_bank_node_legacy_move
AFTER UPDATE OF unit_id ON problems
WHEN NEW.unit_id != OLD.unit_id AND NEW.bank_node_id = OLD.bank_node_id
BEGIN
    UPDATE problems
    SET bank_node_id=(SELECT id FROM bank_nodes WHERE legacy_kind='unit' AND legacy_id=NEW.unit_id)
    WHERE id=NEW.id;
END;
"#,
    )?;

    // ---- v14: 部品も問題バンクと同じ任意深度BankNodeへ所属 ----
    // unit_idは旧クライアント・AI操作との互換列として残し、bank_node_idを正本にする。
    // 未分類部品は両方NULLのまま維持する。
    // v4以前のDBではunit_id自体がまだ無いため、v14バックフィルより先に保証する。
    ensure_column(
        conn,
        "parts",
        "unit_id",
        "unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL",
    )?;
    ensure_column(
        conn,
        "part_versions",
        "unit_id",
        "unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL",
    )?;
    ensure_column(
        conn,
        "parts",
        "bank_node_id",
        "bank_node_id INTEGER REFERENCES bank_nodes(id) ON DELETE SET NULL",
    )?;
    ensure_column(
        conn,
        "part_versions",
        "bank_node_id",
        "bank_node_id INTEGER REFERENCES bank_nodes(id) ON DELETE SET NULL",
    )?;
    conn.execute(
        "UPDATE parts
         SET bank_node_id=(SELECT id FROM bank_nodes
                           WHERE legacy_kind='unit' AND legacy_id=parts.unit_id)
         WHERE bank_node_id IS NULL AND unit_id IS NOT NULL",
        [],
    )?;
    conn.execute(
        "UPDATE part_versions
         SET bank_node_id=(SELECT id FROM bank_nodes
                           WHERE legacy_kind='unit' AND legacy_id=part_versions.unit_id)
         WHERE bank_node_id IS NULL AND unit_id IS NOT NULL",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_parts_bank_node ON parts(bank_node_id)",
        [],
    )?;
    conn.execute_batch(
        r#"
CREATE TRIGGER IF NOT EXISTS sync_part_bank_node_insert
AFTER INSERT ON parts
WHEN NEW.bank_node_id IS NULL AND NEW.unit_id IS NOT NULL
BEGIN
    UPDATE parts
    SET bank_node_id=(SELECT id FROM bank_nodes WHERE legacy_kind='unit' AND legacy_id=NEW.unit_id)
    WHERE id=NEW.id;
END;
CREATE TRIGGER IF NOT EXISTS sync_part_bank_node_legacy_move
AFTER UPDATE OF unit_id ON parts
WHEN NEW.unit_id IS NOT OLD.unit_id AND NEW.bank_node_id IS OLD.bank_node_id
BEGIN
    UPDATE parts
    SET bank_node_id=(SELECT id FROM bank_nodes WHERE legacy_kind='unit' AND legacy_id=NEW.unit_id)
    WHERE id=NEW.id;
END;
"#,
    )?;

    // AI解答作成の拡張情報。既存のanswer/explanation列はPDF・教材・旧クライアントとの
    // 互換出力として維持し、Strategy/Plan/Verificationは追加JSON列へ保存する。
    ensure_column(
        conn,
        "problems",
        "solution_variants_json",
        "solution_variants_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "problem_versions",
        "solution_variants_json",
        "solution_variants_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    // 問題文は一段組版と二段組版を別々に保持する。既存問題は従来版を
    // 両方へ引き継ぎ、AIまたは手編集で二段組版だけを後から最適化できる。
    ensure_column(
        conn,
        "problems",
        "statement_latex_two_column",
        "statement_latex_two_column TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "problem_versions",
        "statement_latex_two_column",
        "statement_latex_two_column TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_statement_two_column",
        "snap_statement_two_column TEXT NOT NULL DEFAULT ''",
    )?;
    conn.execute(
        "UPDATE problems
         SET statement_latex_two_column=statement_latex
         WHERE statement_latex_two_column=''",
        [],
    )?;
    conn.execute(
        "UPDATE problem_versions
         SET statement_latex_two_column=statement_latex
         WHERE statement_latex_two_column=''",
        [],
    )?;
    conn.execute(
        "UPDATE project_items
         SET snap_statement_two_column=snap_statement
         WHERE snap_statement_two_column=''",
        [],
    )?;
    // 問題の新難易度分類（既存 difficulty は保持）
    ensure_column(
        conn,
        "problems",
        "difficulty_rank",
        "difficulty_rank TEXT DEFAULT NULL",
    )?;
    ensure_column(
        conn,
        "problems",
        "is_required",
        "is_required INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "problems",
        "answer_completed",
        "answer_completed INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "problems",
        "explanation_completed",
        "explanation_completed INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "problem_versions",
        "difficulty_rank",
        "difficulty_rank TEXT DEFAULT NULL",
    )?;
    ensure_column(
        conn,
        "problem_versions",
        "is_required",
        "is_required INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "problem_versions",
        "answer_completed",
        "answer_completed INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "problem_versions",
        "explanation_completed",
        "explanation_completed INTEGER NOT NULL DEFAULT 0",
    )?;
    // 教材プロジェクト: 使用テンプレートとそのスナップショット
    ensure_column(
        conn,
        "projects",
        "template_id",
        "template_id INTEGER REFERENCES templates(id) ON DELETE SET NULL",
    )?;
    ensure_column(
        conn,
        "projects",
        "snap_tpl_name",
        "snap_tpl_name TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "projects",
        "snap_tpl_base",
        "snap_tpl_base TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "projects",
        "snap_tpl_problem",
        "snap_tpl_problem TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "projects",
        "snap_tpl_answer",
        "snap_tpl_answer TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "projects",
        "snap_tpl_assets",
        "snap_tpl_assets TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "projects",
        "snap_tpl_compile",
        "snap_tpl_compile TEXT NOT NULL DEFAULT 'uplatex+dvipdfmx'",
    )?;
    // 出力設定の拡張
    ensure_column(
        conn,
        "project_settings",
        "subtitle",
        "subtitle TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "header_left",
        "header_left TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "header_right",
        "header_right TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "number_format",
        "number_format TEXT NOT NULL DEFAULT '問題{n}'",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "answers_two_column",
        "answers_two_column INTEGER NOT NULL DEFAULT 0",
    )?;
    // 2段組の範囲: none / all（問題＋解答全体） / answer_only（解答部分のみ）
    ensure_column(
        conn,
        "project_settings",
        "two_column_mode",
        "two_column_mode TEXT NOT NULL DEFAULT 'none'",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "problem_two_column",
        "problem_two_column INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "show_title",
        "show_title INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "show_header",
        "show_header INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "show_toc",
        "show_toc INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "project_settings",
        "number_headings",
        "number_headings INTEGER NOT NULL DEFAULT 0",
    )?;
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
    ensure_column(
        conn,
        "project_items",
        "heading_level",
        "heading_level INTEGER NOT NULL DEFAULT 1",
    )?;
    // この見出しに番号を振るか（全体設定 number_headings がONのときのみ有効）
    ensure_column(
        conn,
        "project_items",
        "heading_numbered",
        "heading_numbered INTEGER NOT NULL DEFAULT 1",
    )?;
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
    ensure_column(
        conn,
        "project_items",
        "snap_difficulty_rank",
        "snap_difficulty_rank TEXT DEFAULT NULL",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_is_required",
        "snap_is_required INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "project_items",
        "part_id",
        "part_id INTEGER REFERENCES parts(id) ON DELETE SET NULL",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_part_type",
        "snap_part_type TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_part_category",
        "snap_part_category TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_part_description",
        "snap_part_description TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_part_output_target",
        "snap_part_output_target TEXT NOT NULL DEFAULT 'both'",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_part_attachments",
        "snap_part_attachments TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_part_layout_mode",
        "snap_part_layout_mode TEXT NOT NULL DEFAULT 'single_column'",
    )?;
    ensure_column(
        conn,
        "parts",
        "layout_mode",
        "layout_mode TEXT NOT NULL DEFAULT 'single_column'",
    )?;
    ensure_column(
        conn,
        "part_versions",
        "layout_mode",
        "layout_mode TEXT NOT NULL DEFAULT 'single_column'",
    )?;
    ensure_column(
        conn,
        "parts",
        "unit_id",
        "unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL",
    )?;
    ensure_column(
        conn,
        "part_versions",
        "unit_id",
        "unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL",
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_parts_unit ON parts(unit_id)",
        [],
    )?;
    // 旧 answers_two_column フラグを新形式へ移行（一度だけ実行される）
    conn.execute(
        "UPDATE project_settings SET two_column_mode='all', answers_two_column=0 WHERE answers_two_column=1",
        [],
    )?;
    // ---- Web版・同時編集対応: 楽観的ロック用のversion列 ----
    ensure_column(
        conn,
        "problems",
        "version",
        "version INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "project_items",
        "version",
        "version INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "projects",
        "version",
        "version INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "templates",
        "version",
        "version INTEGER NOT NULL DEFAULT 1",
    )?;
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
    inserted_at TEXT NOT NULL DEFAULT '',
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

-- 自然言語から既存機能を操作するAIチャット。モデルはこの表やDBへ直接アクセスせず、
-- ai_chatモジュールが公開する許可済みToolだけを実行する。
CREATE TABLE IF NOT EXISTS ai_chat_sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '新しいチャット',
    status TEXT NOT NULL DEFAULT 'idle',
    execution_mode TEXT NOT NULL DEFAULT 'confirm',
    context_json TEXT NOT NULL DEFAULT '{}',
    pending_calls_json TEXT NOT NULL DEFAULT '[]',
    active_user_message_id INTEGER,
    last_error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_chat_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES ai_chat_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    attachments_json TEXT NOT NULL DEFAULT '[]',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'completed',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_action_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES ai_chat_sessions(id) ON DELETE CASCADE,
    user_message_id INTEGER REFERENCES ai_chat_messages(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'running',
    summary TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id INTEGER NOT NULL REFERENCES ai_action_groups(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    permission TEXT NOT NULL,
    target_type TEXT NOT NULL DEFAULT '',
    target_id TEXT NOT NULL DEFAULT '',
    parameters_json TEXT NOT NULL DEFAULT '{}',
    before_json TEXT NOT NULL DEFAULT 'null',
    after_json TEXT NOT NULL DEFAULT 'null',
    status TEXT NOT NULL DEFAULT 'applied',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_chat_messages_session
    ON ai_chat_messages(session_id, id);
CREATE INDEX IF NOT EXISTS idx_ai_action_groups_session
    ON ai_action_groups(session_id, id DESC);
CREATE INDEX IF NOT EXISTS idx_ai_actions_group
    ON ai_actions(group_id, id);
"#,
    )?;
    ensure_column(
        conn,
        "ai_conversion_jobs",
        "inserted_at",
        "inserted_at TEXT NOT NULL DEFAULT ''",
    )?;
    // 旧版で直接挿入・ソース修正を行ったジョブは、記録済みイベントから可能な範囲で復元する。
    conn.execute(
        "UPDATE ai_conversion_jobs
         SET inserted_at=COALESCE(
             (SELECT MAX(created_at)
              FROM ai_conversion_events
              WHERE ai_conversion_events.job_id=ai_conversion_jobs.id
                AND kind IN ('inserted','revision_applied')),
             ''
         )
         WHERE inserted_at=''",
        [],
    )?;

    // ---- v15: 定石を教材へ挿入できるようにする ----
    // 教材側はスナップショットを持ち、定石ライブラリを更新しても既存教材は変わらない。
    ensure_column(
        conn,
        "project_items",
        "pattern_id",
        "pattern_id INTEGER REFERENCES patterns(id) ON DELETE SET NULL",
    )?;
    ensure_column(
        conn,
        "project_items",
        "snap_pattern_json",
        "snap_pattern_json TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "patterns",
        "source_kind",
        "source_kind TEXT NOT NULL DEFAULT ''",
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
    let current_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
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
            let source =
                Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let mut target = Connection::open(&dest)?;
            {
                let backup = rusqlite::backup::Backup::new(&source, &mut target)?;
                backup.run_to_completion(64, std::time::Duration::from_millis(5), None)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    #[test]
    fn v14_migrates_part_membership_without_changing_part_id() {
        let dir = TempDir::new("kyozai-v14-part-migration").unwrap();
        let path = dir.path().join("kyozai-kobo.db");
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            r#"
            PRAGMA foreign_keys=ON;
            CREATE TABLE subjects(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE fields(id INTEGER PRIMARY KEY AUTOINCREMENT, subject_id INTEGER NOT NULL REFERENCES subjects(id), name TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE units(id INTEGER PRIMARY KEY AUTOINCREMENT, field_id INTEGER NOT NULL REFERENCES fields(id), name TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE parts(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL,
                title TEXT NOT NULL DEFAULT '',
                part_type TEXT NOT NULL DEFAULT 'text',
                category TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE part_versions(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                part_id INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
                unit_id INTEGER REFERENCES units(id) ON DELETE SET NULL
            );
            INSERT INTO subjects(id,name,sort_order) VALUES(10,'数学',1);
            INSERT INTO fields(id,subject_id,name,sort_order) VALUES(20,10,'数III',1);
            INSERT INTO units(id,field_id,name,sort_order) VALUES(30,20,'積分',1);
            INSERT INTO parts(id,unit_id,title,part_type,category) VALUES(77,30,'既存部品','text','');
            INSERT INTO part_versions(id,part_id,unit_id) VALUES(88,77,30);
            PRAGMA user_version=13;
            "#,
        )
        .unwrap();
        drop(old);

        let migrated = open_db(dir.path()).unwrap();
        let (part_id, unit_id, node_id): (i64, i64, i64) = migrated
            .query_row(
                "SELECT p.id, p.unit_id, p.bank_node_id FROM parts p WHERE p.id=77",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((part_id, unit_id), (77, 30));
        let node_name: String = migrated
            .query_row(
                "SELECT name FROM bank_nodes WHERE id=?1",
                [node_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(node_name, "積分");
        let version_node: i64 = migrated
            .query_row(
                "SELECT bank_node_id FROM part_versions WHERE id=88",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version_node, node_id);
        assert_eq!(
            migrated
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
    }
}
