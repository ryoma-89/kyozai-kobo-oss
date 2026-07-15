use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct UnitNode {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub problem_count: i64,
}

#[derive(Serialize)]
pub struct FieldNode {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub units: Vec<UnitNode>,
}

#[derive(Serialize)]
pub struct SubjectNode {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub fields: Vec<FieldNode>,
}

#[derive(Serialize)]
pub struct ProblemSummary {
    pub id: i64,
    pub unit_id: i64,
    pub title: String,
    pub difficulty: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub usage_count: i64,
}

#[derive(Serialize)]
pub struct Attachment {
    pub id: i64,
    pub problem_id: i64,
    pub file_name: String,
    pub stored_name: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ProblemFull {
    pub id: i64,
    pub unit_id: i64,
    pub title: String,
    pub statement_latex: String,
    pub answer_latex: String,
    pub explanation_latex: String,
    pub difficulty: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub memo: String,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<String>,
    pub attachments: Vec<Attachment>,
    /// 楽観的ロック用バージョン
    pub version: i64,
}

#[derive(Deserialize)]
pub struct ProblemUpdate {
    pub id: i64,
    pub unit_id: i64,
    pub title: String,
    pub statement_latex: String,
    pub answer_latex: String,
    pub explanation_latex: String,
    pub difficulty: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub memo: String,
    pub tags: Vec<String>,
    /// 編集開始時のversion。指定時にサーバー側と一致しなければ競合エラー
    #[serde(default)]
    pub expected_version: Option<i64>,
}

#[derive(Serialize)]
pub struct VersionSummary {
    pub id: i64,
    pub title: String,
    pub saved_at: String,
}

#[derive(Serialize)]
pub struct VersionFull {
    pub id: i64,
    pub problem_id: i64,
    pub title: String,
    pub statement_latex: String,
    pub answer_latex: String,
    pub explanation_latex: String,
    pub difficulty: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub memo: String,
    pub saved_at: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub subject_id: Option<i64>,
    pub field_id: Option<i64>,
    pub unit_id: Option<i64>,
    pub difficulty: Option<String>,
    pub difficulty_rank: Option<String>,
    pub difficulty_ranks: Option<Vec<String>>,
    pub required_filter: Option<String>,
    pub tag: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub id: i64,
    pub title: String,
    pub difficulty: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub usage_count: i64,
    pub subject_name: String,
    pub field_name: String,
    pub unit_name: String,
    pub unit_id: i64,
}

#[derive(Serialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub updated_at: String,
    pub item_count: i64,
    pub version: i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SnapAttachment {
    pub file_name: String,
    pub stored_name: String,
}

#[derive(Serialize)]
pub struct ProjectItem {
    pub id: i64,
    pub project_id: i64,
    pub item_type: String,
    pub sort_order: i64,
    pub problem_id: Option<i64>,
    pub part_id: Option<i64>,
    pub snap_title: String,
    pub snap_statement: String,
    pub snap_answer: String,
    pub snap_explanation: String,
    pub snap_difficulty: String,
    pub snap_difficulty_rank: Option<String>,
    pub snap_is_required: bool,
    pub snap_attachments: Vec<SnapAttachment>,
    pub content: String,
    pub snap_part_type: String,
    pub snap_part_category: String,
    pub snap_part_description: String,
    pub snap_part_output_target: String,
    pub snap_part_attachments: Vec<SnapAttachment>,
    /// 見出しのレベル: 1=章(section), 2=節(subsection)
    pub heading_level: i64,
    /// この見出しに番号を振るか（全体設定 number_headings がONのときのみ有効）
    pub heading_numbered: bool,
    /// 問題バンク側が更新されているか（スナップショットとの差分有無）
    pub bank_updated: bool,
    /// 元問題がまだ存在するか
    pub source_exists: bool,
    /// 部品ライブラリ側が更新されているか（スナップショットとの差分有無）
    pub part_updated: bool,
    /// 楽観的ロック用バージョン
    pub version: i64,
}

/// update_project_item の引数（フロントは camelCase キーで送る）
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectItemUpdate {
    pub item_id: i64,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub snap_title: Option<String>,
    #[serde(default)]
    pub snap_statement: Option<String>,
    #[serde(default)]
    pub snap_answer: Option<String>,
    #[serde(default)]
    pub snap_explanation: Option<String>,
    #[serde(default)]
    pub snap_difficulty_rank: Option<String>,
    #[serde(default)]
    pub snap_is_required: Option<bool>,
    #[serde(default)]
    pub snap_part_type: Option<String>,
    #[serde(default)]
    pub snap_part_category: Option<String>,
    #[serde(default)]
    pub snap_part_description: Option<String>,
    #[serde(default)]
    pub snap_part_output_target: Option<String>,
    #[serde(default)]
    pub heading_level: Option<i64>,
    #[serde(default)]
    pub heading_numbered: Option<bool>,
    /// 編集開始時のversion。指定時にサーバー側と一致しなければ競合エラー
    #[serde(default)]
    pub expected_version: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ProjectSettings {
    pub booklet_title: String,
    pub subtitle: String,
    pub target: String,
    pub date_str: String,
    pub header_left: String,
    pub header_right: String,
    pub number_format: String,
    pub show_name_field: bool,
    pub auto_number: bool,
    pub page_break_per_problem: bool,
    pub include_explanation: bool,
    /// 解答冊子の2段組: "none" | "all"（問題＋解答全体） | "answer_only"（解答部分のみ）
    pub two_column_mode: String,
    /// 教材タイトル（{{TITLE}}/{{SUBTITLE}}）を出力するか
    pub show_title: bool,
    /// ヘッダー（{{HEADER_LEFT}}/{{HEADER_RIGHT}}）を出力するか
    pub show_header: bool,
    /// 目次（\tableofcontents）を付けるか
    pub show_toc: bool,
    /// 見出しに番号を振るか（\section / \section*）
    pub number_headings: bool,
    /// 解答冊子に問題文を含めるか
    pub include_statement_in_answers: bool,
    /// 解答冊子に含めた問題文を枠で囲むか
    pub box_statement_in_answers: bool,
    /// 章ごとに問題番号をリセットするか（番号付き章では「2-1」形式）
    pub reset_numbering_per_chapter: bool,
    /// 問題のA/B/C/D表示: none | number_side | top_right
    pub difficulty_display: String,
    /// ★表示: none | required_only
    pub required_display: String,
}

#[derive(Serialize)]
pub struct ProjectFull {
    pub id: i64,
    pub version: i64,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub settings: ProjectSettings,
    pub items: Vec<ProjectItem>,
    /// 使用テンプレート（削除済みならNone）
    pub template_id: Option<i64>,
    pub template_name: String,
    /// スナップショット取得後にテンプレート本体が更新されたか
    pub template_updated: bool,
}

#[derive(Serialize)]
pub struct TemplateSummary {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub compile_method: String,
    pub updated_at: String,
    pub usage_count: i64,
}

#[derive(Serialize)]
pub struct TemplateFull {
    pub id: i64,
    pub version: i64,
    pub name: String,
    pub description: String,
    pub base_template: String,
    pub problem_template: String,
    pub answer_template: String,
    pub compile_method: String,
    pub packages_memo: String,
    pub created_at: String,
    pub updated_at: String,
    pub assets: Vec<TemplateAsset>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct TemplateAsset {
    pub id: i64,
    pub template_id: i64,
    pub file_name: String,
    pub stored_name: String,
}

#[derive(Deserialize)]
pub struct TemplateUpdate {
    pub id: i64,
    pub expected_version: Option<i64>,
    pub name: String,
    pub description: String,
    pub base_template: String,
    pub problem_template: String,
    pub answer_template: String,
    pub compile_method: String,
    pub packages_memo: String,
}

#[derive(Serialize)]
pub struct TemplateVersionSummary {
    pub id: i64,
    pub name: String,
    pub saved_at: String,
}

#[derive(Serialize)]
pub struct ImportAnalysis {
    pub doc_class: String,
    pub packages: Vec<String>,
    pub has_body_placeholder: bool,
    pub has_markers: bool,
    pub has_document_env: bool,
    pub referenced_files: Vec<String>,
    pub content: String,
}

#[derive(Serialize)]
pub struct CompileResult {
    pub success: bool,
    pub pdf_path: Option<String>,
    pub tex_path: Option<String>,
    pub log: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct TexDetection {
    pub uplatex_path: Option<String>,
    pub dvipdfmx_path: Option<String>,
}

#[derive(Serialize)]
pub struct PartAttachment {
    pub id: i64,
    pub part_id: i64,
    pub file_name: String,
    pub stored_name: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct PartSummary {
    pub id: i64,
    pub title: String,
    pub part_type: String,
    pub category: String,
    pub tags: Vec<String>,
    pub plain_text_preview: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub output_target: String,
    pub usage_count: i64,
    pub updated_at: String,
    pub version: i64,
}

#[derive(Serialize)]
pub struct PartFull {
    pub id: i64,
    pub title: String,
    pub part_type: String,
    pub category: String,
    pub tags: Vec<String>,
    pub latex_source: String,
    pub plain_text_preview: String,
    pub description: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub output_target: String,
    pub usage_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub attachments: Vec<PartAttachment>,
}

#[derive(Deserialize)]
pub struct PartUpdate {
    pub id: i64,
    pub title: String,
    pub part_type: String,
    pub category: String,
    pub tags: Vec<String>,
    pub latex_source: String,
    pub description: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub output_target: String,
    /// 編集開始時のversion。指定時にサーバー側と一致しなければ競合エラー
    #[serde(default)]
    pub expected_version: Option<i64>,
}

#[derive(Deserialize)]
pub struct PartSearchQuery {
    pub text: String,
    pub part_type: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub difficulty_rank: Option<String>,
    pub difficulty_ranks: Option<Vec<String>>,
    pub required_filter: Option<String>,
}

#[derive(Serialize)]
pub struct PartVersionSummary {
    pub id: i64,
    pub title: String,
    pub version: i64,
    pub saved_at: String,
}
