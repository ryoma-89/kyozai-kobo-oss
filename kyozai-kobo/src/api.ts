import { invoke, invokeTauriOnly } from "./transport";
import type {
  AiExtractedProblem,
  AiJob,
  Attachment,
  BookletKind,
  CodexStatus,
  CodexModelSettings,
  CompileResult,
  GraphAssetSummary,
  GraphIntegrationPoll,
  GraphIntegrationSession,
  GraphIntegrationStartPayload,
  GraphIntegrationTestResult,
  GraphVersionFull,
  GraphVersionSummary,
  CreateStoredGraphPayload,
  CreateGraphWebSessionPayload,
  CompleteGraphWebSessionResult,
  ImportAnalysis,
  ImportBankResult,
  BankNode,
  BankNodeDeleteImpact,
  BankNodeDeleteStrategy,
  NodeKind,
  PartAttachment,
  PartFull,
  PartSearchQuery,
  PartSummary,
  PartVersionSummary,
  PatternDeleteImpact,
  ApplyPatternProposalPayload,
  ApplyPatternProposalResult,
  PatternExtractionStyle,
  PatternProposal,
  PatternFilterValues,
  PatternFull,
  PatternSearchQuery,
  PatternSummary,
  PatternUpdate,
  PatternVersionFull,
  PatternVersionSummary,
  ProblemPatternView,
  ImportPatternsResult,
  ProblemFull,
  ProblemSolutionVariant,
  ProblemSummary,
  ProjectFull,
  ProjectSettings,
  ProjectSummary,
  SearchQuery,
  SearchResult,
  ServerStatus,
  StoredGraph,
  StoredGraphSummary,
  SubjectNode,
  TailscaleStatus,
  TemplateAsset,
  TemplateFull,
  TemplateSummary,
  TemplateVersionSummary,
  TexDetection,
  VersionFull,
  VersionSummary,
  UpdateStoredGraphPayload,
  GraphWebSession,
} from "./types";

// ---- ツリー ----
export const getTree = () => invoke<SubjectNode[]>("get_tree");
export const addTreeNode = (kind: NodeKind, parentId: number | null, name: string) =>
  invoke<number>("add_tree_node", { kind, parentId, name });
export const renameTreeNode = (kind: NodeKind, id: number, name: string) =>
  invoke<void>("rename_tree_node", { kind, id, name });
export const deleteTreeNode = (kind: NodeKind, id: number) =>
  invoke<void>("delete_tree_node", { kind, id });
export const moveTreeNode = (kind: NodeKind, id: number, delta: number) =>
  invoke<void>("move_tree_node", { kind, id, delta });

// 任意深度の問題バンク正本。上の旧APIはParts・旧クライアント互換用。
export const getBankTree = () => invoke<BankNode[]>("get_bank_tree");
export const createBankNode = (parentId: number | null, name: string) =>
  invoke<number>("create_bank_node", { parentId, name });
export const renameBankNode = (id: number, name: string) =>
  invoke<void>("rename_bank_node", { id, name });
export const moveBankNode = (id: number, newParentId: number | null, sortOrder?: number | null) =>
  invoke<void>("move_bank_node", { id, newParentId, sortOrder: sortOrder ?? null });
export const reorderBankNode = (id: number, delta: number) =>
  invoke<void>("reorder_bank_node", { id, delta });
export const getBankNodeDeleteImpact = (id: number) =>
  invoke<BankNodeDeleteImpact>("get_bank_node_delete_impact", { id });
export const deleteBankNode = (id: number, strategy: BankNodeDeleteStrategy) =>
  invoke<void>("delete_bank_node", { id, strategy });

// ---- 問題 ----
export const listProblems = (bankNodeId: number) =>
  invoke<ProblemSummary[]>("list_problems", { bankNodeId });
export const getProblem = (id: number) => invoke<ProblemFull>("get_problem", { id });
export const createProblem = (bankNodeId: number, title: string) =>
  invoke<number>("create_problem", { bankNodeId, title });
/** 保存に成功すると新しいversionを返す。競合時は ConflictError */
export const updateProblem = (payload: {
  id: number;
  bank_node_id: number;
  unit_id: number;
  title: string;
  statement_latex: string;
  statement_latex_two_column: string;
  answer_latex: string;
  explanation_latex: string;
  solution_variants: ProblemSolutionVariant[];
  answer_completed: boolean;
  explanation_completed: boolean;
  difficulty: string;
  difficulty_rank: string | null;
  is_required: boolean;
  memo: string;
  tags: string[];
  expected_version?: number | null;
}) => invoke<number>("update_problem", { payload });
export const duplicateProblem = (id: number) => invoke<number>("duplicate_problem", { id });
export const deleteProblem = (id: number) => invoke<void>("delete_problem", { id });
export const listVersions = (problemId: number) =>
  invoke<VersionSummary[]>("list_versions", { problemId });
export const getVersion = (versionId: number) => invoke<VersionFull>("get_version", { versionId });
export const restoreVersion = (versionId: number) =>
  invoke<void>("restore_version", { versionId });
export const listAllTags = () => invoke<string[]>("list_all_tags");
export const searchProblems = (query: SearchQuery) =>
  invoke<SearchResult[]>("search_problems", { query });

// ---- 定石ライブラリ ----
export const searchPatterns = (query: PatternSearchQuery) =>
  invoke<PatternSummary[]>("search_patterns", { query });
export const listPatternFilterValues = () =>
  invoke<PatternFilterValues>("list_pattern_filter_values");
export const createPattern = (title: string, patternType = "strategy") =>
  invoke<number>("create_pattern", { title, patternType });
export const getPattern = (id: number) => invoke<PatternFull>("get_pattern", { id });
export const updatePattern = (payload: PatternUpdate) =>
  invoke<number>("update_pattern", { payload });
export const duplicatePattern = (id: number) => invoke<number>("duplicate_pattern", { id });
export const getPatternDeleteImpact = (patternId: number) =>
  invoke<PatternDeleteImpact>("get_pattern_delete_impact", { patternId });
export const deletePattern = (id: number) => invoke<void>("delete_pattern", { id });
export const listPatternsForProblem = (problemId: number) =>
  invoke<ProblemPatternView[]>("list_patterns_for_problem", { problemId });
export const linkProblemPattern = (
  problemId: number,
  patternId: number,
  relationType: string,
) => invoke<void>("link_problem_pattern", { problemId, patternId, relationType });
export const unlinkProblemPattern = (problemId: number, patternId: number) =>
  invoke<void>("unlink_problem_pattern", { problemId, patternId });
export const linkPatternRelation = (
  fromPatternId: number,
  toPatternId: number,
  relationType = "related",
) => invoke<void>("link_pattern_relation", { fromPatternId, toPatternId, relationType });
export const unlinkPatternRelation = (
  fromPatternId: number,
  toPatternId: number,
  relationType: string,
) => invoke<void>("unlink_pattern_relation", { fromPatternId, toPatternId, relationType });
export const listPatternVersions = (patternId: number) =>
  invoke<PatternVersionSummary[]>("list_pattern_versions", { patternId });
export const getPatternVersion = (versionId: number) =>
  invoke<PatternVersionFull>("get_pattern_version", { versionId });
export const restorePatternVersion = (
  versionId: number,
  expectedVersion?: number | null,
) => invoke<number>("restore_pattern_version", { versionId, expectedVersion: expectedVersion ?? null });
export const exportPatternsJson = (patternIds?: number[] | null) =>
  invoke<string>("export_patterns_json", { patternIds: patternIds ?? null });
export const exportPatternsFile = (patternIds: number[] | null, destPath: string) =>
  invoke<void>("export_patterns_file", { patternIds, destPath });
export const importPatternsJson = (jsonText: string) =>
  invoke<ImportPatternsResult>("import_patterns_json", { jsonText });
export const importPatternsFile = (path: string) =>
  invoke<ImportPatternsResult>("import_patterns_file", { path });
export const startPatternExtraction = (
  problemId: number,
  style?: PatternExtractionStyle,
  instruction?: string,
) =>
  invoke<AiJob>("start_pattern_extraction", {
    problemId,
    style: style ?? null,
    instruction: instruction ?? null,
  });
export const patternCardLatex = (patternId: number) =>
  invoke<string>("pattern_card_latex", { patternId });
export const startPatternEdit = (patternId: number, instruction: string) =>
  invoke<AiJob>("start_pattern_edit", { patternId, instruction });
export const applyPatternEdit = (
  patternId: number,
  expectedVersion: number | null,
  proposal: PatternProposal,
) => invoke<number>("apply_pattern_edit", { patternId, expectedVersion, proposal });
export const startPatternImageImport = (inputNames: string[], note?: string) =>
  invoke<AiJob>("start_pattern_image_import", { inputNames, note: note ?? null });
export const startPatternGeneralization = (problemId: number, proposal: PatternProposal) =>
  invoke<AiJob>("start_pattern_generalization", { problemId, proposal });
export const applyPatternProposal = (payload: ApplyPatternProposalPayload) =>
  invoke<ApplyPatternProposalResult>("apply_pattern_proposal", { payload });

// ---- 教材プロジェクト ----
export const listProjects = () => invoke<ProjectSummary[]>("list_projects");
export const createProject = (name: string, templateId?: number | null) =>
  invoke<number>("create_project", { name, templateId: templateId ?? null });
export const setProjectTemplate = (projectId: number, templateId: number) =>
  invoke<void>("set_project_template", { projectId, templateId });
export const refreshProjectTemplate = (projectId: number) =>
  invoke<void>("refresh_project_template", { projectId });
export const updateProjectMeta = (
  id: number,
  name: string,
  description: string,
  expectedVersion?: number | null,
) => invoke<number>("update_project_meta", { id, name, description, expectedVersion: expectedVersion ?? null });
export const deleteProject = (id: number) => invoke<void>("delete_project", { id });
export const duplicateProject = (id: number) => invoke<number>("duplicate_project", { id });
export const getProject = (id: number) => invoke<ProjectFull>("get_project", { id });
export const addProblemToProject = (projectId: number, problemId: number) =>
  invoke<number>("add_problem_to_project", { projectId, problemId });
export const addPatternToProject = (projectId: number, patternId: number) =>
  invoke<number>("add_pattern_to_project", { projectId, patternId });
export const refreshPatternItemFromLibrary = (itemId: number) =>
  invoke<void>("refresh_pattern_item_from_library", { itemId });
export const addPartToProject = (projectId: number, partId: number) =>
  invoke<number>("add_part_to_project", { projectId, partId });
export const addContentItem = (
  projectId: number,
  itemType: string,
  content: string,
  headingLevel?: number,
) => invoke<number>("add_content_item", { projectId, itemType, content, headingLevel: headingLevel ?? 1 });
/** 保存に成功すると新しいversionを返す。競合時は ConflictError */
export const updateProjectItem = (
  itemId: number,
  fields: {
    content?: string;
    snap_title?: string;
    snap_statement?: string;
    snap_statement_two_column?: string;
    snap_answer?: string;
    snap_explanation?: string;
    snap_difficulty_rank?: string | null;
    snap_is_required?: boolean;
    snap_part_type?: string;
    snap_part_category?: string;
    snap_part_description?: string;
    snap_part_output_target?: string;
    snap_part_layout_mode?: string;
    heading_level?: number;
    heading_numbered?: boolean;
    expected_version?: number | null;
  },
) =>
  invoke<number>("update_project_item", {
    itemId,
    content: fields.content ?? null,
    snapTitle: fields.snap_title ?? null,
    snapStatement: fields.snap_statement ?? null,
    snapStatementTwoColumn: fields.snap_statement_two_column ?? null,
    snapAnswer: fields.snap_answer ?? null,
    snapExplanation: fields.snap_explanation ?? null,
    snapDifficultyRank: fields.snap_difficulty_rank ?? null,
    snapIsRequired: fields.snap_is_required ?? null,
    snapPartType: fields.snap_part_type ?? null,
    snapPartCategory: fields.snap_part_category ?? null,
    snapPartDescription: fields.snap_part_description ?? null,
    snapPartOutputTarget: fields.snap_part_output_target ?? null,
    snapPartLayoutMode: fields.snap_part_layout_mode ?? null,
    headingLevel: fields.heading_level ?? null,
    headingNumbered: fields.heading_numbered ?? null,
    expectedVersion: fields.expected_version ?? null,
  });
export const refreshItemFromBank = (itemId: number) =>
  invoke<void>("refresh_item_from_bank", { itemId });
export const refreshPartItemFromLibrary = (itemId: number) =>
  invoke<void>("refresh_part_item_from_library", { itemId });
export const removeProjectItem = (itemId: number) =>
  invoke<void>("remove_project_item", { itemId });
export const reorderProjectItems = (projectId: number, orderedIds: number[]) =>
  invoke<void>("reorder_project_items", { projectId, orderedIds });
export const updateProjectSettings = (
  projectId: number,
  settings: ProjectSettings,
  expectedVersion?: number | null,
) => invoke<number>("update_project_settings", { projectId, settings, expectedVersion: expectedVersion ?? null });

// ---- LaTeX ----
export const generateTex = (projectId: number, kind: BookletKind) =>
  invoke<string>("generate_tex", { projectId, kind });
export const exportTex = (projectId: number, kind: BookletKind) =>
  invoke<string>("export_tex", { projectId, kind });
export const compilePdf = (projectId: number, kind: BookletKind) =>
  invoke<CompileResult>("compile_pdf", { projectId, kind });
export const detectTex = () => invoke<TexDetection>("detect_tex");
export const openPath = (path: string) => invokeTauriOnly<void>("open_path", { path });
export const showInFolder = (path: string) => invokeTauriOnly<void>("show_in_folder", { path });

// ---- Graph integration（デスクトップのみ） ----
export const detectGraphAppPath = () => invoke<string | null>("detect_graph_app_path");
export const testGraphIntegrationSettings = () =>
  invoke<GraphIntegrationTestResult>("test_graph_integration_settings");
export const startGraphIntegration = (payload: GraphIntegrationStartPayload) =>
  invoke<GraphIntegrationSession>("start_graph_integration", { payload });
export const pollGraphIntegration = (requestId: string, requestPath: string) =>
  invoke<GraphIntegrationPoll>("poll_graph_integration", { requestId, requestPath });
export const listGraphAssets = (projectId?: number | null, problemId?: number | null) =>
  invoke<GraphAssetSummary[]>("list_graph_assets", {
    projectId: projectId ?? null,
    problemId: problemId ?? null,
  });

// ---- グラフ正本・Web編集（デスクトップ/Web共通） ----
export const listGraphs = (includeDeleted = false) =>
  invoke<StoredGraphSummary[]>("list_graphs", { includeDeleted });
export const getGraph = (id: string) => invoke<StoredGraph>("get_graph", { id });
export const ensureGraphFromAsset = (assetId: string) =>
  invoke<string>("ensure_graph_from_asset", { assetId });
export const listGraphVersions = (graphId: string) =>
  invoke<GraphVersionSummary[]>("list_graph_versions", { graphId });
export const getGraphVersion = (versionId: number) =>
  invoke<GraphVersionFull>("get_graph_version", { versionId });
export const createGraph = (payload: CreateStoredGraphPayload) =>
  invoke<string>("create_graph", { payload });
export const updateGraph = (payload: UpdateStoredGraphPayload) =>
  invoke<number>("update_graph", { payload });
export const duplicateGraph = (id: string) => invoke<string>("duplicate_graph", { id });
export const deleteGraph = (id: string, expectedVersion?: number | null) =>
  invoke<void>("delete_graph", { id, expectedVersion: expectedVersion ?? null });
export const restoreGraph = (id: string) => invoke<void>("restore_graph", { id });
export const restoreGraphVersion = (versionId: number, expectedVersion: number) =>
  invoke<number>("restore_graph_version", { versionId, expectedVersion });
export const saveGraphExports = (id: string, files: Record<string, string>) =>
  invoke<string[]>("save_graph_exports", { id, files });
export const insertGraphToProject = (id: string, projectId: number, expectedProjectVersion?: number | null) =>
  invoke<number>("insert_graph_to_project", { id, projectId, expectedProjectVersion: expectedProjectVersion ?? null });
export const createGraphWebSession = (payload: CreateGraphWebSessionPayload) =>
  invoke<GraphWebSession>("create_graph_web_session", { payload });
export const getGraphWebSession = (sessionId: string) =>
  invoke<GraphWebSession>("get_graph_web_session", { sessionId });
export const cancelGraphWebSession = (sessionId: string) =>
  invoke<void>("cancel_graph_web_session", { sessionId });
export const completeGraphWebSession = (sessionId: string, graphId: string, expectedGraphVersion: number) =>
  invoke<CompleteGraphWebSessionResult>("complete_graph_web_session", { sessionId, graphId, expectedGraphVersion });

// ---- LaTeXテンプレート ----
export const listTemplates = () => invoke<TemplateSummary[]>("list_templates");
export const getTemplate = (id: number) => invoke<TemplateFull>("get_template", { id });
export const createTemplate = (name: string) => invoke<number>("create_template", { name });
export const updateTemplate = (payload: {
  id: number;
  expected_version?: number | null;
  name: string;
  description: string;
  base_template: string;
  problem_template: string;
  answer_template: string;
  compile_method: string;
  packages_memo: string;
}) => invoke<string[]>("update_template", { payload });
export const deleteTemplate = (id: number) => invoke<void>("delete_template", { id });
export const duplicateTemplate = (id: number) => invoke<number>("duplicate_template", { id });
export const listTemplateVersions = (templateId: number) =>
  invoke<TemplateVersionSummary[]>("list_template_versions", { templateId });
export const restoreTemplateVersion = (versionId: number) =>
  invoke<void>("restore_template_version", { versionId });
export const analyzeTexFile = (path: string) => invoke<ImportAnalysis>("analyze_tex_file", { path });
export const importTemplateFromTex = (path: string, name: string, mode: string) =>
  invoke<number>("import_template_from_tex", { path, name, mode });
export const addTemplateAsset = (templateId: number, sourcePath: string) =>
  invoke<TemplateAsset>("add_template_asset", { templateId, sourcePath });
export const removeTemplateAsset = (assetId: number) =>
  invoke<void>("remove_template_asset", { assetId });
export const exportTemplate = (id: number, destPath: string) =>
  invoke<void>("export_template", { id, destPath });
export const importTemplateFile = (path: string) => invoke<number>("import_template_file", { path });
export const testCompileTemplate = (templateId: number, kind: "problems" | "answers") =>
  invoke<CompileResult>("test_compile_template", { templateId, kind });
export const compileProblemPreview = (
  problemId: number,
  statement: string,
  answer: string,
  explanation: string,
  layoutMode: "single_column" | "two_column",
) => invoke<CompileResult>("compile_problem_preview", {
  problemId,
  statement,
  answer,
  explanation,
  layoutMode,
});
export const compilePartPreview = (
  partId: number,
  latexSource: string,
  layoutMode: "single_column" | "two_column",
) => invoke<CompileResult>("compile_part_preview", { partId, latexSource, layoutMode });

// ---- 部品ライブラリ ----
export const searchParts = (query: PartSearchQuery) =>
  invoke<PartSummary[]>("search_parts", { query });
export const listAllPartTags = () => invoke<string[]>("list_all_part_tags");
export const listPartCategories = () => invoke<string[]>("list_part_categories");
export const createPart = (title: string, bankNodeId: number | null = null) =>
  invoke<number>("create_part", { title, bankNodeId, unitId: null });
export const getPart = (id: number) => invoke<PartFull>("get_part", { id });
/** 保存に成功すると新しいversionを返す。競合時は ConflictError */
export const updatePart = (payload: {
  id: number;
  bank_node_id: number | null;
  unit_id: number | null;
  title: string;
  part_type: string;
  category: string;
  tags: string[];
  latex_source: string;
  description: string;
  difficulty_rank: string | null;
  is_required: boolean;
  output_target: string;
  layout_mode: string;
  expected_version?: number | null;
}) => invoke<number>("update_part", { payload });
export const duplicatePart = (id: number) => invoke<number>("duplicate_part", { id });
export const deletePart = (id: number) => invoke<void>("delete_part", { id });
export const listPartVersions = (partId: number) =>
  invoke<PartVersionSummary[]>("list_part_versions", { partId });
export const addPartAttachment = (partId: number, sourcePath: string) =>
  invoke<PartAttachment>("add_part_attachment", { partId, sourcePath });
export const removePartAttachment = (attachmentId: number) =>
  invoke<void>("remove_part_attachment", { attachmentId });

// ---- 問題バンクの入出力・整理 ----
export type BankScope = "all" | "node" | "subject" | "field" | "unit" | "problems";
export const exportBank = (
  scopeKind: BankScope,
  id: number | null,
  problemIds: number[] | null,
  destPath: string,
) => invoke<string>("export_bank", { scopeKind, id, problemIds, destPath });
export const importBank = (path: string) => invoke<ImportBankResult>("import_bank", { path });
export const moveProblems = (problemIds: number[], bankNodeId: number) =>
  invoke<void>("move_problems", { problemIds, bankNodeId });
export const deleteProblems = (problemIds: number[]) =>
  invoke<void>("delete_problems", { problemIds });

// ---- 設定・添付・サンプル ----
export const getSettings = () => invoke<Record<string, string>>("get_settings");
export const setSettings = (settings: Record<string, string>) =>
  invoke<void>("set_settings", { settings });
export const addAttachment = (problemId: number, sourcePath: string) =>
  invoke<Attachment>("add_attachment", { problemId, sourcePath });
export const removeAttachment = (attachmentId: number) =>
  invoke<void>("remove_attachment", { attachmentId });
export const createSampleData = () => invoke<void>("create_sample_data");
export const hasAnyData = () => invoke<boolean>("has_any_data");

// ---- Web版: 添付のアップロード（multipart） ----
async function uploadFile(url: string, file: File): Promise<unknown> {
  const form = new FormData();
  form.append("file", file, file.name);
  const res = await fetch(url, {
    method: "POST",
    headers: { "X-Requested-With": "kyozai-kobo" },
    credentials: "same-origin",
    body: form,
  });
  const body = await res.json().catch(() => null);
  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent("kk-auth-required"));
  }
  if (!res.ok) {
    throw new Error((body && (body as { error?: string }).error) || `アップロードに失敗しました (${res.status})`);
  }
  return body;
}
export const uploadAttachment = (problemId: number, file: File) =>
  uploadFile(`/api/uploads/attachment?problemId=${problemId}`, file) as Promise<Attachment>;
export const uploadPartAttachment = (partId: number, file: File) =>
  uploadFile(`/api/uploads/part-attachment?partId=${partId}`, file) as Promise<PartAttachment>;

// ---- 教材サーバー管理（デスクトップのみ） ----
export const serverStatus = () => invoke<ServerStatus>("server_status");
export const serverStart = () => invoke<ServerStatus>("server_start");
export const serverStop = () => invoke<ServerStatus>("server_stop");
export const serverRegenPairing = () => invoke<ServerStatus>("server_regen_pairing");
export const serverSettingsGet = () =>
  invoke<{ port: number; lanMode: boolean; serverAutostart: boolean }>("server_settings_get");
export const serverSettingsSet = (s: {
  port?: number;
  lanMode?: boolean;
  serverAutostart?: boolean;
}) => invoke<void>("server_settings_set", s);
export const listWebDevices = () => invoke<ServerStatus["devices"]>("list_web_devices");
export const revokeWebDevice = (deviceId: number) =>
  invoke<void>("revoke_web_device", { deviceId });
export const tailscaleStatus = () => invoke<TailscaleStatus>("tailscale_status");
export const autostartGet = () => invoke<boolean>("autostart_get");
export const autostartSet = (enabled: boolean) => invoke<boolean>("autostart_set", { enabled });

// ---- バックアップ（デスクトップのみ） ----
export const backupNow = () =>
  invoke<{ dbFile: string; assetsMirrored: string[] }>("backup_now");
export const listBackups = () =>
  invoke<{ fileName: string; sizeBytes: number; modified: string }[]>("list_backups");
export const restoreBackup = (fileName: string) => invoke<void>("restore_backup", { fileName });

// ---- Codex / ChatGPT接続 ----
export const codexStatus = () => invoke<CodexStatus>("codex_status");
export const codexLoginStart = (method: "deviceCode" | "browser") =>
  invoke<CodexStatus["login"]>("codex_login_start", { method });
export const codexLoginCancel = () => invoke<void>("codex_login_cancel");
export const codexLogout = () => invoke<void>("codex_logout");
export const codexTest = () => invoke<{ ok: boolean }>("codex_test");
export const codexModels = () => invoke<CodexModelSettings>("codex_models");
export const codexSetModel = (model: string) => invoke<void>("codex_set_model", { model });
export const codexSetPath = (path: string) => invoke<void>("codex_set_path", { path });

// ---- AI変換 ----
export const aiStoreInputImage = (dataBase64: string, fileName: string) =>
  invoke<{ name: string; width: number; height: number; bytes: number }>("ai_store_input_image", {
    dataBase64,
    fileName,
  });
export const aiCreateJob = (payload: {
  sourceType: "image" | "text";
  conversionMode?: string;
  options?: Record<string, unknown>;
  inputText?: string;
  inputNames?: string[];
  targetEntityType?: string;
  targetEntityId?: number;
  targetField?: string;
}) => invoke<AiJob>("ai_create_job", payload);
export const aiGetJob = (jobId: number) => invoke<AiJob>("ai_get_job", { jobId });
export const aiListJobs = (limit?: number) =>
  invoke<AiJob[]>("ai_list_jobs", { limit: limit ?? null });
export const aiCancelJob = (jobId: number) => invoke<void>("ai_cancel_job", { jobId });
export const aiRetryJob = (jobId: number, mode?: string, options?: Record<string, unknown>) =>
  invoke<AiJob>("ai_retry_job", { jobId, mode: mode ?? null, options: options ?? null });
export const aiDeleteJob = (jobId: number) => invoke<void>("ai_delete_job", { jobId });
export const aiUpdateJobLatex = (jobId: number, latex: string) =>
  invoke<void>("ai_update_job_latex", { jobId, latex });
export const aiRecompileJob = (jobId: number) => invoke<AiJob>("ai_recompile_job", { jobId });
export const aiSaveAsPart = (
  jobId: number,
  title: string,
  category: string | undefined,
  confirmed: boolean,
) =>
  invoke<number>("ai_save_as_part", {
    jobId,
    title,
    category: category ?? null,
    confirmed,
  });
export const aiSaveAsProblem = (
  jobId: number,
  bankNodeId: number,
  title: string,
  confirmed: boolean,
) =>
  invoke<number>("ai_save_as_problem", { jobId, bankNodeId, title, confirmed });
export const aiSaveExtractedProblems = (
  jobId: number,
  bankNodeId: number,
  problems: AiExtractedProblem[],
  confirmed: boolean,
) =>
  invoke<number[]>("ai_save_extracted_problems", { jobId, bankNodeId, problems, confirmed });
export const aiMarkInserted = (
  jobId: number,
  entityType: string,
  entityId: number,
  field: string,
  confirmed: boolean,
) => invoke<void>("ai_mark_inserted", { jobId, entityType, entityId, field, confirmed });
export const aiInsertIntoTargetProblem = (jobId: number, confirmed: boolean) =>
  invoke<{ problemId: number; field: "answer_latex" | "explanation_latex" }>(
    "ai_insert_into_target_problem",
    { jobId, confirmed },
  );
export const aiApplySourceRevision = (jobId: number, confirmed: boolean) =>
  invoke<{
    entityType: "problem" | "part";
    entityId: number;
    field: "statement_latex" | "answer_latex" | "explanation_latex" | "latex_source";
  }>("ai_apply_source_revision", { jobId, confirmed });

// ---- AIチャット / 既存機能Tool Agent ----
export const aiChatCreateSession = (context?: Record<string, unknown>) =>
  invoke<import("./types").AiChatSession>("ai_chat_create_session", { context: context ?? null });
export const aiChatGetSession = (sessionId: string) =>
  invoke<import("./types").AiChatSession>("ai_chat_get_session", { sessionId });
export const aiChatSessionStatus = (sessionId: string) =>
  invoke<{ status: import("./types").AiChatStatus; updatedAt: string }>("ai_chat_session_status", { sessionId });
export const aiChatSendMessage = (payload: {
  sessionId: string;
  content: string;
  inputNames?: string[];
  context?: Record<string, unknown>;
}) => invoke<import("./types").AiChatSession>("ai_chat_send_message", {
  sessionId: payload.sessionId,
  content: payload.content,
  inputNames: payload.inputNames ?? [],
  context: payload.context ?? null,
});
export const aiChatConfirm = (sessionId: string, approved: boolean) =>
  invoke<import("./types").AiChatSession>("ai_chat_confirm", { sessionId, approved });
export const aiChatCancel = (sessionId: string) =>
  invoke<void>("ai_chat_cancel", { sessionId });
export const aiChatRegenerate = (sessionId: string) =>
  invoke<import("./types").AiChatSession>("ai_chat_regenerate", { sessionId });
export const aiChatUndo = (sessionId: string) =>
  invoke<import("./types").AiChatSession>("ai_chat_undo", { sessionId });
export const aiChatRedo = (sessionId: string) =>
  invoke<import("./types").AiChatSession>("ai_chat_redo", { sessionId });
export const aiChatReadAttachment = (sessionId: string, storedName: string) =>
  invoke<{ mimeType: string; dataBase64: string }>("ai_chat_read_attachment", { sessionId, storedName });
