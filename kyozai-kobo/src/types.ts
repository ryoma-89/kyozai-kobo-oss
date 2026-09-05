export interface UnitNode {
  id: number;
  name: string;
  sort_order: number;
  problem_count: number;
  part_count: number;
}

export interface FieldNode {
  id: number;
  name: string;
  sort_order: number;
  units: UnitNode[];
}

export interface SubjectNode {
  id: number;
  name: string;
  sort_order: number;
  fields: FieldNode[];
}

export type NodeKind = "subject" | "field" | "unit";

/** 問題と部品が共有する任意深度のバンク階層。 */
export interface BankNode {
  id: number;
  parent_id: number | null;
  name: string;
  sort_order: number;
  problem_count: number;
  descendant_problem_count: number;
  part_count: number;
  descendant_part_count: number;
  legacy_unit_id: number | null;
  children: BankNode[];
}

export interface BankNodeDeleteImpact {
  node_id: number;
  child_node_count: number;
  direct_problem_count: number;
  descendant_problem_count: number;
  direct_part_count: number;
  descendant_part_count: number;
  parent_id: number | null;
}

export type BankNodeDeleteStrategy = "delete_all" | "move_to_parent";

export type Difficulty = "基礎" | "標準" | "発展";

export interface SolutionPatternRef {
  patternId: number;
  strategyId: number;
  /** Rust側でcanonical Patternから補完する表示用情報。 */
  patternVersion?: number;
  patternTitle?: string;
  strategyTitle?: string;
}

export interface SolutionStrategyEvaluation {
  complete: boolean;
  highSchoolAppropriate: boolean;
  examNatural: boolean;
  calculationCost: "low" | "medium" | "high";
  clarity: "low" | "medium" | "high";
  educationalValue: "low" | "medium" | "high";
  distinctness: "low" | "medium" | "high";
  recommendationReason: string;
}

export interface SolutionStrategy {
  id: string;
  title: string;
  summary: string;
  difficulty?: "basic" | "standard" | "advanced";
  answerLength?: "short" | "medium" | "long";
  concepts?: string[];
  suitability?: {
    examAnswer?: boolean;
    textbookExplanation?: boolean;
    alternativeSolution?: boolean;
  };
  note?: string;
  /** 解法の起点になったPattern Candidate。表示内容の抽出には使わない。 */
  patternRefs?: SolutionPatternRef[];
  evaluation?: SolutionStrategyEvaluation;
}

export interface ProblemAnalysis {
  subject: string;
  problemType: string;
  conditions: string[];
  concepts: string[];
  cautions: string[];
}

export interface SolutionPlan {
  strategyId: string;
  outline: Array<{ id: string; purpose: string; content: string }>;
  requiredConditions?: string[];
  importantChecks?: string[];
  equalityConditions?: string[];
}

export interface VerificationResult {
  valid: boolean;
  issues: Array<{
    severity: "warning" | "error";
    message: string;
    location?: string;
  }>;
  correctedSolution?: string;
}

export interface SolutionBlock {
  id: string;
  content: string;
  role?: string;
}

export interface ExplanationSection {
  solutionBlockIds: string[];
  title?: string;
  content: string;
}

export type SolutionFlowBlock =
  | {
      id: string;
      type: "text";
      content: string;
    }
  | {
      id: string;
      type: "pattern";
      patternId: number;
      patternVersion: number;
      snapshot: PatternSnapshot;
      /** 全Candidateを表示したまま、カード後の説明と解法の対応を追跡するために使う。 */
      usedStrategyIds: number[];
    }
  | {
      id: string;
      type: "formula";
      latex: string;
    }
  | {
      id: string;
      type: "heading";
      text: string;
    }
  | {
      id: string;
      type: "caution";
      content: string;
    };

export interface ProblemSolutionVariant {
  id: string;
  strategy: SolutionStrategy;
  role: "main" | "alternative";
  plan?: SolutionPlan;
  solution: string;
  solutionBlocks?: SolutionBlock[];
  verification?: VerificationResult;
  explanation?: string;
  explanationSections?: ExplanationSection[];
  explanationOutdated?: boolean;
  /** 答案を逐語的に説明せず、解法の発見・判断・見通しを表す可変長Block列。 */
  flowBlocks?: SolutionFlowBlock[];
}

export interface StrategyValidationResult {
  valid: boolean;
  message: string;
  normalizedStrategy: SolutionStrategy;
  suggestedStrategy: SolutionStrategy;
}

export interface ProblemSummary {
  id: number;
  bank_node_id: number;
  /** 旧API・AI互換用。問題バンクの所属正本は bank_node_id。 */
  unit_id: number;
  title: string;
  difficulty: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  answer_completed: boolean;
  explanation_completed: boolean;
  tags: string[];
  created_at: string;
  updated_at: string;
  usage_count: number;
}

export interface Attachment {
  id: number;
  problem_id: number;
  file_name: string;
  stored_name: string;
  created_at: string;
}

export interface ProblemFull {
  id: number;
  bank_node_id: number;
  /** 旧API・AI互換用。問題バンクの所属正本は bank_node_id。 */
  unit_id: number;
  title: string;
  /** 一段組用の問題文 */
  statement_latex: string;
  /** 二段組の片方の列へ配置する問題文 */
  statement_latex_two_column: string;
  answer_latex: string;
  explanation_latex: string;
  common_flow_blocks: SolutionFlowBlock[];
  solution_variants: ProblemSolutionVariant[];
  answer_completed: boolean;
  explanation_completed: boolean;
  difficulty: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  memo: string;
  created_at: string;
  updated_at: string;
  tags: string[];
  attachments: Attachment[];
  /** 楽観的ロック用バージョン */
  version: number;
}

export interface VersionSummary {
  id: number;
  title: string;
  saved_at: string;
}

export interface VersionFull {
  id: number;
  problem_id: number;
  title: string;
  statement_latex: string;
  statement_latex_two_column: string;
  answer_latex: string;
  explanation_latex: string;
  common_flow_blocks: SolutionFlowBlock[];
  solution_variants: ProblemSolutionVariant[];
  answer_completed: boolean;
  explanation_completed: boolean;
  difficulty: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  memo: string;
  saved_at: string;
}

export interface SearchQuery {
  text: string;
  bank_node_id?: number | null;
  include_descendants?: boolean | null;
  subject_id?: number | null;
  field_id?: number | null;
  unit_id?: number | null;
  difficulty?: string | null;
  difficulty_rank?: DifficultyRank | null;
  difficulty_ranks?: (DifficultyRank | "__unset")[] | null;
  required_filter?: "all" | "required" | "not_required" | null;
  tag?: string | null;
}

export interface SearchResult {
  id: number;
  title: string;
  difficulty: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  answer_completed: boolean;
  explanation_completed: boolean;
  tags: string[];
  updated_at: string;
  usage_count: number;
  bank_node_id: number;
  bank_path: string;
  subject_name: string;
  field_name: string;
  unit_name: string;
  unit_id: number;
}

export type PatternType = "strategy" | "technique" | "calculation_tip" | "check" | string;
export type ProblemPatternRelationType = "applicable" | "used" | string;
export type PatternRelationType =
  | "related"
  | "prerequisite"
  | "derived"
  | "alternative"
  | "generalization"
  | "specialization"
  | string;

export interface PatternFacets {
  domains: string[];
  goals: string[];
  operations: string[];
  structures: string[];
  situations: string[];
}

export interface PatternStrategyInput {
  id?: number | null;
  parent_strategy_id?: number | null;
  title: string;
  description: string;
  condition: string;
  reasoning: string;
  branch_label: string;
  sort_order: number;
}

export interface PatternStrategy extends PatternStrategyInput {
  id: number;
  pattern_id: number;
  parent_strategy_id: number | null;
}

export type PatternProposalSourceType =
  | "solution_used"
  | "explanation_used"
  | "ai_inferred"
  | "image_import"
  | "ai_chat"
  | "manual";
export type PatternProposalRecommendation =
  | "create_new"
  | "create_child_pattern"
  | "merge_into_existing"
  | "add_candidate_to_existing"
  | "add_caution_to_existing"
  | "add_example_to_existing"
  | "duplicate"
  | "ignore";

/** AIが推奨する保存方式。ユーザーの選択を拘束しない参考値。 */
export type PatternProposalStorage =
  | "new_pattern"
  | "child_pattern"
  | "example"
  | "candidate_strategy"
  | "merge_existing"
  | "duplicate"
  | "ignore";

/** 粒度の方針。keep_as_is は「今の粒度が入試数学の定石として適切」という正当な結論。 */
export type PatternGeneralizationDecision =
  | "generalize"
  | "keep_as_is"
  | "split_general_and_specific";

/** 抽出のやり直しで選べる方針。 */
export type PatternExtractionStyle =
  | "standard"
  | "more_general"
  | "exam_pattern_focused"
  | "custom";

/** 1=principle / 2=strategy / 3=technique / 4=specialized */
export type PatternSpecificityLevel = 1 | 2 | 3 | 4;

export interface PatternParentHint {
  title: string;
  reason: string;
}

export interface PatternProposalStrategy {
  title: string;
  description: string;
  condition: string;
  reasoning: string;
  sortOrder: number;
}

export interface PatternProposal {
  proposalId: string;
  /** 元Problemで実際に使われた具体的手法。固有名・数値を含んでよい。 */
  rawTechnique: string;
  title: string;
  patternType: PatternType;
  summary: string;
  situation: string;
  principle: string;
  strategies: PatternProposalStrategy[];
  cautions: string[];
  domains: string[];
  goals: string[];
  operations: string[];
  structures: string[];
  situations: string[];
  tags: string[];
  sourceType: PatternProposalSourceType;
  matchedPatternId: number | null;
  matchedPatternTitle: string | null;
  similarityReason: string;
  actionRecommendation: PatternProposalRecommendation;
  /** rawTechniqueから何を取り除いて一般化したか。 */
  generalizationReason: string;
  specificityLevel: PatternSpecificityLevel;
  /** 0.0〜1.0。他の問題へ再利用できる度合いのAI自己評価。 */
  reusabilityScore: number;
  /** 既存定石検索へ使う一般化されたキーワード。 */
  searchConcepts: string[];
  isOverlySpecific: boolean;
  isOverlyGeneral: boolean;
  specificityReason: string;
  possibleParentPattern: PatternParentHint | null;
  generalizationDecision: PatternGeneralizationDecision;
  recommendedStorage: PatternProposalStorage;
  /** 自動・手動を通じて一般化し直した回数。 */
  generalizationPassCount: number;
}

export interface AiPatternExtractionResult {
  schemaVersion: 1;
  kind: "pattern-extraction";
  patterns: PatternProposal[];
  /** Generic AI-job views use these optional compatibility fields. */
  plainText?: string;
  problems?: AiExtractedProblem[];
  requiredPackages?: string[];
  detectedType?: string;
}

export interface ApplyPatternProposalPayload {
  /** 抽出元Problem。画像・AI Chat・手動由来では省略する。 */
  problemId?: number;
  /** 画像ファイル名など、由来を1行で残すための参照。 */
  sourceReference?: string;
  proposal: PatternProposal;
  action:
    | "create_new"
    | "create_child_pattern"
    | "merge_into_existing"
    | "add_candidate_to_existing"
    | "add_caution_to_existing"
    | "add_example_to_existing"
    | "link_existing";
  targetPatternId?: number | null;
  linkRelationType?: ProblemPatternRelationType | null;
  /** create_child_patternのときだけ使う上位定石。 */
  parentPatternId?: number | null;
}

export interface ApplyPatternProposalResult {
  patternId: number;
  action: string;
  created: boolean;
  linked: boolean;
}

export interface PatternSummary {
  id: number;
  uuid: string;
  title: string;
  summary: string;
  pattern_type: PatternType;
  tags: string[];
  facets: PatternFacets;
  updated_at: string;
  version: number;
  strategy_count: number;
  problem_count: number;
}

export interface PatternRelationView {
  from_pattern_id: number;
  to_pattern_id: number;
  pattern_id: number;
  title: string;
  pattern_type: PatternType;
  relation_type: PatternRelationType;
  direction: "incoming" | "outgoing";
}

export interface PatternProblemView {
  problem_id: number;
  title: string;
  bank_node_id: number;
  bank_path: string;
  relation_type: ProblemPatternRelationType;
}

export interface ProblemPatternView {
  pattern_id: number;
  title: string;
  summary: string;
  pattern_type: PatternType;
  relation_type: ProblemPatternRelationType;
  tags: string[];
}

export interface PatternFull {
  id: number;
  uuid: string;
  title: string;
  summary: string;
  pattern_type: PatternType;
  situation: string;
  principle: string;
  cautions: string;
  examples: string;
  source_note: string;
  tags: string[];
  facets: PatternFacets;
  strategies: PatternStrategy[];
  related_patterns: PatternRelationView[];
  related_problems: PatternProblemView[];
  created_at: string;
  updated_at: string;
  version: number;
}

export interface PatternUpdate {
  id: number;
  expected_version?: number | null;
  title: string;
  summary: string;
  pattern_type: PatternType;
  situation: string;
  principle: string;
  cautions: string;
  examples: string;
  source_note: string;
  tags: string[];
  facets: PatternFacets;
  strategies: PatternStrategyInput[];
}

export interface PatternSearchQuery {
  text: string;
  pattern_type?: string | null;
  tag?: string | null;
  domain?: string | null;
  goal?: string | null;
  operation?: string | null;
  structure?: string | null;
  situation?: string | null;
  exclude_id?: number | null;
  limit?: number | null;
  offset?: number | null;
}

export interface PatternFilterValues {
  pattern_types: string[];
  tags: string[];
  domains: string[];
  goals: string[];
  operations: string[];
  structures: string[];
  situations: string[];
}

export interface PatternDeleteImpact {
  pattern_id: number;
  problem_count: number;
  related_pattern_count: number;
}

export interface PatternSnapshot {
  /** Snapshot取得時のcanonical Pattern版。旧snapshotは0として扱う。 */
  version: number;
  uuid: string;
  title: string;
  summary: string;
  pattern_type: PatternType;
  situation: string;
  principle: string;
  cautions: string;
  examples: string;
  source_note: string;
  tags: string[];
  facets: PatternFacets;
  strategies: PatternStrategyInput[];
}

export interface PatternVersionSummary {
  id: number;
  pattern_id: number;
  title: string;
  version: number;
  saved_at: string;
}

export interface PatternVersionFull {
  id: number;
  pattern_id: number;
  version: number;
  saved_at: string;
  snapshot: PatternSnapshot;
}

export interface ImportPatternsResult {
  created: number;
  skipped: number;
  relations_created: number;
  problem_relations_created: number;
}

export interface ProjectSummary {
  id: number;
  name: string;
  description: string;
  updated_at: string;
  item_count: number;
  version: number;
}

export interface SnapAttachment {
  file_name: string;
  stored_name: string;
}

export type ItemType = "problem" | "heading" | "text" | "pagebreak" | "part" | "pattern";
export type DifficultyRank = "A" | "B" | "C" | "D";
export type RequiredFilter = "all" | "required" | "not_required";
export type PartOutputTarget = "problems" | "answers" | "both" | "none";
export type PartLayoutMode = "single_column" | "two_column";

export interface ProjectItem {
  id: number;
  project_id: number;
  item_type: ItemType;
  sort_order: number;
  problem_id: number | null;
  part_id: number | null;
  snap_title: string;
  snap_statement: string;
  snap_statement_two_column: string;
  snap_answer: string;
  snap_explanation: string;
  snap_difficulty: string;
  snap_difficulty_rank: DifficultyRank | null;
  snap_is_required: boolean;
  snap_attachments: SnapAttachment[];
  content: string;
  snap_part_type: PartType | string;
  snap_part_category: string;
  snap_part_description: string;
  snap_part_output_target: PartOutputTarget;
  snap_part_layout_mode: PartLayoutMode;
  snap_part_attachments: SnapAttachment[];
  /** 見出しのレベル: 1=章(section), 2=節(subsection) */
  heading_level: number;
  /** この見出しに番号を振るか（全体設定がONのときのみ有効） */
  heading_numbered: boolean;
  bank_updated: boolean;
  source_exists: boolean;
  part_updated: boolean;
  /** 定石項目の元Pattern（削除済みならnull） */
  pattern_id: number | null;
  /** 追加時点のPatternSnapshotのJSON。教材側の正本はこちら */
  snap_pattern_json: string;
  /** 定石ライブラリ側に新しい版があるか */
  pattern_updated: boolean;
  /** 楽観的ロック用バージョン */
  version: number;
}

export interface ProjectSettings {
  booklet_title: string;
  subtitle: string;
  target: string;
  date_str: string;
  header_left: string;
  header_right: string;
  number_format: string;
  show_name_field: boolean;
  auto_number: boolean;
  page_break_per_problem: boolean;
  include_explanation: boolean;
  /** 問題冊子（合本の問題編を含む）を縦線付き二段組にする */
  problem_two_column: boolean;
  /** 解答冊子の2段組: "none" | "all"（問題＋解答全体） | "answer_only"（解答部分のみ） */
  two_column_mode: string;
  show_title: boolean;
  show_header: boolean;
  show_toc: boolean;
  number_headings: boolean;
  include_statement_in_answers: boolean;
  box_statement_in_answers: boolean;
  /** 章ごとに問題番号をリセットする（番号付き章では 2-1 形式） */
  reset_numbering_per_chapter: boolean;
  difficulty_display: "none" | "number_side" | "top_right";
  required_display: "none" | "required_only";
}

/** 冊子の種類 */
export type BookletKind = "problems" | "answers" | "combined";

export interface ProjectFull {
  id: number;
  version: number;
  name: string;
  description: string;
  created_at: string;
  updated_at: string;
  settings: ProjectSettings;
  items: ProjectItem[];
  template_id: number | null;
  template_name: string;
  template_updated: boolean;
}

export interface TemplateSummary {
  id: number;
  name: string;
  description: string;
  compile_method: string;
  updated_at: string;
  usage_count: number;
}

export interface TemplateAsset {
  id: number;
  template_id: number;
  file_name: string;
  stored_name: string;
}

export interface TemplateFull {
  id: number;
  version: number;
  name: string;
  description: string;
  base_template: string;
  problem_template: string;
  answer_template: string;
  compile_method: string;
  packages_memo: string;
  created_at: string;
  updated_at: string;
  assets: TemplateAsset[];
  warnings: string[];
}

export interface TemplateVersionSummary {
  id: number;
  name: string;
  saved_at: string;
}

export interface ImportBankResult {
  nodes_created?: number;
  subjects_created: number;
  fields_created: number;
  units_created: number;
  problems_imported: number;
}

export interface ImportAnalysis {
  doc_class: string;
  packages: string[];
  has_body_placeholder: boolean;
  has_markers: boolean;
  has_document_env: boolean;
  referenced_files: string[];
  content: string;
}

export interface CompileResult {
  success: boolean;
  pdf_path: string | null;
  tex_path: string | null;
  log: string;
  message: string;
}

export interface TexDetection {
  uplatex_path: string | null;
  dvipdfmx_path: string | null;
}

export type PartType =
  | "heading"
  | "text"
  | "notice"
  | "hint"
  | "example"
  | "homework"
  | "reflection"
  | "box"
  | "table"
  | "image_block"
  | "latex_snippet"
  | "page_break"
  | "custom";

export interface PartAttachment {
  id: number;
  part_id: number;
  file_name: string;
  stored_name: string;
  created_at: string;
}

export interface PartSummary {
  id: number;
  bank_node_id: number | null;
  bank_path: string;
  unit_id: number | null;
  unit_name: string;
  field_id: number | null;
  field_name: string;
  subject_id: number | null;
  subject_name: string;
  title: string;
  part_type: PartType | string;
  category: string;
  tags: string[];
  plain_text_preview: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  output_target: PartOutputTarget;
  layout_mode: PartLayoutMode;
  usage_count: number;
  updated_at: string;
  version: number;
}

export interface PartFull extends PartSummary {
  latex_source: string;
  description: string;
  created_at: string;
  attachments: PartAttachment[];
}

export interface PartSearchQuery {
  text: string;
  bank_node_id?: number | null;
  include_descendants?: boolean | null;
  subject_id?: number | null;
  field_id?: number | null;
  unit_id?: number | null;
  part_type?: string | null;
  category?: string | null;
  tag?: string | null;
  difficulty_rank?: DifficultyRank | null;
  difficulty_ranks?: (DifficultyRank | "__unset")[] | null;
  required_filter?: RequiredFilter | null;
  unassigned_only?: boolean | null;
}

export interface PartVersionSummary {
  id: number;
  title: string;
  version: number;
  saved_at: string;
}

export interface GraphIntegrationStartPayload {
  projectId?: number | null;
  problemId?: number | null;
  itemId?: number | null;
  insertTarget: string;
  selectionStart?: number | null;
  selectionEnd?: number | null;
  reeditAssetId?: string | null;
}

export interface GraphIntegrationSession {
  requestId: string;
  requestPath: string;
  returnFolder: string;
  graphAppPath: string;
  launched: boolean;
  message: string;
}

export interface GraphIntegrationPoll {
  status: "pending" | "completed" | "cancelled" | "failed";
  requestId: string;
  assetId: string | null;
  graphId: string | null;
  displayName: string | null;
  insertedLatex: string | null;
  message: string;
  details: string | null;
}

export interface GraphAssetSummary {
  assetId: string;
  graphId: string;
  displayName: string;
  projectId: number | null;
  problemId: number | null;
  itemId: number | null;
  sourceApplication: string;
  editableSourcePath: string;
  primaryAssetPath: string;
  previewAssetPath: string;
  latexSourcePath: string;
  insertedLatex: string;
  createdAt: string;
  updatedAt: string;
  version: number;
}

export interface GraphIntegrationTestResult {
  ok: boolean;
  path: string | null;
  message: string;
}

export interface CreateGraphWebSessionPayload {
  projectId?: number | null;
  problemId?: number | null;
  itemId?: number | null;
  targetField: string;
  selectionStart?: number | null;
  selectionEnd?: number | null;
}

export interface GraphWebSession {
  sessionId: string;
  status: "pending" | "completed" | "cancelled" | "expired";
  projectId: number | null;
  problemId: number | null;
  itemId: number | null;
  targetField: string;
  selectionStart: number;
  selectionEnd: number;
  expectedTargetVersion: number;
  graphId: string;
  assetId: string;
  insertedLatex: string;
  createdAt: string;
  expiresAt: number;
}

export interface CompleteGraphWebSessionResult {
  session: GraphWebSession;
  assetId: string;
  insertedLatex: string;
}

// ---- 共通サーバー上のグラフ正本（MathGraph PDF Studio Project JSON） ----
export interface StoredGraphSummary {
  id: string;
  title: string;
  graphType: "function_graph" | "geometry" | "mixed" | "spatial_geometry";
  sourceType: "manual" | "ai_text" | "ai_image" | "ai_problem" | "import";
  warnings: string[];
  thumbnailPath: string;
  createdAt: string;
  updatedAt: string;
  version: number;
  usageCount: number;
  savedFormats: Array<"pdf" | "png" | "svg" | "tex" | "json">;
  exportsCurrent: boolean;
}

export interface StoredGraph extends StoredGraphSummary {
  graphJson: string;
}

export interface GraphVersionSummary {
  id: number;
  graphId: string;
  title: string;
  version: number;
  savedAt: string;
}

export interface GraphVersionFull extends GraphVersionSummary {
  graphJson: string;
  graphType: StoredGraphSummary["graphType"];
  sourceType: StoredGraphSummary["sourceType"];
  warnings: string[];
}

export interface CreateStoredGraphPayload {
  title: string;
  graphJson: string;
  graphType?: StoredGraphSummary["graphType"];
  sourceType?: StoredGraphSummary["sourceType"];
  warnings?: string[];
}

export interface UpdateStoredGraphPayload extends CreateStoredGraphPayload {
  id: string;
  expectedVersion?: number | null;
}

// ---- 教材サーバー ----

export interface WebDevice {
  id: number;
  deviceName: string;
  userAgent: string;
  createdAt: string;
  lastSeenAt: string;
  revoked: boolean;
}

export interface ServerStatus {
  running: boolean;
  port: number;
  lanMode: boolean;
  localUrl: string;
  pairingCode: string | null;
  activeSessions: number;
  devices: WebDevice[];
  log: string[];
}

export interface TailscaleStatus {
  installed: boolean;
  message?: string;
  version?: string;
  backendState?: string;
  connected?: boolean;
  dnsName?: string;
  httpsUrl?: string;
  serveConfigured?: boolean;
  serveStatus?: string;
  suggestedCommand?: string;
}

// ---- Codex ----

export interface CodexLoginState {
  loginId: string;
  method: string;
  userCode: string | null;
  verificationUrl: string | null;
  authUrl: string | null;
  status: "pending" | "success" | "failed";
  error: string | null;
}

export interface CodexAccount {
  account: {
    type: string;
    email?: string | null;
    planType?: string;
  } | null;
  requiresOpenaiAuth: boolean;
}

export interface CodexStatus {
  installed: boolean;
  exePath: string;
  version: string;
  running: boolean;
  account: CodexAccount | null;
  rateLimits: unknown;
  login: CodexLoginState | null;
  selectedModel: string;
  lastError: string | null;
  log: string[];
}

export interface CodexModelInfo {
  model: string;
  displayName: string;
  description: string;
  isDefault: boolean;
}

export interface CodexModelSettings {
  selectedModel: string;
  models: CodexModelInfo[];
}

// ---- AI変換 ----

export type AiJobStatus =
  | "queued"
  | "preprocessing"
  | "waiting_for_codex"
  | "converting"
  | "validating"
  | "compiling"
  | "completed"
  | "failed"
  | "cancelled";

export interface AiWarning {
  code: string;
  severity: "info" | "warning" | "error";
  message: string;
}

export interface AiUncertainFragment {
  id: string;
  description: string;
  candidates: string[];
}

export interface AiSegment {
  order: number;
  kind: string;
  latex: string;
}

export interface AiExtractedProblem {
  title: string;
  /** 一段組用の問題文 */
  statementLatex: string;
  /** 二段組の片方の列用の問題文 */
  statementLatexTwoColumn: string;
  sourceImageIndexes: number[];
}

export interface AiStructuredResult {
  schemaVersion: number;
  detectedType: string;
  latex: string;
  plainText: string;
  requiredPackages: string[];
  warnings: AiWarning[];
  uncertainFragments: AiUncertainFragment[];
  segments: AiSegment[];
  suggestedInsertTarget: string;
  /** 問題バンク取込モードで抽出された、独立保存可能な問題。 */
  problems?: AiExtractedProblem[];
}

export interface AiSolutionWorkflowResult {
  schemaVersion: 1;
  kind:
    | "solution-strategies"
    | "strategy-validation"
    | "solution-plan"
    | "solution-common-flow"
    | "solution-flow"
    | "solution-flow-from-answer"
    | "solution-verification";
  analysis?: ProblemAnalysis;
  strategies?: SolutionStrategy[];
  validation?: StrategyValidationResult;
  plan?: SolutionPlan;
  flow?: SolutionFlowBlock[];
  commonFlow?: SolutionFlowBlock[];
  variantFlows?: Array<{ variantId: string; flow: SolutionFlowBlock[] }>;
  verification?: VerificationResult;
  /** Generic AI-job views use these optional compatibility fields. */
  plainText?: string;
  problems?: AiExtractedProblem[];
  requiredPackages?: string[];
  detectedType?: string;
}

export interface AiGraphSpec {
  schemaVersion: 1;
  detectedType: "function_graph" | "mixed" | "unknown";
  title: string;
  expressions: Array<{ id: string; expression: string; style: { lineType: "solid" | "dashed"; lineWidth: number; color: string } }>;
  viewport: { xMin: number; xMax: number; yMin: number; yMax: number };
  axes: { showX: boolean; showY: boolean; showGrid: boolean };
  points: Array<{ id: string; x: number; y: number; label: string }>;
  lines: unknown[];
  regions: unknown[];
  labels: Array<{ id: string; latex: string; x: number; y: number }>;
  warnings: AiWarning[];
  uncertainFragments: AiUncertainFragment[];
}

export interface AiGraphStructuredResult extends AiStructuredResult {
  kind: "graph";
  graphProject: Record<string, unknown>;
  graphSpec: AiGraphSpec;
}

export interface AiSpatialSpec {
  schemaVersion: 1;
  detectedType: "solid_geometry" | "mixed" | "unknown";
  title: string;
  projection: { type: "orthographic" | "perspective" };
  solids: Array<{ id: string; type: "cube" | "cuboid" | "prism" | "pyramid" | "cylinder" | "cone" | "sphere"; name: string; size: [number, number, number]; position: [number, number, number]; rotation: [number, number, number]; vertexNames: string[] }>;
  segments: Array<{ id: string; name: string; from: [number, number, number]; to: [number, number, number]; lineType: "solid" | "dashed" }>;
  points: Array<{ id: string; position: [number, number, number]; label: string }>;
  labels: Array<{ id: string; text: string; position: [number, number, number] }>;
  warnings: AiWarning[];
  uncertainFragments: AiUncertainFragment[];
}

export interface AiSpatialStructuredResult extends AiStructuredResult {
  kind: "spatial-geometry";
  spatialDocument: Record<string, unknown>;
  spatialSpec: AiSpatialSpec;
}

export interface AiJob {
  id: number;
  jobUuid: string;
  sourceType: "image" | "text";
  conversionMode: string;
  options: Record<string, unknown>;
  status: AiJobStatus;
  progressMessage: string;
  inputText: string;
  inputAssetPaths: string[];
  outputLatex: string;
  structuredResult: AiStructuredResult | AiGraphStructuredResult | AiSpatialStructuredResult | AiSolutionWorkflowResult | AiPatternExtractionResult | null;
  warnings: AiWarning[];
  uncertainFragments: AiUncertainFragment[];
  compileStatus: "none" | "ok" | "failed" | "skipped";
  compileLog: string;
  previewPdfPath: string;
  targetEntityType: string;
  targetEntityId: number | null;
  /** 現在の問題・部品等から解決した表示名。 */
  targetEntityName: string;
  targetField: string;
  /** エディタへの挿入、問題・部品への保存、修正反映を行った日時。 */
  insertedAt: string;
  errorCode: string;
  errorMessage: string;
  createdAt: string;
  updatedAt: string;
  completedAt: string;
}

// ---- AIチャット / 既存機能Tool Agent ----

export type AiChatStatus = "idle" | "running" | "cancelling" | "awaiting_confirmation" | "failed";
export type AiChatExecutionMode = "suggest" | "confirm" | "auto";

export interface AiChatAttachment {
  name: string;
  stored_name: string;
}

export interface AiChatToolCall {
  call_id: string;
  name: string;
  arguments: Record<string, unknown>;
}

export interface AiChatMessage {
  id: number;
  sessionId: string;
  role: "user" | "assistant" | "tool";
  content: string;
  attachments: AiChatAttachment[];
  metadata: Record<string, unknown>;
  status: "queued" | "running" | "completed" | "failed" | "cancelled" | "awaiting_confirmation";
  createdAt: string;
}

export interface AiChatSession {
  id: string;
  title: string;
  status: AiChatStatus;
  executionMode: AiChatExecutionMode;
  context: Record<string, unknown>;
  pendingCalls: AiChatToolCall[];
  lastError: string;
  createdAt: string;
  updatedAt: string;
  messages: AiChatMessage[];
}
