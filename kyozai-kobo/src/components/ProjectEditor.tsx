import { useEffect, useRef, useState } from "react";
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  addContentItem,
  addPartToProject,
  addPatternToProject,
  addProblemToProject,
  aiCreateJob,
  compilePdf,
  createGraphWebSession,
  ensureGraphFromAsset,
  exportTex,
  generateTex,
  getProject,
  listTemplates,
  listGraphAssets,
  refreshItemFromBank,
  refreshPartItemFromLibrary,
  refreshPatternItemFromLibrary,
  refreshProjectTemplate,
  removeProjectItem,
  reorderProjectItems,
  setProjectTemplate,
  showInFolder,
  startGraphIntegration,
  updateProjectItem,
  updateProjectMeta,
  updateProjectSettings,
} from "../api";
import { insertTextAtRange, waitForGraphIntegration } from "../graphIntegration";
import { openAiChatForTarget } from "../aiChat";
import { useApp, type ProjectReviewFix } from "../store";
import {
  buildFileUrl,
  ConflictError,
  isTauri,
  openCompiledFile,
} from "../transport";
import type {
  AiJob,
  BookletKind,
  CompileResult,
  GraphAssetSummary,
  ProjectFull,
  ProjectItem,
  ProjectSettings,
  TemplateSummary,
} from "../types";
import { LatexEditor, type LatexEditorHandle } from "./LatexEditor";
import { AiConvertDialog, type AiConvertPreset } from "./AiConvertDialog";
import { LatexPreview } from "./LatexPreview";
import { PdfCanvasViewer } from "./PdfCanvasViewer";
import { PdfSaveButton } from "./PdfSaveButton";
import { Icon } from "./Icon";
import { PartPicker } from "./PartPicker";
import { PatternPicker } from "./PatternPicker";
import { ProblemPicker } from "./ProblemPicker";
import { DifficultyBadge, DifficultyRankBadge, Modal } from "./ui";

const ITEM_TYPE_LABEL: Record<string, string> = {
  problem: "問題",
  heading: "見出し",
  text: "説明文",
  pagebreak: "改ページ",
  part: "部品",
  pattern: "定石",
};

const OUTPUT_TARGET_LABEL: Record<string, string> = {
  problems: "問題冊子",
  answers: "解答冊子",
  both: "両方",
  none: "出力しない",
};

function buildProjectReviewSource(project: ProjectFull): string {
  const lines: string[] = [
    "【教材メタデータ】",
    `教材ID: ${project.id}`,
    `教材名: ${project.name}`,
    `説明: ${project.description || "（なし）"}`,
    `問題数: ${project.items.filter((item) => item.item_type === "problem").length}`,
    `全項目数: ${project.items.length}`,
    `考え方を出力: ${project.settings.include_explanation ? "はい" : "いいえ"}`,
    `問題冊子レイアウト: ${project.settings.problem_two_column ? "二段組" : "一段組"}`,
    `解答冊子レイアウト: ${project.settings.two_column_mode === "none" ? "一段組" : "二段組"}`,
    "",
    "【確認対象の教材項目】",
  ];

  project.items.forEach((item, index) => {
    lines.push("", `===== 項目 ${index + 1} / 教材項目ID ${item.id} =====`);
    if (item.item_type === "problem") {
      lines.push(
        "種別: 問題",
        `問題バンクID: ${item.problem_id ?? "（元問題削除済み）"}`,
        `題名: ${item.snap_title || "（題名なし）"}`,
        "【問題文（一段組用）】",
        item.snap_statement || "（空欄）",
        "【問題文（二段組用）】",
        item.snap_statement_two_column || item.snap_statement || "（空欄）",
        "【考え方】",
        item.snap_explanation || "（空欄）",
        "【解答】",
        item.snap_answer || "（空欄）",
      );
      return;
    }
    if (item.item_type === "part") {
      lines.push(
        "種別: 部品",
        `部品ライブラリID: ${item.part_id ?? "（元部品削除済み）"}`,
        `題名: ${item.snap_title || "（題名なし）"}`,
        `部品種別: ${item.snap_part_type || "（未設定）"}`,
        `出力先: ${OUTPUT_TARGET_LABEL[item.snap_part_output_target] ?? item.snap_part_output_target}`,
        `レイアウト: ${item.snap_part_layout_mode === "two_column" ? "二段組" : "一段組"}`,
        "【部品ソース】",
        item.content || "（空欄）",
      );
      return;
    }
    lines.push(
      `種別: ${ITEM_TYPE_LABEL[item.item_type] ?? item.item_type}`,
      item.item_type === "heading" ? `見出しレベル: ${item.heading_level}` : "",
      "【内容】",
      item.content || (item.item_type === "pagebreak" ? "\\newpage" : "（空欄）"),
    );
  });
  return lines.filter((line) => line !== "").join("\n");
}

function SortableItem({
  item,
  children,
}: {
  item: ProjectItem;
  children: (dragHandleProps: Record<string, unknown>) => React.ReactNode;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: item.id,
  });
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={isDragging ? "z-10 opacity-70" : ""}
    >
      {children({ ...attributes, ...listeners })}
    </div>
  );
}

/** 教材プロジェクト編集画面 */
export function ProjectEditor({ projectId, onBack }: { projectId: number; onBack: () => void }) {
  const {
    showToast,
    confirm,
    setContextName,
    setDirty,
    setLastCompile,
    setLogOpen,
    bumps,
    openGraphOverlay,
    pendingProjectReviewFix,
    clearProjectReviewFix,
  } = useApp();
  const [project, setProject] = useState<ProjectFull | null>(null);
  const [templates, setTemplates] = useState<TemplateSummary[]>([]);
  const [showPicker, setShowPicker] = useState(false);
  const [showPartPicker, setShowPartPicker] = useState(false);
  const [showPatternPicker, setShowPatternPicker] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [editItem, setEditItem] = useState<ProjectItem | null>(null);
  const [editItemInitialTab, setEditItemInitialTab] = useState<
    "statement" | "answer" | "explanation"
  >("statement");
  const [editItemReviewFix, setEditItemReviewFix] = useState<{
    field: ProjectReviewFix["field"];
    guidance: string;
    action: ProjectReviewFix["action"];
  } | null>(null);
  const [previewIds, setPreviewIds] = useState<Set<number>>(new Set());
  const [texPreview, setTexPreview] = useState<{ kind: BookletKind; tex: string } | null>(null);
  const [compileResult, setCompileResult] = useState<
    (CompileResult & { download_key: number }) | null
  >(null);
  const [compilePreviewUrl, setCompilePreviewUrl] = useState<string | null>(null);
  const [compilePreviewZoom, setCompilePreviewZoom] = useState(100);
  const [compiling, setCompiling] = useState<string | null>(null);
  const [showLog, setShowLog] = useState(false);
  const [graphBusy, setGraphBusy] = useState(false);
  const [graphAssets, setGraphAssets] = useState<GraphAssetSummary[] | null>(null);
  const [projectReviewJob, setProjectReviewJob] = useState<AiJob | null>(null);
  const [projectReviewStarting, setProjectReviewStarting] = useState(false);
  const [nameEditing, setNameEditing] = useState(false);
  const seenProjectsBumpRef = useRef(bumps.projects);
  const pendingProjectRefreshRef = useRef(false);
  const projectLoadRequestRef = useRef(0);
  const projectVersionRef = useRef(0);
  const projectInteractionRef = useRef(false);
  projectInteractionRef.current = nameEditing || showSettings || editItem != null;

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));

  const load = async (preserveInteraction = false) => {
    const requestId = ++projectLoadRequestRef.current;
    try {
      const p = await getProject(projectId);
      if (requestId !== projectLoadRequestRef.current) return;
      if (preserveInteraction && projectInteractionRef.current) {
        pendingProjectRefreshRef.current = true;
        return;
      }
      setProject(p);
      projectVersionRef.current = p.version;
      setDirty(false);
      setContextName(p.name);
    } catch (e) {
      if (requestId === projectLoadRequestRef.current) showToast(String(e), "error");
    }
  };

  const openReviewFix = (
    target: Omit<ProjectReviewFix, "projectId">,
  ) => {
    const currentProject = project;
    if (!currentProject) return;
    const targetItem = currentProject.items.find((item) => item.id === target.itemId);
    if (!targetItem) {
      showToast(
        "AI確認後に対象項目が削除または更新されたため、修正画面を開けませんでした",
        "error",
      );
      return;
    }
    const problemTab =
      target.field === "answer" || target.field === "explanation"
        ? target.field
        : "statement";
    setEditItemInitialTab(problemTab);
    setEditItemReviewFix({
      field: target.field,
      guidance: target.guidance,
      action: target.action,
    });
    setEditItem(targetItem);
    setProjectReviewJob(null);
  };

  useEffect(() => {
    seenProjectsBumpRef.current = bumps.projects;
    pendingProjectRefreshRef.current = false;
    load();
    listTemplates().then(setTemplates).catch(() => {});
    return () => {
      setContextName("");
      setDirty(false);
    };
  }, [projectId]);

  const hasUnsavedInteraction = nameEditing || showSettings || editItem != null;

  // モーダルや名前入力中はリモート再読込を保留し、閉じた後に安全に反映する。
  useEffect(() => {
    if (seenProjectsBumpRef.current === bumps.projects) return;
    seenProjectsBumpRef.current = bumps.projects;
    if (hasUnsavedInteraction) {
      pendingProjectRefreshRef.current = true;
      return;
    }
    void load(true);
  }, [bumps.projects]);

  useEffect(() => {
    if (hasUnsavedInteraction || !pendingProjectRefreshRef.current) return;
    pendingProjectRefreshRef.current = false;
    void load(true);
  }, [nameEditing, showSettings, editItem]);

  useEffect(() => {
    if (
      !project ||
      !pendingProjectReviewFix ||
      pendingProjectReviewFix.projectId !== project.id
    ) {
      return;
    }
    openReviewFix({
      itemId: pendingProjectReviewFix.itemId,
      field: pendingProjectReviewFix.field,
      guidance: pendingProjectReviewFix.guidance,
      action: pendingProjectReviewFix.action,
    });
    clearProjectReviewFix();
  }, [project?.id, project?.items, pendingProjectReviewFix]);

  if (!project)
    return (
      <p className="p-4 text-sm" style={{ color: "var(--muted)" }}>
        読み込み中...
      </p>
    );

  const saveMeta = async (name: string, description: string): Promise<boolean> => {
    try {
      const version = await updateProjectMeta(project.id, name, description, projectVersionRef.current);
      projectVersionRef.current = version;
      setProject((current) => (current ? { ...current, name, description, version } : current));
      setContextName(name);
      setDirty(false);
      return true;
    } catch (e) {
      if (e instanceof ConflictError) {
        const overwrite = await confirm(
          "他の端末で教材名または設定が更新されています。\n「OK」: 自分の変更で上書き\n「キャンセル」: サーバー版を読み込む",
        );
        if (overwrite) {
          try {
            const version = await updateProjectMeta(project.id, name, description, null);
            projectVersionRef.current = version;
            setProject((current) => (current ? { ...current, name, description, version } : current));
            setDirty(false);
            showToast("自分の変更で更新しました");
            return true;
          } catch (overwriteError) {
            showToast(String(overwriteError), "error");
            return false;
          }
        }
        await load();
        showToast("サーバー版を読み込みました");
        return false;
      }
      showToast(String(e), "error");
      return false;
    }
  };

  const saveSettings = async (settings: ProjectSettings): Promise<boolean> => {
    try {
      const version = await updateProjectSettings(project.id, settings, projectVersionRef.current);
      projectVersionRef.current = version;
      setProject((current) => (current ? { ...current, settings, version } : current));
      return true;
    } catch (e) {
      if (e instanceof ConflictError) {
        const overwrite = await confirm(
          "他の端末で教材名または設定が更新されています。\n「OK」: 自分の設定で上書き\n「キャンセル」: サーバー版を読み込む",
        );
        if (overwrite) {
          try {
            const version = await updateProjectSettings(project.id, settings, null);
            projectVersionRef.current = version;
            setProject((current) => (current ? { ...current, settings, version } : current));
            showToast("自分の設定で更新しました");
            return true;
          } catch (overwriteError) {
            showToast(String(overwriteError), "error");
            return false;
          }
        }
        await load();
        showToast("サーバー版を読み込みました");
        return false;
      }
      showToast(String(e), "error");
      return false;
    }
  };

  const onChangeTemplate = async (templateId: number) => {
    if (
      !(await confirm(
        "使用テンプレートを変更しますか？\n（選択したテンプレートの現在の内容がこの教材にスナップショット保存されます）",
      ))
    )
      return;
    try {
      await setProjectTemplate(project.id, templateId);
      await load();
      showToast("テンプレートを変更しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRefreshTemplate = async () => {
    if (!(await confirm("テンプレートの最新内容でこの教材のスナップショットを更新しますか？"))) return;
    try {
      await refreshProjectTemplate(project.id);
      await load();
      showToast("テンプレートを最新版に更新しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDragEnd = async (e: DragEndEvent) => {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    const oldIndex = project.items.findIndex((i) => i.id === active.id);
    const newIndex = project.items.findIndex((i) => i.id === over.id);
    const items = arrayMove(project.items, oldIndex, newIndex);
    setProject({ ...project, items });
    try {
      await reorderProjectItems(
        project.id,
        items.map((i) => i.id),
      );
    } catch (err) {
      showToast(String(err), "error");
      load();
    }
  };

  const onRemoveItem = async (item: ProjectItem) => {
    const label = item.item_type === "problem" ? `問題「${item.snap_title}」` : ITEM_TYPE_LABEL[item.item_type];
    if (!(await confirm(`${label}を教材から削除しますか？`))) return;
    try {
      await removeProjectItem(item.id);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRefreshItem = async (item: ProjectItem) => {
    if (!(await confirm("問題バンクの最新内容でこの問題のスナップショットを更新しますか？"))) return;
    try {
      await refreshItemFromBank(item.id);
      await load();
      showToast("最新版に更新しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRefreshPartItem = async (item: ProjectItem) => {
    if (!(await confirm("部品ライブラリの最新内容でこの部品のスナップショットを更新しますか？"))) return;
    try {
      await refreshPartItemFromLibrary(item.id);
      await load();
      showToast("部品を最新版に更新しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRefreshPatternItem = async (item: ProjectItem) => {
    if (!(await confirm("定石ライブラリの最新内容でこの定石のスナップショットを更新しますか？"))) return;
    try {
      await refreshPatternItemFromLibrary(item.id);
      await load();
      showToast("定石を最新版に更新しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const addContent = async (type: "heading" | "text" | "pagebreak", level = 1) => {
    try {
      const content =
        type === "heading" ? (level >= 2 ? "新しい節" : "新しい章") : type === "text" ? "説明文" : "";
      await addContentItem(project.id, type, content, level);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const startProjectReview = async () => {
    if (project.items.length === 0) {
      showToast("確認する教材項目がありません", "error");
      return;
    }
    setProjectReviewStarting(true);
    try {
      const reviewJob = await aiCreateJob({
        sourceType: "text",
        conversionMode: "project_review",
        options: {
          projectId: project.id,
          projectName: project.name,
          projectVersion: project.version,
          itemCount: project.items.length,
          problemCount: project.items.filter((item) => item.item_type === "problem").length,
        },
        inputText: buildProjectReviewSource(project),
        targetEntityType: "project",
        targetEntityId: project.id,
        targetField: "review",
      });
      setProjectReviewJob(reviewJob);
      showToast("教材全体のAI確認を開始しました。バックグラウンドでも処理を続けられます");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setProjectReviewStarting(false);
    }
  };

  const onInsertProjectGraph = async () => {
    if (graphBusy) return;
    setGraphBusy(true);
    if (!isTauri) {
      try {
        const session = await createGraphWebSession({
          projectId: project.id,
          targetField: "project_text",
          selectionStart: 0,
          selectionEnd: 0,
        });
        openGraphOverlay(
          session,
          async (result) => {
            await addContentItem(project.id, "text", result.insertedLatex, 1);
            await load();
            setGraphBusy(false);
            showToast("グラフを教材へ追加しました");
          },
          () => {
            setGraphBusy(false);
            showToast("グラフ挿入は中止されました");
          },
        );
      } catch (e) {
        setGraphBusy(false);
        showToast(String(e), "error");
      }
      return;
    }
    try {
      const session = await startGraphIntegration({
        projectId: project.id,
        insertTarget: "project_text",
        selectionStart: 0,
        selectionEnd: 0,
      });
      showToast("グラフ作成アプリを起動しました");
      const result = await waitForGraphIntegration(session);
      if (result.status === "completed" && result.insertedLatex) {
        await addContentItem(project.id, "text", result.insertedLatex, 1);
        await load();
        showToast("グラフを教材へ追加しました");
      } else if (result.status === "cancelled") {
        showToast("グラフ挿入は中止されました");
      } else {
        showToast(result.details ? `${result.message}\n${result.details}` : result.message, "error");
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setGraphBusy(false);
    }
  };

  const openGraphReeditList = async () => {
    try {
      setGraphAssets(await listGraphAssets(project.id));
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onReeditGraph = async (asset: GraphAssetSummary) => {
    if (graphBusy) return;
    setGraphBusy(true);
    if (!isTauri) {
      try {
        const latest = await getProject(project.id);
        const candidates = asset.itemId
          ? latest.items.filter((item) => item.id === asset.itemId)
          : latest.items;
        const fields = [
          "content",
          "snap_statement",
          "snap_statement_two_column",
          "snap_answer",
          "snap_explanation",
        ] as const;
        const match = candidates
          .flatMap((item) => fields.map((field) => ({ item, field, value: item[field] })))
          .find(({ value }) => !!asset.insertedLatex && value.includes(asset.insertedLatex));
        if (!match) {
          throw new Error("教材内の挿入位置を特定できません。グラフ一覧から複製して追加してください。");
        }
        const start = match.value.indexOf(asset.insertedLatex);
        const end = start + asset.insertedLatex.length;
        const targetField = match.field === "content"
          ? "project_item_content"
          : `project_item_${match.field.replace("snap_", "")}`;
        const graphId = await ensureGraphFromAsset(asset.assetId);
        const session = await createGraphWebSession({
          projectId: project.id,
          problemId: match.item.problem_id,
          itemId: match.item.id,
          targetField,
          selectionStart: start,
          selectionEnd: end,
        });
        openGraphOverlay(
          session,
          async (result) => {
            await updateProjectItem(match.item.id, {
              [match.field]: insertTextAtRange(match.value, result.insertedLatex, start, end),
              expected_version: match.item.version,
            });
            setGraphAssets(null);
            await load();
            setGraphBusy(false);
            showToast("既存グラフを新しい版として更新しました");
          },
          () => {
            setGraphBusy(false);
            showToast("グラフ再編集は中止されました");
          },
          graphId,
        );
      } catch (e) {
        setGraphBusy(false);
        showToast(String(e), "error");
      }
      return;
    }
    try {
      const session = await startGraphIntegration({
        projectId: project.id,
        insertTarget: "graph_reedit",
        reeditAssetId: asset.assetId,
      });
      showToast("グラフ作成アプリを再編集モードで起動しました");
      const result = await waitForGraphIntegration(session);
      if (result.status === "completed") {
        setGraphAssets(null);
        showToast("既存のグラフを更新しました");
      } else if (result.status === "cancelled") {
        showToast("グラフ再編集は中止されました");
      } else {
        showToast(result.details ? `${result.message}\n${result.details}` : result.message, "error");
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setGraphBusy(false);
    }
  };

  const KIND_LABEL: Record<BookletKind, string> = {
    problems: "問題冊子",
    answers: "解答冊子",
    combined: "合本（問題＋解答）",
  };

  const onExportTex = async (kind: BookletKind) => {
    try {
      const path = await exportTex(project.id, kind);
      showToast(`.texを書き出しました:\n${path}`);
      setTexPreview(null);
      if (isTauri) await showInFolder(path).catch(() => {});
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onCompile = async (kind: BookletKind) => {
    // テンプレート本体が更新されている場合は反映するか確認
    if (project.template_updated) {
      const update = await confirm(
        "使用テンプレートの本体が更新されています。\nこの教材のスナップショットを最新版に更新してから出力しますか？\n\n「OK」= 最新版に更新して出力\n「キャンセル」= 保存済みスナップショットのまま出力",
      );
      if (update) {
        try {
          await refreshProjectTemplate(project.id);
          await load();
        } catch (e) {
          showToast(String(e), "error");
          return;
        }
      }
    }
    setCompiling(kind);
    setCompilePreviewUrl(null);
    setCompilePreviewZoom(100);
    try {
      const result = await compilePdf(project.id, kind);
      const resultWithDownloadKey = { ...result, download_key: Date.now() };
      setCompileResult(resultWithDownloadKey);
      if (!isTauri && result.success && result.pdf_path) {
        setCompilePreviewUrl(buildFileUrl(result.pdf_path, Date.now()));
      }
      setLastCompile({
        ...resultWithDownloadKey,
        label: `${project.name}（${KIND_LABEL[kind]}）`,
      });
      if (!result.success) setLogOpen(true);
      if (isTauri && result.success && result.pdf_path) {
        await openCompiledFile(result.pdf_path).catch(() => {});
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setCompiling(null);
    }
  };

  const togglePreview = (id: number) => {
    const next = new Set(previewIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setPreviewIds(next);
  };

  // 表示用の問題番号ラベルを事前計算（章ごとリセット・番号付き章の「2-1」形式を反映）
  const numberLabels = new Map<number, string>();
  {
    let n = 0;
    let chapterNo = 0;
    let chapterNumbered = false;
    for (const item of project.items) {
      if (item.item_type === "heading" && item.heading_level <= 1) {
        if (project.settings.reset_numbering_per_chapter) n = 0;
        const numbered = project.settings.number_headings && item.heading_numbered;
        if (numbered) {
          chapterNo += 1;
          chapterNumbered = true;
        } else {
          chapterNumbered = false;
        }
      } else if (item.item_type === "problem") {
        n += 1;
        const nStr =
          project.settings.reset_numbering_per_chapter && chapterNumbered ? `${chapterNo}-${n}` : `${n}`;
        numberLabels.set(item.id, project.settings.number_format.replace("{n}", nStr));
      }
    }
  }

  return (
    <div className="project-editor flex h-full min-w-0 flex-col">
      {/* ヘッダー */}
      <div className="flex items-center gap-2 border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
        <button onClick={onBack} className="btn btn-ghost btn-sm">
          ← 一覧
        </button>
        <input
          value={project.name}
          onChange={(e) => {
            const name = e.target.value;
            setProject((current) => (current ? { ...current, name } : current));
            setContextName(name);
            setDirty(true);
          }}
          onFocus={() => setNameEditing(true)}
          onBlur={() => {
            setNameEditing(false);
            void saveMeta(project.name, project.description);
          }}
          className="input min-w-0 flex-1 font-semibold"
          placeholder="教材名"
        />
        <button
          onClick={() => {
            setShowSettings(true);
            setDirty(true);
          }}
          className="btn btn-ghost"
        >
          出力設定
        </button>
      </div>

      {/* テンプレート行 */}
      <div
        className="flex flex-wrap items-center gap-2 border-b px-3 py-1.5"
        style={{ borderColor: "var(--border)" }}
      >
        <span className="section-label">テンプレート</span>
        <select
          value={project.template_id ?? ""}
          onChange={(e) => {
            if (e.target.value) onChangeTemplate(Number(e.target.value));
          }}
          className="select text-xs"
        >
          {project.template_id == null && <option value="">（削除済み: {project.template_name || "既定"}）</option>}
          {templates.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
        {project.template_updated && (
          <button
            onClick={onRefreshTemplate}
            className="badge badge-warn cursor-pointer"
            title="テンプレート本体が更新されています。クリックでこの教材のスナップショットを最新版に更新"
          >
            テンプレート更新あり → 最新版に更新
          </button>
        )}
        {!project.template_updated && project.template_id != null && (
          <span className="text-[11px]" style={{ color: "var(--muted)" }}>
            スナップショット保存済み（テンプレートを変更しても過去の教材は変わりません）
          </span>
        )}
      </div>

      {/* ツールバー */}
      <div
        className="flex flex-wrap items-center gap-1.5 border-b px-3 py-1.5"
        style={{ borderColor: "var(--border)" }}
      >
        <button
          onClick={() => openAiChatForTarget({ kind: "material", id: project.id, title: project.name, currentScreen: "projects" })}
          className="btn btn-outline btn-sm"
          title="この教材を対象にAI Chatを開く"
        >
          <Icon name="sparkle" size={14} /> AI Chat
        </button>
        <button onClick={() => setShowPicker(true)} className="btn btn-solid btn-sm">
          ＋ 問題を追加
        </button>
        <button onClick={() => setShowPartPicker(true)} className="btn btn-outline btn-sm">
          ＋ 部品を追加
        </button>
        <button onClick={() => setShowPatternPicker(true)} className="btn btn-outline btn-sm">
          ＋ 定石を追加
        </button>
        <button onClick={() => addContent("heading", 1)} className="btn btn-ghost btn-sm" title="章レベルの見出し（\section）">
          ＋章見出し
        </button>
        <button onClick={() => addContent("heading", 2)} className="btn btn-ghost btn-sm" title="節レベルの見出し（\subsection）">
          ＋節見出し
        </button>
        <button onClick={() => addContent("text")} className="btn btn-ghost btn-sm">
          ＋説明文
        </button>
        <button onClick={() => addContent("pagebreak")} className="btn btn-ghost btn-sm">
          ＋改ページ
        </button>
        <button onClick={onInsertProjectGraph} disabled={graphBusy} className="btn btn-outline btn-sm">
          {graphBusy ? "グラフ連携中..." : "グラフを挿入"}
        </button>
        <button onClick={openGraphReeditList} disabled={graphBusy} className="btn btn-ghost btn-sm">
          グラフを再編集
        </button>
        <button
          onClick={startProjectReview}
          disabled={projectReviewStarting || project.items.length === 0}
          className="btn btn-outline btn-sm"
          title="教材内の問題文・解答・解説・部品をまとめてAIで点検"
        >
          {projectReviewStarting ? "AI確認を開始中..." : "教材全体をAI確認"}
        </button>
        <span className="mx-2 h-4 w-px" style={{ background: "var(--border)" }} />
        {(["problems", "answers", "combined"] as BookletKind[]).map((k) => (
          <button
            key={k}
            onClick={async () => setTexPreview({ kind: k, tex: await generateTex(project.id, k) })}
            className="btn btn-ghost btn-sm"
            title={`${KIND_LABEL[k]}のLaTeXソースを表示`}
          >
            {k === "problems" ? "問題" : k === "answers" ? "解答" : "合本"} .tex
          </button>
        ))}
        <button onClick={() => onCompile("problems")} disabled={compiling != null} className="btn btn-outline btn-sm">
          {compiling === "problems" ? "生成中..." : <><Icon name="play" size={15} /> 問題冊子PDF</>}
        </button>
        <button onClick={() => onCompile("answers")} disabled={compiling != null} className="btn btn-outline btn-sm">
          {compiling === "answers" ? "生成中..." : <><Icon name="play" size={15} /> 解答冊子PDF</>}
        </button>
        <button
          onClick={() => onCompile("combined")}
          disabled={compiling != null}
          className="btn btn-outline btn-sm"
          title="問題冊子と解答冊子を1つのPDFにまとめて出力"
        >
          {compiling === "combined" ? "生成中..." : <><Icon name="play" size={15} /> 合本PDF</>}
        </button>
      </div>

      {/* 項目リスト */}
      <div className="project-items-scroll flex-1 overflow-y-auto px-4 py-3">
        {project.items.length === 0 ? (
          <p className="py-10 text-center text-sm" style={{ color: "var(--muted)" }}>
            「＋問題を追加」から問題バンクの問題を追加してください。
          </p>
        ) : (
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
            <SortableContext items={project.items.map((i) => i.id)} strategy={verticalListSortingStrategy}>
              <div className="mx-auto max-w-3xl space-y-1.5">
                {project.items.map((item) => {
                  return (
                    <SortableItem key={item.id} item={item}>
                      {(handle) => (
                        <div className="card card-glow px-3 py-2">
                          <div className="project-item-main flex items-center gap-2">
                            <span
                              {...handle}
                              className="cursor-grab select-none"
                              style={{ color: "var(--border-strong)" }}
                              title="ドラッグで並べ替え"
                            >
                              ⠿
                            </span>
                            {item.item_type === "problem" ? (
                              <>
                                <span
                                  className="min-w-8 shrink-0 text-right text-xs font-bold whitespace-nowrap"
                                  style={{ color: "var(--accent)" }}
                                >
                                  {project.settings.auto_number ? (numberLabels.get(item.id) ?? "・") : "・"}
                                </span>
                                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                                  {item.snap_title}
                                </span>
                                <DifficultyBadge value={item.snap_difficulty} />
                                <DifficultyRankBadge rank={item.snap_difficulty_rank} required={item.snap_is_required} />
                                {!item.source_exists && (
                                  <span
                                    className="badge badge-muted"
                                    title="元の問題は削除されていますが、スナップショットで出力できます"
                                  >
                                    元問題削除済
                                  </span>
                                )}
                                {item.bank_updated && (
                                  <button
                                    onClick={() => onRefreshItem(item)}
                                    className="badge badge-warn cursor-pointer"
                                    title="問題バンク側が更新されています。クリックで最新版に更新"
                                  >
                                    バンク更新あり
                                  </button>
                                )}
                              </>
                            ) : item.item_type === "part" ? (
                              <>
                                <span className="badge badge-standard shrink-0">部品</span>
                                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                                  {item.snap_title}
                                  <span className="ml-2 text-xs font-normal" style={{ color: "var(--muted)" }}>
                                    {item.snap_part_type || "custom"} / {item.snap_part_category || "カテゴリなし"} /{" "}
                                    {OUTPUT_TARGET_LABEL[item.snap_part_output_target] ?? item.snap_part_output_target} /{" "}
                                    {item.snap_part_layout_mode === "two_column" ? "二段組" : "一段組"}
                                  </span>
                                </span>
                                <DifficultyRankBadge
                                  rank={item.snap_difficulty_rank}
                                  required={item.snap_is_required}
                                  muted
                                />
                                {!item.source_exists && (
                                  <span
                                    className="badge badge-muted"
                                    title="元の部品は削除されていますが、スナップショットで出力できます"
                                  >
                                    元部品削除済
                                  </span>
                                )}
                                {item.part_updated && (
                                  <button
                                    onClick={() => onRefreshPartItem(item)}
                                    className="badge badge-warn cursor-pointer"
                                    title="部品ライブラリ側が更新されています。クリックで最新版に更新"
                                  >
                                    ライブラリ更新あり
                                  </button>
                                )}
                              </>
                            ) : item.item_type === "pattern" ? (
                              <>
                                <span className="badge badge-standard shrink-0">定石</span>
                                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                                  {item.snap_title}
                                  <span className="ml-2 text-xs font-normal" style={{ color: "var(--muted)" }}>
                                    追加時点の内容で固定
                                  </span>
                                </span>
                                {!item.source_exists && (
                                  <span
                                    className="badge badge-muted"
                                    title="元の定石は削除されていますが、スナップショットで出力できます"
                                  >
                                    元定石削除済
                                  </span>
                                )}
                                {item.pattern_updated && (
                                  <button
                                    onClick={() => onRefreshPatternItem(item)}
                                    className="badge badge-warn cursor-pointer"
                                    title="定石ライブラリ側に新しい版があります。クリックで最新版に更新"
                                  >
                                    新しい版があります
                                  </button>
                                )}
                              </>
                            ) : item.item_type === "pagebreak" ? (
                              <span
                                className="flex-1 border-t border-dashed text-center text-xs"
                                style={{ borderColor: "var(--border-strong)", color: "var(--muted)" }}
                              >
                                ─ 改ページ ─
                              </span>
                            ) : (
                              <span
                                className={`min-w-0 flex-1 truncate text-sm ${
                                  item.item_type === "heading" ? "font-bold" : ""
                                } ${item.item_type === "heading" && item.heading_level >= 2 ? "pl-5" : ""}`}
                                style={item.item_type === "text" ? { color: "var(--muted)" } : undefined}
                              >
                                <span className="badge badge-muted mr-1.5">
                                  {item.item_type === "heading"
                                    ? item.heading_level >= 2
                                      ? "節見出し"
                                      : "章見出し"
                                    : ITEM_TYPE_LABEL[item.item_type]}
                                </span>
                                {item.item_type === "heading" &&
                                  project.settings.number_headings &&
                                  !item.heading_numbered && (
                                    <span className="badge badge-muted mr-1.5" title="この見出しは番号なし">
                                      番号なし
                                    </span>
                                  )}
                                {item.content}
                              </span>
                            )}
                            <span className="project-item-actions flex shrink-0 gap-1">
                              {item.item_type !== "pagebreak" && (
                                <button onClick={() => togglePreview(item.id)} className="btn btn-ghost btn-sm">
                                  {previewIds.has(item.id) ? "閉じる" : "プレビュー"}
                                </button>
                              )}
                              {item.item_type !== "pagebreak" && (
                                <button
                                  onClick={() => {
                                    setEditItemInitialTab("statement");
                                    setEditItemReviewFix(null);
                                    setEditItem(item);
                                  }}
                                  className="btn btn-ghost btn-sm"
                                  title={item.item_type === "problem" ? "この教材内だけの内容を編集" : "編集"}
                                >
                                  編集
                                </button>
                              )}
                              {item.item_type === "problem" && item.source_exists && !item.bank_updated && (
                                <button
                                  onClick={() => onRefreshItem(item)}
                                  className="btn btn-ghost btn-sm"
                                  title="問題バンクの最新内容で更新"
                                >
                                  最新版
                                </button>
                              )}
                              {item.item_type === "part" && item.source_exists && !item.part_updated && (
                                <button
                                  onClick={() => onRefreshPartItem(item)}
                                  className="btn btn-ghost btn-sm"
                                  title="部品ライブラリの最新内容で更新"
                                >
                                  最新版
                                </button>
                              )}
                              <button onClick={() => onRemoveItem(item)} className="btn btn-danger btn-sm">
                                ✕
                              </button>
                            </span>
                          </div>
                          {previewIds.has(item.id) && item.item_type === "problem" && (
                            <div className="mt-2 border-t pt-2 pl-8" style={{ borderColor: "var(--border)" }}>
                              <div className="paper space-y-2">
                                <LatexPreview
                                  source={
                                    project.settings.problem_two_column
                                      ? item.snap_statement_two_column || item.snap_statement
                                      : item.snap_statement
                                  }
                                />
                                {item.snap_answer && (
                                  <details>
                                    <summary className="cursor-pointer text-xs" style={{ color: "#555" }}>
                                      解答を表示
                                    </summary>
                                    <LatexPreview source={item.snap_answer} />
                                  </details>
                                )}
                              </div>
                            </div>
                          )}
                          {previewIds.has(item.id) &&
                            (item.item_type === "heading" || item.item_type === "text" || item.item_type === "part") && (
                              <div className="mt-2 border-t pt-2 pl-8" style={{ borderColor: "var(--border)" }}>
                                <div className="paper">
                                  <LatexPreview source={item.content} />
                                </div>
                              </div>
                            )}
                        </div>
                      )}
                    </SortableItem>
                  );
                })}
              </div>
            </SortableContext>
          </DndContext>
        )}
      </div>

      {/* 問題追加モーダル */}
      {showPicker && (
        <ProblemPicker
          existingProblemIds={project.items.flatMap((item) =>
            item.item_type === "problem" && item.problem_id != null ? [item.problem_id] : [],
          )}
          onClose={async () => {
            setShowPicker(false);
            await load();
          }}
          onPick={async (problemId) => {
            await addProblemToProject(project.id, problemId);
          }}
        />
      )}

      {projectReviewJob && (
        <AiConvertDialog
          initialJob={projectReviewJob}
          onProjectReviewFix={(target, action) =>
            openReviewFix({
              ...target,
              action,
            })
          }
          onClose={() => setProjectReviewJob(null)}
        />
      )}

      {/* 部品追加モーダル */}
      {showPartPicker && (
        <PartPicker
          onClose={async () => {
            setShowPartPicker(false);
            await load();
          }}
          onPick={async (partId) => {
            await addPartToProject(project.id, partId);
          }}
        />
      )}

      {/* 定石追加モーダル */}
      {showPatternPicker && (
        <PatternPicker
          title="教材へ追加する定石を選択"
          onClose={async () => {
            setShowPatternPicker(false);
            await load();
          }}
          onPick={async (pattern) => {
            await addPatternToProject(project.id, pattern.id);
          }}
        />
      )}

      {/* 出力設定モーダル */}
      {showSettings && (
        <SettingsModal
          project={project}
          onSave={saveSettings}
          onSaveMeta={saveMeta}
          onClose={() => {
            setShowSettings(false);
            setDirty(false);
          }}
        />
      )}

      {/* 項目編集モーダル */}
      {editItem && (
        <ItemEditModal
          projectId={project.id}
          item={editItem}
          initialProblemTab={editItemInitialTab}
          reviewFix={editItemReviewFix}
          problemLayout={
            project.settings.problem_two_column ? "two_column" : "single_column"
          }
          solutionLayout={
            project.settings.two_column_mode === "none" ? "single_column" : "two_column"
          }
          onClose={() => {
            setEditItem(null);
            setEditItemReviewFix(null);
          }}
          onSaved={async () => {
            setEditItem(null);
            setEditItemReviewFix(null);
            await load();
          }}
        />
      )}

      {graphAssets && (
        <Modal title="グラフを再編集" onClose={() => setGraphAssets(null)}>
          {graphAssets.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--muted)" }}>
              まだ再編集できるグラフがありません。
            </p>
          ) : (
            <div className="space-y-2">
              {graphAssets.map((asset) => (
                <button
                  key={asset.assetId}
                  onClick={() => onReeditGraph(asset)}
                  disabled={graphBusy}
                  className="card w-full px-3 py-2 text-left"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="min-w-0 truncate text-sm font-semibold">
                      {asset.displayName || asset.assetId}
                    </span>
                    <span className="badge badge-muted">v{asset.version}</span>
                  </div>
                  <div className="mt-1 truncate font-mono text-[11px]" style={{ color: "var(--muted)" }}>
                    {asset.assetId}
                  </div>
                  <div className="mt-1 text-[11px]" style={{ color: "var(--muted)" }}>
                    更新: {asset.updatedAt}
                  </div>
                </button>
              ))}
            </div>
          )}
        </Modal>
      )}

      {/* .texプレビューモーダル */}
      {texPreview && (
        <Modal title={`${KIND_LABEL[texPreview.kind]} LaTeXソース`} onClose={() => setTexPreview(null)} wide>
          {project.template_updated && (
            <p className="mb-2 text-xs" style={{ color: "var(--warn)" }}>
              <Icon name="warning" size={14} /> テンプレート本体の更新は未反映です（この教材の保存済みスナップショット版で生成しています）。
              反映するにはテンプレート行の「最新版に更新」を押してください。
            </p>
          )}
          <pre
            className="log-pre max-h-[60vh] overflow-auto rounded p-3"
            style={{ background: "var(--panel-2)", fontSize: 12 }}
          >
            {texPreview.tex}
          </pre>
          <div className="mt-3 flex justify-end gap-2">
            <button
              onClick={() => navigator.clipboard.writeText(texPreview.tex).then(() => showToast("コピーしました"))}
              className="btn btn-ghost"
            >
              クリップボードへコピー
            </button>
            <button onClick={() => onExportTex(texPreview.kind)} className="btn btn-solid">
              .texファイルを書き出す
            </button>
          </div>
        </Modal>
      )}

      {/* コンパイル結果モーダル */}
      {compileResult && (
        <Modal
          title={compileResult.success ? "PDF生成 完了" : "PDF生成 失敗"}
          onClose={() => {
            setCompileResult(null);
            setCompilePreviewUrl(null);
            setShowLog(false);
          }}
          wide={showLog || (!isTauri && compileResult.success)}
        >
          <div className="pdf-result-primary-actions sticky top-0 z-10 mb-3 flex items-center justify-end gap-2 rounded-md border p-2">
            <button onClick={() => setShowLog(!showLog)} className="btn btn-ghost">
              {showLog ? "ログを隠す" : "ログ詳細"}
            </button>
            {compileResult.success && compileResult.pdf_path && (
              <>
                {isTauri && (
                  <button onClick={() => showInFolder(compileResult.pdf_path!)} className="btn btn-ghost">
                    フォルダで表示
                  </button>
                )}
                {isTauri ? (
                  <button onClick={() => openCompiledFile(compileResult.pdf_path!)} className="btn btn-solid">
                    PDFを開く
                  </button>
                ) : (
                  <PdfSaveButton
                    path={compileResult.pdf_path}
                    cacheKey={compileResult.download_key}
                    className="pdf-download-action btn btn-solid"
                    onError={(message) => showToast(message, "error")}
                  />
                )}
              </>
            )}
          </div>
          {!isTauri && compileResult.success && compileResult.pdf_path && (
            <p className="standalone-save-hint mb-3 text-xs">
              表示されたメニューから「“ファイル”に保存」を選んでください
            </p>
          )}
          <p
            className="mb-3 text-sm whitespace-pre-wrap"
            style={{ color: compileResult.success ? "var(--success)" : "var(--danger)" }}
          >
            {compileResult.message}
          </p>
          {compileResult.pdf_path && (
            <p className="mb-3 text-xs break-all" style={{ color: "var(--muted)" }}>
              {compileResult.pdf_path}
            </p>
          )}
          {!isTauri && compileResult.success && compilePreviewUrl && (
            <div className="mb-3">
              <div className="mb-2 flex items-center justify-end gap-1">
                <button className="btn btn-ghost btn-sm" onClick={() => setCompilePreviewZoom((z) => Math.max(50, z - 10))}>－</button>
                <button className="btn btn-ghost btn-sm w-14 justify-center" onClick={() => setCompilePreviewZoom(100)}>{compilePreviewZoom}%</button>
                <button className="btn btn-ghost btn-sm" onClick={() => setCompilePreviewZoom((z) => Math.min(300, z + 10))}>＋</button>
              </div>
              <div className="max-h-[62vh] overflow-auto rounded border p-2" style={{ borderColor: "var(--border)" }}>
                <PdfCanvasViewer src={compilePreviewUrl} zoom={compilePreviewZoom} />
              </div>
            </div>
          )}
          {showLog && (
            <pre
              className="log-pre mb-3 max-h-[45vh] overflow-auto rounded p-3"
              style={{ background: "#080b11" }}
            >
              {compileResult.log || "(ログなし)"}
            </pre>
          )}
        </Modal>
      )}
    </div>
  );
}

/** 出力設定モーダル（テンプレートのプレースホルダと連携する設定値） */
function SettingsModal({
  project,
  onSave,
  onSaveMeta,
  onClose,
}: {
  project: ProjectFull;
  onSave: (s: ProjectSettings) => Promise<boolean>;
  onSaveMeta: (name: string, description: string) => Promise<boolean>;
  onClose: () => void;
}) {
  const [s, setS] = useState<ProjectSettings>({ ...project.settings });
  const [description, setDescription] = useState(project.description);

  const checkRow = (label: string, key: keyof ProjectSettings) => (
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        checked={s[key] as boolean}
        onChange={(e) => setS({ ...s, [key]: e.target.checked })}
      />
      {label}
    </label>
  );
  const textRow = (label: string, key: keyof ProjectSettings, placeholder = "", hint = "") => (
    <div className="flex-1">
      <label className="section-label mb-0.5 block">
        {label}
        {hint && (
          <span className="ml-1 font-normal" style={{ color: "var(--accent)" }}>
            {hint}
          </span>
        )}
      </label>
      <input
        value={s[key] as string}
        onChange={(e) => setS({ ...s, [key]: e.target.value })}
        className="input w-full"
        placeholder={placeholder}
      />
    </div>
  );

  return (
    <Modal title="出力設定" onClose={onClose} wide>
      <div className="space-y-3">
        <div className="flex gap-3">
          {textRow("教材タイトル", "booklet_title", "", "{{TITLE}}")}
          {textRow("副題", "subtitle", "例: 夏期講習 第1回", "{{SUBTITLE}}")}
        </div>
        <div className="flex gap-3">
          {textRow("学年・対象", "target", "例: 高1", "{{TARGET}}")}
          {textRow("日付", "date_str", "例: 2026年7月10日", "{{DATE}}")}
        </div>
        <div className="flex gap-3">
          {textRow("ヘッダー左", "header_left", "未入力なら教材タイトル", "{{HEADER_LEFT}}")}
          {textRow("ヘッダー右", "header_right", "未入力なら日付", "{{HEADER_RIGHT}}")}
        </div>
        <div className="flex gap-3">
          {textRow("問題番号の形式", "number_format", "問題{n} / 第{n}問 / [{n}] など")}
        </div>
        <div className="grid grid-cols-2 gap-1.5">
          {checkRow("教材タイトルを表示する", "show_title")}
          {checkRow("ヘッダーを表示する", "show_header")}
          {checkRow("氏名欄を表示する（問題冊子）", "show_name_field")}
          {checkRow("問題番号を自動で振る", "auto_number")}
          {checkRow("問題ごとに改ページする", "page_break_per_problem")}
          {checkRow("目次を付ける", "show_toc")}
          {checkRow("章・節見出しに番号を振る", "number_headings")}
          {checkRow("章ごとに問題番号をリセット（番号付き章では 2-1 形式）", "reset_numbering_per_chapter")}
          {checkRow("解答冊子に問題文を含める", "include_statement_in_answers")}
          {checkRow("解答冊子の問題文を枠で囲む", "box_statement_in_answers")}
          {checkRow("解答冊子に考え方を含める", "include_explanation")}
        </div>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div>
            <label className="section-label mb-0.5 block">問題冊子の段組（縦線付き）</label>
            <select
              value={s.problem_two_column ? "two_column" : "single_column"}
              onChange={(e) =>
                setS({ ...s, problem_two_column: e.target.value === "two_column" })
              }
              className="select w-full"
            >
              <option value="single_column">一段組（横幅を活かす）</option>
              <option value="two_column">二段組（問題を左右の列へ流す）</option>
            </select>
          </div>
          <div>
          <label className="section-label mb-0.5 block">解答冊子の2段組（縦線付き）</label>
          <select
            value={s.two_column_mode || "none"}
            onChange={(e) => setS({ ...s, two_column_mode: e.target.value })}
            className="select w-full"
          >
            <option value="none">2段組にしない</option>
            <option value="all">問題＋解答全体を2段組にする</option>
            <option value="answer_only">解答部分だけを2段組にする（問題文は1段のまま）</option>
          </select>
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="section-label mb-0.5 block">A/B/C/D表示</label>
            <select
              value={s.difficulty_display || "number_side"}
              onChange={(e) => setS({ ...s, difficulty_display: e.target.value as ProjectSettings["difficulty_display"] })}
              className="select w-full"
            >
              <option value="none">表示しない</option>
              <option value="number_side">問題番号の左</option>
              <option value="top_right">問題の右上</option>
            </select>
          </div>
          <div>
            <label className="section-label mb-0.5 block">★表示</label>
            <select
              value={s.required_display || "required_only"}
              onChange={(e) => setS({ ...s, required_display: e.target.value as ProjectSettings["required_display"] })}
              className="select w-full"
            >
              <option value="none">表示しない</option>
              <option value="required_only">最低限問題だけ表示</option>
            </select>
          </div>
        </div>
        <div>
          <label className="section-label mb-0.5 block">説明・メモ</label>
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            className="input-area h-16 w-full resize-none"
          />
        </div>
        <div className="flex justify-end">
          <button
            onClick={async () => {
              if (!(await onSave(s))) return;
              if (!(await onSaveMeta(project.name, description))) return;
              onClose();
            }}
            className="btn btn-solid"
          >
            保存
          </button>
        </div>
      </div>
    </Modal>
  );
}

/** 項目（見出し/説明文/問題スナップショット）編集モーダル */
function ItemEditModal({
  projectId,
  item,
  initialProblemTab,
  reviewFix,
  problemLayout,
  solutionLayout,
  onClose,
  onSaved,
}: {
  projectId: number;
  item: ProjectItem;
  initialProblemTab: "statement" | "answer" | "explanation";
  reviewFix: {
    field: ProjectReviewFix["field"];
    guidance: string;
    action: ProjectReviewFix["action"];
  } | null;
  problemLayout: "two_column" | "single_column";
  solutionLayout: "two_column" | "single_column";
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { showToast, openGraphOverlay } = useApp();
  const [content, setContent] = useState(item.content);
  const [headingLevel, setHeadingLevel] = useState(item.heading_level);
  const [headingNumbered, setHeadingNumbered] = useState(item.heading_numbered);
  const [title, setTitle] = useState(item.snap_title);
  const [partTitle, setPartTitle] = useState(item.snap_title);
  const [partType, setPartType] = useState(item.snap_part_type || "custom");
  const [partCategory, setPartCategory] = useState(item.snap_part_category);
  const [partDescription, setPartDescription] = useState(item.snap_part_description);
  const [partOutputTarget, setPartOutputTarget] = useState(item.snap_part_output_target || "both");
  const [partLayoutMode, setPartLayoutMode] = useState(item.snap_part_layout_mode || "single_column");
  const [statement, setStatement] = useState(item.snap_statement);
  const [statementTwoColumn, setStatementTwoColumn] = useState(
    item.snap_statement_two_column || item.snap_statement,
  );
  const [statementLayout, setStatementLayout] = useState<
    "single_column" | "two_column"
  >(problemLayout);
  const [answer, setAnswer] = useState(item.snap_answer);
  const [explanation, setExplanation] = useState(item.snap_explanation);
  const [tab, setTab] = useState<"statement" | "answer" | "explanation">(
    initialProblemTab,
  );
  const [graphBusy, setGraphBusy] = useState(false);
  const [showAi, setShowAi] = useState(false);
  const [itemAiPreset, setItemAiPreset] = useState<AiConvertPreset | null>(null);
  const editorRef = useRef<LatexEditorHandle>(null);

  const reviewRevisionPreset = (): AiConvertPreset | null => {
    if (!reviewFix || reviewFix.field === "item") return null;
    if (item.item_type === "problem") {
      const statementSource =
        statementLayout === "two_column" ? statementTwoColumn : statement;
      const target =
        reviewFix.field === "answer" || reviewFix.field === "explanation"
          ? reviewFix.field
          : "statement";
      const targetSource = {
        statement: statementSource,
        answer,
        explanation,
      }[target];
      const text =
        target === "statement"
          ? targetSource
          : `【問題文（参考）】\n${statementSource}\n\n【修正対象の${
              target === "answer" ? "解答" : "解説"
            }】\n${targetSource}`;
      return {
        sourceType: "text",
        text,
        mode: "revise_source",
        title: `AI確認の指摘に基づいて${target === "statement" ? "問題文" : target === "answer" ? "解答" : "解説"}を修正`,
        revisionTarget:
          target === "statement" && statementLayout === "two_column"
            ? "problem_statement_two_column"
            : `problem_${target}`,
        revisionSourceVersion: item.version,
        revisionGuidance: reviewFix.guidance,
        replaceTarget: true,
        solutionLayout: target === "statement" ? problemLayout : solutionLayout,
      };
    }
    return {
      sourceType: "text",
      text: content,
      mode: "revise_source",
      title: "AI確認の指摘に基づいて項目を修正",
      revisionTarget: "part",
      revisionSourceVersion: item.version,
      revisionGuidance: reviewFix.guidance,
      replaceTarget: true,
      solutionLayout,
    };
  };

  useEffect(() => {
    if (reviewFix?.action !== "ai") return;
    const preset = reviewRevisionPreset();
    if (!preset) return;
    setItemAiPreset(preset);
    setShowAi(true);
  }, []);

  const save = async () => {
    try {
      if (item.item_type === "problem") {
        await updateProjectItem(item.id, {
          snap_title: title,
          snap_statement: statement,
          snap_statement_two_column: statementTwoColumn,
          snap_answer: answer,
          snap_explanation: explanation,
          expected_version: item.version,
        });
      } else if (item.item_type === "part") {
        await updateProjectItem(item.id, {
          content,
          snap_title: partTitle,
          snap_part_type: partType,
          snap_part_category: partCategory,
          snap_part_description: partDescription,
          snap_part_output_target: partOutputTarget,
          snap_part_layout_mode: partLayoutMode,
          expected_version: item.version,
        });
      } else {
        await updateProjectItem(item.id, {
          content,
          heading_level: headingLevel,
          heading_numbered: headingNumbered,
          expected_version: item.version,
        });
      }
      await onSaved();
    } catch (e) {
      if (e instanceof ConflictError) {
        showToast(
          "他の端末がこの項目を先に保存しています。編集画面を開き直して最新の内容を確認してください。",
          "error",
        );
        return;
      }
      showToast(String(e), "error");
    }
  };

  const onInsertGraphInItem = async () => {
    const editor = editorRef.current;
    if (!editor || graphBusy) return;
    const start = editor.selectionStart;
    const end = editor.selectionEnd;
    const activeTab = tab;
    const activeStatementLayout = statementLayout;
    const activeTargetField =
      activeTab === "statement" && activeStatementLayout === "two_column"
        ? "project_item_statement_two_column"
        : item.item_type === "problem"
          ? `project_item_${activeTab}`
          : "project_item_content";
    setGraphBusy(true);
    if (!isTauri) {
      try {
        const session = await createGraphWebSession({
          projectId,
          problemId: item.problem_id,
          itemId: item.id,
          targetField: activeTargetField,
          selectionStart: start,
          selectionEnd: end,
        });
        openGraphOverlay(
          session,
          async (result) => {
            if (item.item_type === "problem") {
              if (activeTab === "statement" && activeStatementLayout === "single_column") {
                setStatement((value) =>
                  insertTextAtRange(value, result.insertedLatex, start, end),
                );
              }
              if (activeTab === "statement" && activeStatementLayout === "two_column") {
                setStatementTwoColumn((value) =>
                  insertTextAtRange(value, result.insertedLatex, start, end),
                );
              }
              if (activeTab === "answer") setAnswer((value) => insertTextAtRange(value, result.insertedLatex, start, end));
              if (activeTab === "explanation") setExplanation((value) => insertTextAtRange(value, result.insertedLatex, start, end));
            } else {
              setContent((value) => insertTextAtRange(value, result.insertedLatex, start, end));
            }
            requestAnimationFrame(() => {
              const currentEditor = editorRef.current;
              if (currentEditor) {
                currentEditor.focus();
                const pos = start + result.insertedLatex.length;
                currentEditor.setSelectionRange(pos, pos);
              }
            });
            setGraphBusy(false);
            showToast("グラフを挿入しました");
          },
          () => {
            setGraphBusy(false);
            showToast("グラフ挿入は中止されました");
          },
        );
      } catch (e) {
        setGraphBusy(false);
        showToast(String(e), "error");
      }
      return;
    }
    try {
      const session = await startGraphIntegration({
        projectId,
        problemId: item.problem_id,
        itemId: item.id,
        insertTarget: activeTargetField,
        selectionStart: start,
        selectionEnd: end,
      });
      showToast("グラフ作成アプリを起動しました");
      const result = await waitForGraphIntegration(session);
      if (result.status === "completed" && result.insertedLatex) {
        if (item.item_type === "problem") {
          if (activeTab === "statement" && activeStatementLayout === "single_column") {
            setStatement((v) =>
              insertTextAtRange(v, result.insertedLatex!, start, end),
            );
          }
          if (activeTab === "statement" && activeStatementLayout === "two_column") {
            setStatementTwoColumn((v) =>
              insertTextAtRange(v, result.insertedLatex!, start, end),
            );
          }
          if (activeTab === "answer") setAnswer((v) => insertTextAtRange(v, result.insertedLatex!, start, end));
          if (activeTab === "explanation") setExplanation((v) => insertTextAtRange(v, result.insertedLatex!, start, end));
        } else {
          setContent((v) => insertTextAtRange(v, result.insertedLatex!, start, end));
        }
        requestAnimationFrame(() => {
          editor.focus();
          const pos = start + result.insertedLatex!.length;
          editor.setSelectionRange(pos, pos);
        });
        showToast("グラフを挿入しました");
      } else if (result.status === "cancelled") {
        showToast("グラフ挿入は中止されました");
      } else {
        showToast(result.details ? `${result.message}\n${result.details}` : result.message, "error");
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setGraphBusy(false);
    }
  };

  const reviewFixBanner = reviewFix && (
    <div
      className="mb-3 rounded border p-2 text-xs"
      style={{ borderColor: "rgba(251,191,36,0.5)", background: "var(--warn-dim)" }}
    >
      <div className="flex flex-wrap items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="mb-1 font-semibold" style={{ color: "var(--warn)" }}>
            教材全体のAI確認から開きました
          </p>
          <p className="whitespace-pre-wrap">{reviewFix.guidance}</p>
        </div>
        {reviewFix.field !== "item" && (
          <button
            onClick={() => {
              const preset = reviewRevisionPreset();
              if (!preset) return;
              setItemAiPreset(preset);
              setShowAi(true);
            }}
            className="btn btn-solid btn-sm"
          >
            <Icon name="sparkle" size={14} /> AI修正案
          </button>
        )}
      </div>
    </div>
  );

  if (item.item_type !== "problem") {
    return (
      <Modal title={`${ITEM_TYPE_LABEL[item.item_type]}を編集`} onClose={onClose}>
        {reviewFixBanner}
        {item.item_type === "heading" && (
          <div className="mb-2 space-y-2">
            <div>
              <label className="section-label mb-0.5 block">見出しレベル</label>
              <select
                value={headingLevel}
                onChange={(e) => setHeadingLevel(Number(e.target.value))}
                className="select w-full"
              >
                <option value={1}>章（\section）</option>
                <option value={2}>節（\subsection）</option>
              </select>
            </div>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={headingNumbered}
                onChange={(e) => setHeadingNumbered(e.target.checked)}
              />
              この見出しに番号を振る（出力設定「章・節見出しに番号を振る」がONのとき有効）
            </label>
          </div>
        )}
        {item.item_type === "part" && (
          <div className="mb-3 space-y-2">
            <input
              value={partTitle}
              onChange={(e) => setPartTitle(e.target.value)}
              className="input w-full font-semibold"
              placeholder="部品タイトル"
            />
            <div className="grid grid-cols-2 gap-2">
              <input
                value={partType}
                onChange={(e) => setPartType(e.target.value)}
                className="input"
                placeholder="種類"
              />
              <input
                value={partCategory}
                onChange={(e) => setPartCategory(e.target.value)}
                className="input"
                placeholder="カテゴリ"
              />
            </div>
            <select
              value={partOutputTarget}
              onChange={(e) => setPartOutputTarget(e.target.value as ProjectItem["snap_part_output_target"])}
              className="select w-full"
            >
              <option value="problems">問題冊子に表示</option>
              <option value="answers">解答冊子に表示</option>
              <option value="both">両方に表示</option>
              <option value="none">出力しない</option>
            </select>
            <select
              value={partLayoutMode}
              onChange={(e) => setPartLayoutMode(e.target.value as ProjectItem["snap_part_layout_mode"])}
              className="select w-full"
            >
              <option value="single_column">一段組</option>
              <option value="two_column">二段組</option>
            </select>
            <textarea
              value={partDescription}
              onChange={(e) => setPartDescription(e.target.value)}
              className="input-area h-16 w-full resize-none"
              placeholder="説明・メモ"
            />
          </div>
        )}
        <div className="mb-2 flex justify-end">
          <button onClick={onInsertGraphInItem} disabled={graphBusy} className="btn btn-outline btn-sm">
            {graphBusy ? "グラフ連携中..." : "グラフを挿入"}
          </button>
        </div>
        <div className="mb-2 flex justify-end">
          <button
            onClick={() => {
              setItemAiPreset(null);
              setShowAi(true);
            }}
            className="btn btn-outline btn-sm"
          >
            <Icon name="sparkle" size={15} /> AI変換
          </button>
        </div>
        <LatexEditor
          key={`item-${item.id}-content`}
          ref={editorRef}
          value={content}
          onChange={setContent}
          className="h-32"
        />
        <div className="mt-3 flex justify-end">
          <button onClick={save} className="btn btn-solid">
            保存
          </button>
        </div>
        {showAi && (
          <AiConvertDialog
            onClose={() => {
              setShowAi(false);
              setItemAiPreset(null);
            }}
            preset={itemAiPreset ?? { solutionLayout }}
            insertTargets={[
              {
                label: itemAiPreset?.replaceTarget ? "この項目を置き換える" : "この項目へ挿入",
                entityType: "project_item",
                entityId: item.id,
                field: "content",
                insert: (latex) => {
                  if (itemAiPreset?.replaceTarget) {
                    setContent(latex);
                    return;
                  }
                  const editor = editorRef.current;
                  const start = editor?.selectionStart ?? content.length;
                  const end = editor?.selectionEnd ?? start;
                  setContent((value) => insertTextAtRange(value, latex, start, end));
                },
              },
            ]}
          />
        )}
      </Modal>
    );
  }

  const tabs = { statement: "問題文", answer: "解答", explanation: "解説" } as const;
  const activeStatement =
    statementLayout === "two_column" ? statementTwoColumn : statement;
  const setActiveStatement =
    statementLayout === "two_column" ? setStatementTwoColumn : setStatement;
  const values = { statement: activeStatement, answer, explanation };
  const setters = {
    statement: setActiveStatement,
    answer: setAnswer,
    explanation: setExplanation,
  };
  const aiProblemTab =
    itemAiPreset?.revisionTarget === "problem_statement" ||
    itemAiPreset?.revisionTarget === "problem_statement_two_column"
      ? "statement"
      : itemAiPreset?.revisionTarget === "problem_answer"
        ? "answer"
        : itemAiPreset?.revisionTarget === "problem_explanation"
          ? "explanation"
          : tab;

  return (
    <Modal title="この教材内だけの内容を編集（問題バンクには影響しません）" onClose={onClose} wide>
      {reviewFixBanner}
      <input
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        className="input mb-2 w-full font-semibold"
        placeholder="タイトル"
      />
      <div className="mb-1 flex border-b" style={{ borderColor: "var(--border)" }}>
        {(Object.keys(tabs) as (keyof typeof tabs)[]).map((t) => (
          <button key={t} onClick={() => setTab(t)} className={`tab ${tab === t ? "tab-active" : ""}`}>
            {tabs[t]}
          </button>
        ))}
      </div>
      {tab === "statement" && (
        <div className="mb-2 flex flex-wrap items-center gap-2">
          <span className="text-xs font-semibold" style={{ color: "var(--muted)" }}>
            表示形式
          </span>
          <button
            type="button"
            onClick={() => setStatementLayout("single_column")}
            className={`btn btn-sm ${
              statementLayout === "single_column" ? "btn-solid" : "btn-outline"
            }`}
          >
            一段組用
          </button>
          <button
            type="button"
            onClick={() => setStatementLayout("two_column")}
            className={`btn btn-sm ${
              statementLayout === "two_column" ? "btn-solid" : "btn-outline"
            }`}
          >
            二段組用
          </button>
          <span className="text-xs" style={{ color: "var(--muted)" }}>
            冊子の段組に応じて自動選択されます
          </span>
        </div>
      )}
      <div className="mb-2 flex justify-end">
        <button onClick={onInsertGraphInItem} disabled={graphBusy} className="btn btn-outline btn-sm">
          {graphBusy ? "グラフ連携中..." : "グラフを挿入"}
        </button>
      </div>
      <div className="mb-2 flex flex-wrap justify-end gap-2">
        {tab === "statement" && (
          <>
            <button
              onClick={() => {
                const isTwoColumn = statementLayout === "two_column";
                const layoutName = isTwoColumn ? "二段組の片方の列" : "一段組";
                setItemAiPreset({
                  sourceType: "text",
                  text: activeStatement,
                  mode: "revise_source",
                  title: `問題文を${isTwoColumn ? "二段組" : "一段組"}向けに整形`,
                  revisionTarget: isTwoColumn
                    ? "problem_statement_two_column"
                    : "problem_statement",
                  revisionSourceVersion: item.version,
                  revisionGuidance: `問題の条件、数値、記号、点名、小問番号、選択肢、図表との対応を一切変えず、${layoutName}の\\linewidthで読みやすくなるよう、文章と長い数式の改行だけを調整してください。外側の段組環境は追加しないでください。`,
                  replaceTarget: true,
                  solutionLayout: statementLayout,
                });
                setShowAi(true);
              }}
              disabled={!activeStatement.trim()}
              className="btn btn-outline btn-sm"
            >
              <Icon name="sparkle" size={15} /> この表示形式を整形
            </button>
            <button
              onClick={() => {
                setItemAiPreset({
                  sourceType: "text",
                  text: statement.trim() || statementTwoColumn.trim(),
                  mode: "generate_problem_layouts",
                  title: "問題文の一段組用・二段組用を生成",
                  solutionLayout: "single_column",
                });
                setShowAi(true);
              }}
              disabled={!statement.trim() && !statementTwoColumn.trim()}
              className="btn btn-outline btn-sm"
            >
              <Icon name="sparkle" size={15} /> 両方の表示形式を生成
            </button>
          </>
        )}
        <button
          onClick={() => {
            setItemAiPreset(null);
            setShowAi(true);
          }}
          className="btn btn-outline btn-sm"
        >
          <Icon name="sparkle" size={15} /> AI変換
        </button>
      </div>
      <LatexEditor
        key={`item-${item.id}-${tab}-${statementLayout}`}
        ref={editorRef}
        value={values[tab]}
        onChange={setters[tab]}
        className="h-56"
      />
      <div className="mt-3 flex justify-end">
        <button onClick={save} className="btn btn-solid">
          保存
        </button>
      </div>
      {showAi && (
        <AiConvertDialog
          onClose={() => {
            setShowAi(false);
            setItemAiPreset(null);
          }}
          preset={itemAiPreset ?? {
            solutionLayout: tab === "statement" ? statementLayout : solutionLayout,
          }}
          onUseProblemLayouts={
            itemAiPreset?.mode === "generate_problem_layouts"
              ? (layouts) => {
                  setStatement(layouts.statementLatex);
                  setStatementTwoColumn(layouts.statementLatexTwoColumn);
                  setStatementLayout("single_column");
                  setTab("statement");
                }
              : undefined
          }
          insertTargets={[
            {
              label: itemAiPreset?.replaceTarget
                ? `${tabs[aiProblemTab]}を置き換える`
                : `${tabs[aiProblemTab]}へ挿入`,
              entityType: "project_item",
              entityId: item.id,
              field:
                aiProblemTab === "statement" && statementLayout === "two_column"
                  ? "snap_statement_two_column"
                  : `snap_${aiProblemTab}`,
              insert: (latex) => {
                if (itemAiPreset?.replaceTarget) {
                  setters[aiProblemTab](latex);
                  setTab(aiProblemTab);
                  return;
                }
                const editor = editorRef.current;
                const value = values[aiProblemTab];
                const start = editor?.selectionStart ?? value.length;
                const end = editor?.selectionEnd ?? start;
                setters[aiProblemTab]((current) => insertTextAtRange(current, latex, start, end));
              },
            },
          ]}
        />
      )}
    </Modal>
  );
}
