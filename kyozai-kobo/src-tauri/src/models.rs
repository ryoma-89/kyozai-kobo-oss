use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionPatternRef {
    pub pattern_id: i64,
    pub strategy_id: i64,
    #[serde(default)]
    pub pattern_version: Option<i64>,
    #[serde(default)]
    pub pattern_title: Option<String>,
    #[serde(default)]
    pub strategy_title: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionStrategyEvaluation {
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub high_school_appropriate: bool,
    #[serde(default)]
    pub exam_natural: bool,
    #[serde(default)]
    pub calculation_cost: String,
    #[serde(default)]
    pub clarity: String,
    #[serde(default)]
    pub educational_value: String,
    #[serde(default)]
    pub distinctness: String,
    #[serde(default)]
    pub recommendation_reason: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionStrategySuitability {
    #[serde(default)]
    pub exam_answer: bool,
    #[serde(default)]
    pub textbook_explanation: bool,
    #[serde(default)]
    pub alternative_solution: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionStrategy {
    pub id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub answer_length: Option<String>,
    #[serde(default)]
    pub concepts: Vec<String>,
    #[serde(default)]
    pub suitability: Option<SolutionStrategySuitability>,
    #[serde(default)]
    pub note: Option<String>,
    /// Pattern全体から今回の解法に使うCandidateを追跡する参照。
    /// Candidateだけを表示するためには使わない。
    #[serde(default)]
    pub pattern_refs: Vec<SolutionPatternRef>,
    #[serde(default)]
    pub evaluation: Option<SolutionStrategyEvaluation>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionPlanStep {
    pub id: String,
    pub purpose: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionPlan {
    pub strategy_id: String,
    #[serde(default)]
    pub outline: Vec<SolutionPlanStep>,
    #[serde(default)]
    pub required_conditions: Vec<String>,
    #[serde(default)]
    pub important_checks: Vec<String>,
    #[serde(default)]
    pub equality_conditions: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct VerificationIssue {
    pub severity: String,
    pub message: String,
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub valid: bool,
    #[serde(default)]
    pub issues: Vec<VerificationIssue>,
    #[serde(default)]
    pub corrected_solution: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionBlock {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationSection {
    #[serde(default)]
    pub solution_block_ids: Vec<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub content: String,
}

/// 解法の発見・判断・見通しを教材化する可変長Block。
/// block_typeごとに使うフィールドだけが設定される。Pattern本文はAIではなく
/// canonical Patternから取得したsnapshotを正本として保持する。
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionFlowBlock {
    pub id: String,
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub latex: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub pattern_id: Option<i64>,
    #[serde(default)]
    pub pattern_version: Option<i64>,
    #[serde(default)]
    pub snapshot: Option<PatternSnapshot>,
    #[serde(default)]
    pub used_strategy_ids: Vec<i64>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProblemSolutionVariant {
    pub id: String,
    pub strategy: SolutionStrategy,
    pub role: String,
    #[serde(default)]
    pub plan: Option<SolutionPlan>,
    #[serde(default)]
    pub solution: String,
    #[serde(default)]
    pub solution_blocks: Vec<SolutionBlock>,
    #[serde(default)]
    pub verification: Option<VerificationResult>,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default)]
    pub explanation_sections: Vec<ExplanationSection>,
    #[serde(default)]
    pub explanation_outdated: bool,
    #[serde(default)]
    pub flow_blocks: Vec<SolutionFlowBlock>,
}

#[derive(Serialize)]
pub struct UnitNode {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub problem_count: i64,
    pub part_count: i64,
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

#[derive(Serialize, Clone)]
pub struct BankNode {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub sort_order: i64,
    /// このノードに直接所属する問題数
    pub problem_count: i64,
    /// このノード自身と全子孫に所属する問題数
    pub descendant_problem_count: i64,
    /// このノードに直接所属する部品数
    pub part_count: i64,
    /// このノード自身と全子孫に所属する部品数
    pub descendant_part_count: i64,
    /// 旧Unit由来のノードだけが持つ互換ID
    pub legacy_unit_id: Option<i64>,
    pub children: Vec<BankNode>,
}

#[derive(Serialize)]
pub struct BankNodeDeleteImpact {
    pub node_id: i64,
    pub child_node_count: i64,
    pub direct_problem_count: i64,
    pub descendant_problem_count: i64,
    pub direct_part_count: i64,
    pub descendant_part_count: i64,
    pub parent_id: Option<i64>,
}

#[derive(Serialize)]
pub struct ProblemSummary {
    pub id: i64,
    pub bank_node_id: i64,
    /// 旧API・AIとの互換用。問題バンクの正本は bank_node_id。
    pub unit_id: i64,
    pub title: String,
    pub difficulty: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub answer_completed: bool,
    pub explanation_completed: bool,
    pub tags: Vec<String>,
    pub created_at: String,
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
    pub bank_node_id: i64,
    /// 旧API・AIとの互換用。問題バンクの正本は bank_node_id。
    pub unit_id: i64,
    pub title: String,
    /// 一段組用の問題文
    pub statement_latex: String,
    /// 二段組の片方の列へ配置する問題文
    pub statement_latex_two_column: String,
    pub answer_latex: String,
    pub explanation_latex: String,
    /// 解法分岐前の共通する着眼・Pattern引用。
    pub common_flow_blocks: Vec<SolutionFlowBlock>,
    /// Strategy -> Flow -> Exam Answer を保持する拡張データ。
    /// 従来の answer_latex / explanation_latex は答案とFlowを連結した互換出力として残す。
    pub solution_variants: Vec<ProblemSolutionVariant>,
    pub answer_completed: bool,
    pub explanation_completed: bool,
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
    #[serde(default)]
    pub bank_node_id: Option<i64>,
    pub unit_id: i64,
    pub title: String,
    /// 一段組用の問題文
    pub statement_latex: String,
    /// 二段組の片方の列へ配置する問題文
    pub statement_latex_two_column: String,
    pub answer_latex: String,
    pub explanation_latex: String,
    #[serde(default)]
    pub common_flow_blocks: Vec<SolutionFlowBlock>,
    #[serde(default)]
    pub solution_variants: Vec<ProblemSolutionVariant>,
    #[serde(default)]
    pub answer_completed: bool,
    #[serde(default)]
    pub explanation_completed: bool,
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
    pub statement_latex_two_column: String,
    pub answer_latex: String,
    pub explanation_latex: String,
    pub common_flow_blocks: Vec<SolutionFlowBlock>,
    pub solution_variants: Vec<ProblemSolutionVariant>,
    pub answer_completed: bool,
    pub explanation_completed: bool,
    pub difficulty: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub memo: String,
    pub saved_at: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub text: String,
    #[serde(default)]
    pub bank_node_id: Option<i64>,
    #[serde(default)]
    pub include_descendants: Option<bool>,
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
    pub answer_completed: bool,
    pub explanation_completed: bool,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub usage_count: i64,
    pub bank_node_id: i64,
    pub bank_path: String,
    pub subject_name: String,
    pub field_name: String,
    pub unit_name: String,
    pub unit_id: i64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct PatternFacets {
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub operations: Vec<String>,
    #[serde(default)]
    pub structures: Vec<String>,
    #[serde(default)]
    pub situations: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct PatternStrategyInput {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub parent_strategy_id: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub branch_label: String,
    pub sort_order: i64,
}

/// Problemから抽出した定石候補内の手法。canonicalなpattern_strategiesへは
/// ユーザーがProposalを承認した時だけ変換される。
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatternProposalStrategy {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub sort_order: i64,
}

/// AIが推定した上位（より一般的な）定石の手掛かり。canonicalなpatternsへは保存せず、
/// ユーザーがchild_patternとして承認したときだけ関連付けの材料になる。
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatternParentHint {
    pub title: String,
    #[serde(default)]
    pub reason: String,
}

/// AI抽出結果の一時Proposal。ai_conversion_jobsの構造化結果として保持し、
/// patternsテーブルへは直接保存しない。
///
/// rawTechnique（元問題で実際に使われた具体的操作）とgeneralized側の本文
/// （title/summary/situation/principle/strategies）を別項目として持ち、
/// 「この問題で何をしたか」と「他の問題でも使える判断知識」を分離する。
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatternProposal {
    #[serde(default)]
    pub proposal_id: String,
    /// 元Problemで実際に使われた具体的手法。固有名・数値を含んでよい。
    #[serde(default)]
    pub raw_technique: String,
    pub title: String,
    pub pattern_type: String,
    // 概要・状況・基本原則はカードに出さないため、AIは生成しない。
    // 旧データを読み込めるように項目自体は残す。
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub situation: String,
    #[serde(default)]
    pub principle: String,
    #[serde(default)]
    pub strategies: Vec<PatternProposalStrategy>,
    #[serde(default)]
    pub cautions: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub operations: Vec<String>,
    #[serde(default)]
    pub structures: Vec<String>,
    #[serde(default)]
    pub situations: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_type: String,
    #[serde(default)]
    pub matched_pattern_id: Option<i64>,
    #[serde(default)]
    pub matched_pattern_title: Option<String>,
    #[serde(default)]
    pub similarity_reason: String,
    #[serde(default)]
    pub action_recommendation: String,
    /// rawTechniqueから何を取り除いて一般化したか。
    #[serde(default)]
    pub generalization_reason: String,
    /// 抽象度。1=principle / 2=strategy / 3=technique / 4=specialized。
    #[serde(default)]
    pub specificity_level: i64,
    /// 0.0〜1.0。他の問題へ再利用できる度合いのAI自己評価。
    #[serde(default)]
    pub reusability_score: f64,
    /// 既存定石検索へ使う一般化されたキーワード。titleそのものより広く当てる。
    #[serde(default)]
    pub search_concepts: Vec<String>,
    /// 元問題固有の対象へ限定されすぎているか。
    #[serde(default)]
    pub is_overly_specific: bool,
    /// SituationやPurposeを失った単なる操作名になっていないか。
    #[serde(default)]
    pub is_overly_general: bool,
    /// 具体・一般どちらかへ偏っている場合の理由。
    #[serde(default)]
    pub specificity_reason: String,
    /// より一般的な上位定石の手掛かり。
    #[serde(default)]
    pub possible_parent_pattern: Option<PatternParentHint>,
    /// 粒度の方針。generalize=もう一段一般化すべき / keep_as_is=今の粒度が定石として適切 /
    /// split_general_and_specific=一般定石と特殊化の両方を残すべき。
    #[serde(default)]
    pub generalization_decision: String,
    /// AIが推奨する保存方式。ユーザーの選択を拘束しない参考値。
    #[serde(default)]
    pub recommended_storage: String,
    /// 自動再一般化を行った回数。無限ループ防止のため上限を設ける。
    #[serde(default)]
    pub generalization_pass_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatternExtractionResult {
    pub schema_version: i64,
    pub kind: String,
    #[serde(default)]
    pub patterns: Vec<PatternProposal>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPatternProposalPayload {
    /// 抽出元Problem。画像・AI Chat・手動由来では0（Problemに紐づかない）。
    #[serde(default)]
    pub problem_id: i64,
    /// 画像ファイル名やチャットセッション等、由来を1行で残すための参照。
    #[serde(default)]
    pub source_reference: String,
    pub proposal: PatternProposal,
    pub action: String,
    #[serde(default)]
    pub target_pattern_id: Option<i64>,
    /// Noneなら関連付けない。指定する場合は applicable / used。
    #[serde(default)]
    pub link_relation_type: Option<String>,
    /// create_child_patternのときだけ使う上位定石。specialization関連を張る。
    #[serde(default)]
    pub parent_pattern_id: Option<i64>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPatternProposalResult {
    pub pattern_id: i64,
    pub action: String,
    pub created: bool,
    pub linked: bool,
}

#[derive(Serialize, Clone)]
pub struct PatternStrategy {
    pub id: i64,
    pub pattern_id: i64,
    pub parent_strategy_id: Option<i64>,
    pub title: String,
    pub description: String,
    pub condition: String,
    pub reasoning: String,
    pub branch_label: String,
    pub sort_order: i64,
}

#[derive(Serialize, Clone)]
pub struct PatternSummary {
    pub id: i64,
    pub uuid: String,
    pub title: String,
    pub summary: String,
    pub pattern_type: String,
    pub tags: Vec<String>,
    pub facets: PatternFacets,
    pub updated_at: String,
    pub version: i64,
    pub strategy_count: i64,
    pub problem_count: i64,
}

#[derive(Serialize, Clone)]
pub struct PatternRelationView {
    pub from_pattern_id: i64,
    pub to_pattern_id: i64,
    pub pattern_id: i64,
    pub title: String,
    pub pattern_type: String,
    pub relation_type: String,
    pub direction: String,
}

#[derive(Serialize, Clone)]
pub struct PatternProblemView {
    pub problem_id: i64,
    pub title: String,
    pub bank_node_id: i64,
    pub bank_path: String,
    pub relation_type: String,
}

#[derive(Serialize, Clone)]
pub struct ProblemPatternView {
    pub pattern_id: i64,
    pub title: String,
    pub summary: String,
    pub pattern_type: String,
    pub relation_type: String,
    pub tags: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct PatternFull {
    pub id: i64,
    pub uuid: String,
    pub title: String,
    pub summary: String,
    pub pattern_type: String,
    pub situation: String,
    pub principle: String,
    pub cautions: String,
    pub examples: String,
    pub source_note: String,
    pub tags: Vec<String>,
    pub facets: PatternFacets,
    pub strategies: Vec<PatternStrategy>,
    pub related_patterns: Vec<PatternRelationView>,
    pub related_problems: Vec<PatternProblemView>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
}

#[derive(Deserialize)]
pub struct PatternUpdate {
    pub id: i64,
    #[serde(default)]
    pub expected_version: Option<i64>,
    pub title: String,
    pub summary: String,
    pub pattern_type: String,
    pub situation: String,
    pub principle: String,
    pub cautions: String,
    pub examples: String,
    #[serde(default)]
    pub source_note: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub facets: PatternFacets,
    #[serde(default)]
    pub strategies: Vec<PatternStrategyInput>,
}

#[derive(Deserialize, Default)]
pub struct PatternSearchQuery {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub pattern_type: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub structure: Option<String>,
    #[serde(default)]
    pub situation: Option<String>,
    #[serde(default)]
    pub exclude_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Serialize, Default)]
pub struct PatternFilterValues {
    pub pattern_types: Vec<String>,
    pub tags: Vec<String>,
    pub domains: Vec<String>,
    pub goals: Vec<String>,
    pub operations: Vec<String>,
    pub structures: Vec<String>,
    pub situations: Vec<String>,
}

#[derive(Serialize)]
pub struct PatternDeleteImpact {
    pub pattern_id: i64,
    pub problem_count: i64,
    pub related_pattern_count: i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PatternSnapshot {
    /// スナップショットを取った時点の定石の版。教材側で「新しい版があるか」を見るために持つ。
    /// 版を持たない旧スナップショットは0になる。
    #[serde(default)]
    pub version: i64,
    pub uuid: String,
    pub title: String,
    pub summary: String,
    pub pattern_type: String,
    pub situation: String,
    pub principle: String,
    pub cautions: String,
    pub examples: String,
    pub source_note: String,
    pub tags: Vec<String>,
    pub facets: PatternFacets,
    pub strategies: Vec<PatternStrategyInput>,
}

#[derive(Serialize)]
pub struct PatternVersionSummary {
    pub id: i64,
    pub pattern_id: i64,
    pub title: String,
    pub version: i64,
    pub saved_at: String,
}

#[derive(Serialize)]
pub struct PatternVersionFull {
    pub id: i64,
    pub pattern_id: i64,
    pub version: i64,
    pub saved_at: String,
    pub snapshot: PatternSnapshot,
}

#[derive(Serialize)]
pub struct ImportPatternsResult {
    pub created: i64,
    pub skipped: i64,
    pub relations_created: i64,
    pub problem_relations_created: i64,
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
    pub snap_statement_two_column: String,
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
    pub snap_part_layout_mode: String,
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
    /// 定石項目の元Pattern（削除済みならNone）
    pub pattern_id: Option<i64>,
    /// 追加時点のPatternSnapshotのJSON。教材側の正本はこちら。
    pub snap_pattern_json: String,
    /// 定石ライブラリ側に新しい版があるか
    pub pattern_updated: bool,
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
    pub snap_statement_two_column: Option<String>,
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
    pub snap_part_layout_mode: Option<String>,
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
    /// 問題冊子（合本の問題編を含む）を縦線付き2段組にする
    #[serde(default)]
    pub problem_two_column: bool,
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
    pub bank_node_id: Option<i64>,
    pub bank_path: String,
    pub unit_id: Option<i64>,
    pub unit_name: String,
    pub field_id: Option<i64>,
    pub field_name: String,
    pub subject_id: Option<i64>,
    pub subject_name: String,
    pub title: String,
    pub part_type: String,
    pub category: String,
    pub tags: Vec<String>,
    pub plain_text_preview: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub output_target: String,
    pub layout_mode: String,
    pub usage_count: i64,
    pub updated_at: String,
    pub version: i64,
}

#[derive(Serialize)]
pub struct PartFull {
    pub id: i64,
    pub bank_node_id: Option<i64>,
    pub bank_path: String,
    pub unit_id: Option<i64>,
    pub unit_name: String,
    pub field_id: Option<i64>,
    pub field_name: String,
    pub subject_id: Option<i64>,
    pub subject_name: String,
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
    pub layout_mode: String,
    pub usage_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub attachments: Vec<PartAttachment>,
}

#[derive(Deserialize)]
pub struct PartUpdate {
    pub id: i64,
    #[serde(default)]
    pub bank_node_id: Option<i64>,
    #[serde(default)]
    pub unit_id: Option<i64>,
    pub title: String,
    pub part_type: String,
    pub category: String,
    pub tags: Vec<String>,
    pub latex_source: String,
    pub description: String,
    pub difficulty_rank: Option<String>,
    pub is_required: bool,
    pub output_target: String,
    #[serde(default = "default_part_layout_mode")]
    pub layout_mode: String,
    /// 編集開始時のversion。指定時にサーバー側と一致しなければ競合エラー
    #[serde(default)]
    pub expected_version: Option<i64>,
}

fn default_part_layout_mode() -> String {
    "single_column".to_string()
}

#[derive(Deserialize)]
pub struct PartSearchQuery {
    pub text: String,
    #[serde(default)]
    pub bank_node_id: Option<i64>,
    #[serde(default)]
    pub include_descendants: Option<bool>,
    pub subject_id: Option<i64>,
    pub field_id: Option<i64>,
    pub unit_id: Option<i64>,
    pub part_type: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub difficulty_rank: Option<String>,
    pub difficulty_ranks: Option<Vec<String>>,
    pub required_filter: Option<String>,
    pub unassigned_only: Option<bool>,
}

#[derive(Serialize)]
pub struct PartVersionSummary {
    pub id: i64,
    pub title: String,
    pub version: i64,
    pub saved_at: String,
}
