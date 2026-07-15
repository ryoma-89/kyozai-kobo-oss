export interface UnitNode {
  id: number;
  name: string;
  sort_order: number;
  problem_count: number;
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

export type Difficulty = "基礎" | "標準" | "発展";

export interface ProblemSummary {
  id: number;
  unit_id: number;
  title: string;
  difficulty: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  tags: string[];
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
  unit_id: number;
  title: string;
  statement_latex: string;
  answer_latex: string;
  explanation_latex: string;
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
  answer_latex: string;
  explanation_latex: string;
  difficulty: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  memo: string;
  saved_at: string;
}

export interface SearchQuery {
  text: string;
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
  tags: string[];
  updated_at: string;
  usage_count: number;
  subject_name: string;
  field_name: string;
  unit_name: string;
  unit_id: number;
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

export type ItemType = "problem" | "heading" | "text" | "pagebreak" | "part";
export type DifficultyRank = "A" | "B" | "C" | "D";
export type RequiredFilter = "all" | "required" | "not_required";
export type PartOutputTarget = "problems" | "answers" | "both" | "none";

export interface ProjectItem {
  id: number;
  project_id: number;
  item_type: ItemType;
  sort_order: number;
  problem_id: number | null;
  part_id: number | null;
  snap_title: string;
  snap_statement: string;
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
  snap_part_attachments: SnapAttachment[];
  /** 見出しのレベル: 1=章(section), 2=節(subsection) */
  heading_level: number;
  /** この見出しに番号を振るか（全体設定がONのときのみ有効） */
  heading_numbered: boolean;
  bank_updated: boolean;
  source_exists: boolean;
  part_updated: boolean;
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
  title: string;
  part_type: PartType | string;
  category: string;
  tags: string[];
  plain_text_preview: string;
  difficulty_rank: DifficultyRank | null;
  is_required: boolean;
  output_target: PartOutputTarget;
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
  part_type?: string | null;
  category?: string | null;
  tag?: string | null;
  difficulty_rank?: DifficultyRank | null;
  difficulty_ranks?: (DifficultyRank | "__unset")[] | null;
  required_filter?: RequiredFilter | null;
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
  lastError: string | null;
  log: string[];
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
  structuredResult: AiStructuredResult | AiGraphStructuredResult | AiSpatialStructuredResult | null;
  warnings: AiWarning[];
  uncertainFragments: AiUncertainFragment[];
  compileStatus: "none" | "ok" | "failed" | "skipped";
  compileLog: string;
  previewPdfPath: string;
  targetEntityType: string;
  targetEntityId: number | null;
  targetField: string;
  errorCode: string;
  errorMessage: string;
  createdAt: string;
  updatedAt: string;
  completedAt: string;
}
