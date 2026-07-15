import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  addAttachment,
  compileProblemPreview,
  createGraphWebSession,
  ensureGraphFromAsset,
  createProblem,
  deleteProblem,
  duplicateProblem,
  getProblem,
  getVersion,
  listGraphAssets,
  listVersions,
  removeAttachment,
  restoreVersion,
  updateProblem,
  uploadAttachment,
} from "../api";
import { insertTextAtRange } from "../graphIntegration";
import { moveProblems } from "../api";
import { useApp } from "../store";
import { compiledPdfUrl, ConflictError, isTauri, revokeIfBlobUrl } from "../transport";
import type { GraphAssetSummary, ProblemFull, VersionFull, VersionSummary } from "../types";
import { AiConvertDialog } from "./AiConvertDialog";
import { ConflictDialog } from "./ConflictDialog";
import { LatexEditor, type LatexEditorHandle } from "./LatexEditor";
import { LatexPreview } from "./LatexPreview";
import { PdfCanvasViewer } from "./PdfCanvasViewer";
import { Icon } from "./Icon";
import { UnitPicker } from "./ProblemList";
import { DIFFICULTY_RANKS, DifficultyRankBadge, Modal } from "./ui";

type Tab = "statement" | "answer" | "explanation";

const TAB_LABELS: Record<Tab, string> = {
  statement: "問題文",
  answer: "解答",
  explanation: "解説",
};

const SNIPPETS: { label: string; text: string; cursorOffset?: number }[] = [
  { label: "\\frac", text: "\\frac{}{}", cursorOffset: 6 },
  { label: "\\sqrt", text: "\\sqrt{}", cursorOffset: 6 },
  { label: "x^2", text: "^{}", cursorOffset: 2 },
  { label: "x_i", text: "_{}", cursorOffset: 2 },
  { label: "$...$", text: "$$", cursorOffset: 1 },
  { label: "\\[...\\]", text: "\\[  \\]", cursorOffset: 3 },
  { label: "enumerate", text: "\\begin{enumerate}\n\\item \n\\end{enumerate}", cursorOffset: 24 },
  { label: "itemize", text: "\\begin{itemize}\n\\item \n\\end{itemize}", cursorOffset: 22 },
  { label: "align*", text: "\\begin{align*}\n \n\\end{align*}", cursorOffset: 15 },
  { label: "cases", text: "\\begin{cases}\n \n\\end{cases}", cursorOffset: 14 },
  { label: "\\leqq", text: "\\leqq " },
  { label: "\\geqq", text: "\\geqq " },
  { label: "\\pi", text: "\\pi " },
  { label: "\\theta", text: "\\theta " },
  {
    label: "図[中央]",
    text: "\n\\begin{center}\n\\includegraphics[width=6cm]{}\n\\end{center}\n",
    cursorOffset: "\n\\begin{center}\n\\includegraphics[width=6cm]{".length,
  },
  {
    label: "図[H]",
    text: "\n\\begin{figure}[H]\n\\centering\n\\includegraphics[width=6cm]{}\n\\caption{}\n\\end{figure}\n",
    cursorOffset: "\n\\begin{figure}[H]\n\\centering\n\\includegraphics[width=6cm]{".length,
  },
];

// 図の幅の候補
const IMG_WIDTHS = ["4cm", "6cm", "8cm", "10cm", "12cm", "\\linewidth"];

/** カーソル位置に確実に留まる中央寄せ図ブロック（フロートではないので位置がずれない） */
function centerFigureSnippet(stored: string, width: string): string {
  return `\n\\begin{center}\n\\includegraphics[width=${width}]{${stored}}\n\\end{center}\n`;
}

/** figure環境 [H] 指定（float パッケージが必要。番号・キャプション付き） */
function floatFigureSnippet(stored: string, width: string): string {
  return `\n\\begin{figure}[H]\n\\centering\n\\includegraphics[width=${width}]{${stored}}\n\\caption{}\n\\end{figure}\n`;
}

function clearProblemDraft(problemId: number): void {
  try {
    localStorage.removeItem(`kk-draft-problem-${problemId}`);
  } catch {
    // 保存本体の成功をlocalStorageの利用可否で失敗扱いにしない。
  }
}

/** 問題編集画面（中央エディタ + 右プレビュー） */
export function ProblemEditor() {
  const {
    selectedProblemId,
    selectProblem,
    refreshTree,
    showToast,
    confirm,
    setDirty,
    dirty,
    bumps,
    setLastCompile,
    setLogOpen,
    openGraphOverlay,
  } = useApp();

  const [problem, setProblem] = useState<ProblemFull | null>(null);
  const [tab, setTab] = useState<Tab>("statement");
  const [tagInput, setTagInput] = useState("");
  const [versions, setVersions] = useState<VersionSummary[] | null>(null);
  const [versionView, setVersionView] = useState<VersionFull | null>(null);
  const [showMove, setShowMove] = useState(false);
  const [imgWidth, setImgWidth] = useState("6cm");
  const [previewMode, setPreviewMode] = useState<"quick" | "pdf">("quick");
  const [pdfSrc, setPdfSrc] = useState<string | null>(null);
  const [pdfBusy, setPdfBusy] = useState(false);
  const [graphBusy, setGraphBusy] = useState(false);
  const [graphAssets, setGraphAssets] = useState<GraphAssetSummary[] | null>(null);
  const [zoom, setZoom] = useState(100);
  const [showAi, setShowAi] = useState(false);
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState<ProblemFull | null>(null);
  const previewBoxRef = useRef<HTMLDivElement>(null);
  const webFileInputRef = useRef<HTMLInputElement>(null);

  const changeZoom = (delta: number) => {
    setZoom((z) => Math.min(300, Math.max(50, z + delta)));
  };

  // Ctrl+ホイールで拡大縮小（ブラウザ既定のズームを抑止するため非passiveで登録）
  useEffect(() => {
    const el = previewBoxRef.current;
    if (!el) return;
    const handler = (e: WheelEvent) => {
      if (e.ctrlKey) {
        e.preventDefault();
        changeZoom(e.deltaY < 0 ? 10 : -10);
      }
    };
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, [problem != null]);
  const textareaRef = useRef<LatexEditorHandle>(null);
  const problemRef = useRef<ProblemFull | null>(null);
  problemRef.current = problem;
  const seenProblemsBumpRef = useRef(bumps.problems);
  const pendingProblemsRefreshRef = useRef(false);
  const problemLoadRequestRef = useRef(0);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const savingRef = useRef(false);

  // アンマウント時（別画面・別問題への遷移後）に未保存バッジが残らないようにする
  useEffect(() => {
    return () => setDirty(false);
  }, []);

  const load = async (preserveDirty = false) => {
    if (selectedProblemId == null) return;
    const requestId = ++problemLoadRequestRef.current;
    try {
      const p = await getProblem(selectedProblemId);
      if (requestId !== problemLoadRequestRef.current) return;
      if (preserveDirty && dirtyRef.current) {
        pendingProblemsRefreshRef.current = true;
        return;
      }
      setProblem(p);
      setDirty(false);
      setTab("statement");
      setPdfSrc((prev) => {
        revokeIfBlobUrl(prev);
        return null;
      });
      setPreviewMode("quick");
      // 通信断時に端末へ退避した未送信ドラフトの復元提案
      const draftKey = `kk-draft-problem-${p.id}`;
      const raw = localStorage.getItem(draftKey);
      if (raw) {
        try {
          const draft = JSON.parse(raw) as { savedAt: number; problem: ProblemFull };
          const shouldRestore =
            !!draft.problem &&
            (await confirm(
              "この端末に未送信の編集内容が残っています（通信エラー時の退避データ）。\n復元しますか？\n「キャンセル」で破棄します。",
            ));
          if (requestId !== problemLoadRequestRef.current) return;
          if (shouldRestore) {
            // ドラフトが作られた時点のversionを維持する。ここをサーバー最新版へ
            // 差し替えると、復元後の保存が他端末の変更を競合なしで上書きしてしまう。
            const draftVersion = Number.isFinite(draft.problem.version) ? draft.problem.version : -1;
            setProblem({ ...draft.problem, version: draftVersion });
            setDirty(true);
          } else {
            clearProblemDraft(p.id);
          }
        } catch {
          clearProblemDraft(p.id);
        }
      }
    } catch (e) {
      if (requestId === problemLoadRequestRef.current) showToast(String(e), "error");
    }
  };

  /** この問題だけをuplatexでコンパイルして右パネルにPDF表示する */
  const onPdfPreview = async () => {
    const p = problemRef.current;
    if (!p) return;
    setPdfBusy(true);
    try {
      const result = await compileProblemPreview(
        p.id,
        p.statement_latex,
        p.answer_latex,
        p.explanation_latex,
      );
      setLastCompile({ ...result, label: `問題プレビュー「${p.title}」` });
      if (result.success && result.pdf_path) {
        const url = await compiledPdfUrl(result.pdf_path, Date.now());
        setPdfSrc((prev) => {
          revokeIfBlobUrl(prev);
          return url;
        });
        setPreviewMode("pdf");
      } else {
        setLogOpen(true);
        showToast(result.message, "error");
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setPdfBusy(false);
    }
  };

  useEffect(() => {
    load();
  }, [selectedProblemId]);

  // 他端末更新は編集中の内容を上書きしない。保存・破棄でdirtyが解消した時点で追随する。
  useEffect(() => {
    if (seenProblemsBumpRef.current === bumps.problems) return;
    seenProblemsBumpRef.current = bumps.problems;
    if (dirty) {
      pendingProblemsRefreshRef.current = true;
      return;
    }
    void load(true);
  }, [bumps.problems]);

  useEffect(() => {
    if (dirty || !pendingProblemsRefreshRef.current) return;
    pendingProblemsRefreshRef.current = false;
    void load(true);
  }, [dirty]);

  const patch = (fields: Partial<ProblemFull>) => {
    setProblem((p) => (p ? { ...p, ...fields } : p));
    setDirty(true);
  };

  /** expectedVersion=undefined なら現在のversionで競合チェック、nullなら強制上書き */
  const save = async (forceOverwrite = false) => {
    const p = problemRef.current;
    // 連打・Ctrl+S連打で同じversionの保存が2回飛ぶと、2回目が自分自身と
    // 競合して偽の競合ダイアログが出るため、実行中は多重保存しない。
    if (!p || savingRef.current) return;
    savingRef.current = true;
    setSaving(true);
    try {
      const newVersion = await updateProblem({
        id: p.id,
        unit_id: p.unit_id,
        title: p.title,
        statement_latex: p.statement_latex,
        answer_latex: p.answer_latex,
        explanation_latex: p.explanation_latex,
        difficulty: p.difficulty,
        difficulty_rank: p.difficulty_rank,
        is_required: p.is_required,
        memo: p.memo,
        tags: p.tags,
        expected_version: forceOverwrite ? null : p.version,
      });
      setProblem((prev) => (prev ? { ...prev, version: newVersion } : prev));
      setDirty(false);
      clearProblemDraft(p.id);
      showToast("保存しました");
    } catch (e) {
      if (e instanceof ConflictError) {
        // 他端末が先に保存 → サーバー版を取得して解決ダイアログへ
        try {
          setConflict(await getProblem(p.id));
        } catch {
          showToast("競合を検出しましたが、サーバー版を取得できませんでした", "error");
        }
        return;
      }
      // 通信断などの場合に備えて端末内へ一時保存（保存済みとは表示しない）
      try {
        localStorage.setItem(
          `kk-draft-problem-${p.id}`,
          JSON.stringify({ savedAt: Date.now(), problem: p }),
        );
      } catch {
        /* localStorage不可なら諦める */
      }
      showToast(`${String(e)}\n（編集内容はこの端末に一時保存されています）`, "error");
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  /** 競合解決 */
  const resolveConflict = async (choice: "server" | "mine" | "copy") => {
    const server = conflict;
    const mine = problemRef.current;
    setConflict(null);
    if (!server || !mine) return;
    if (choice === "server") {
      setProblem(server);
      setDirty(false);
      clearProblemDraft(mine.id);
      showToast("サーバー版を読み込みました");
    } else if (choice === "mine") {
      await save(true);
    } else {
      // 自分の変更をコピーとして保存し、エディタはサーバー版へ
      try {
        const newId = await createProblem(mine.unit_id, `${mine.title} (競合コピー)`);
        await updateProblem({
          id: newId,
          unit_id: mine.unit_id,
          title: `${mine.title} (競合コピー)`,
          statement_latex: mine.statement_latex,
          answer_latex: mine.answer_latex,
          explanation_latex: mine.explanation_latex,
          difficulty: mine.difficulty,
          difficulty_rank: mine.difficulty_rank,
          is_required: mine.is_required,
          memo: mine.memo,
          tags: mine.tags,
        });
        await refreshTree();
        setProblem(server);
        setDirty(false);
        clearProblemDraft(mine.id);
        showToast("自分の変更を「(競合コピー)」として保存しました");
      } catch (e) {
        showToast(String(e), "error");
      }
    }
  };

  // Ctrl+S で保存
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        save();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  if (selectedProblemId == null || problem == null) return null;

  const fieldKey: Record<Tab, "statement_latex" | "answer_latex" | "explanation_latex"> = {
    statement: "statement_latex",
    answer: "answer_latex",
    explanation: "explanation_latex",
  };
  const currentText = problem[fieldKey[tab]];

  const insertSnippet = (text: string, cursorOffset?: number) => {
    const ta = textareaRef.current;
    if (!ta) return;
    const start = ta.selectionStart;
    const end = ta.selectionEnd;
    const newValue = currentText.slice(0, start) + text + currentText.slice(end);
    patch({ [fieldKey[tab]]: newValue } as Partial<ProblemFull>);
    requestAnimationFrame(() => {
      ta.focus();
      const pos = start + (cursorOffset ?? text.length);
      ta.setSelectionRange(pos, pos);
    });
  };

  const onInsertGraph = async () => {
    const p = problemRef.current;
    const editor = textareaRef.current;
    if (!p || !editor || graphBusy) return;
    const targetTab = tab;
    const targetField = fieldKey[targetTab];
    const start = editor.selectionStart;
    const end = editor.selectionEnd;
    setGraphBusy(true);
    try {
      const session = await createGraphWebSession({
        problemId: p.id,
        targetField: `problem_${targetTab}`,
        selectionStart: start,
        selectionEnd: end,
      });
      openGraphOverlay(
        session,
        async (result) => {
          const latest = problemRef.current;
          if (latest) {
            const nextText = insertTextAtRange(latest[targetField], result.insertedLatex, start, end);
            patch({ [targetField]: nextText } as Partial<ProblemFull>);
            requestAnimationFrame(() => {
              const currentEditor = textareaRef.current;
              if (tab === targetTab && currentEditor) {
                currentEditor.focus();
                const pos = start + result.insertedLatex.length;
                currentEditor.setSelectionRange(pos, pos);
              }
            });
            showToast("2D・3Dグラフを問題へ挿入しました");
          }
          setGraphBusy(false);
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
  };

  const openGraphReeditList = async () => {
    try {
      setGraphAssets(await listGraphAssets(null, problem.id));
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onReeditGraph = async (asset: GraphAssetSummary) => {
    const latest = problemRef.current;
    if (!latest || graphBusy || !asset.insertedLatex) return;
    const fields = ["statement_latex", "answer_latex", "explanation_latex"] as const;
    const target = fields.find((field) => latest[field].includes(asset.insertedLatex));
    if (!target) {
      showToast("問題内の挿入位置を特定できません。グラフ一覧から複製して挿入してください。", "error");
      return;
    }
    const start = latest[target].indexOf(asset.insertedLatex);
    const end = start + asset.insertedLatex.length;
    const targetTab = target.replace("_latex", "") as Tab;
    setGraphBusy(true);
    try {
      const graphId = await ensureGraphFromAsset(asset.assetId);
      const session = await createGraphWebSession({
        problemId: latest.id,
        targetField: `problem_${targetTab}`,
        selectionStart: start,
        selectionEnd: end,
      });
      openGraphOverlay(
        session,
        async (result) => {
          const current = problemRef.current;
          if (current) {
            patch({ [target]: insertTextAtRange(current[target], result.insertedLatex, start, end) } as Partial<ProblemFull>);
            setTab(targetTab);
          }
          setGraphAssets(null);
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
  };

  const addTag = () => {
    const t = tagInput.trim();
    if (!t) return;
    if (!problem.tags.includes(t)) patch({ tags: [...problem.tags, t] });
    setTagInput("");
  };

  /** 未保存の変更を破棄する操作の前に確認する */
  const confirmDiscard = async (message: string): Promise<boolean> => {
    if (!dirtyRef.current) return true;
    return confirm(message);
  };

  const onBackToList = async () => {
    if (!(await confirmDiscard("未保存の変更があります。保存せずに問題一覧へ戻りますか？"))) return;
    setDirty(false);
    selectProblem(null);
  };

  const onDuplicate = async () => {
    if (!(await confirmDiscard("未保存の変更があります。保存せずに複製を開きますか？\n（複製には保存済みの内容が使われます）"))) return;
    try {
      const newId = await duplicateProblem(problem.id);
      await refreshTree();
      selectProblem(newId);
      showToast("複製しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDelete = async () => {
    if (!(await confirm(`問題「${problem.title}」を削除しますか？`))) return;
    try {
      await deleteProblem(problem.id);
      clearProblemDraft(problem.id);
      await refreshTree();
      selectProblem(null);
      setDirty(false);
      showToast("削除しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const openHistory = async () => {
    try {
      setVersions(await listVersions(problem.id));
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRestore = async (versionId: number) => {
    if (!(await confirm("この履歴の内容に戻しますか？\n（現在の内容も履歴として保存されます）"))) return;
    try {
      await restoreVersion(versionId);
      setVersions(null);
      setVersionView(null);
      await load();
      showToast("履歴から復元しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onAddAttachment = async () => {
    try {
      if (isTauri) {
        const file = await openDialog({
          multiple: false,
          filters: [{ name: "画像・PDF", extensions: ["png", "jpg", "jpeg", "pdf"] }],
        });
        if (!file) return;
        await addAttachment(problem.id, file as string);
      } else {
        webFileInputRef.current?.click();
        return;
      }
      const p = await getProblem(problem.id);
      setProblem((prev) => (prev ? { ...prev, attachments: p.attachments } : prev));
      showToast("画像を添付しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  /** Web版: <input type="file"> からのアップロード */
  const onWebFilePicked = async (files: FileList | null) => {
    if (!files || files.length === 0 || !problemRef.current) return;
    try {
      for (const f of Array.from(files)) {
        await uploadAttachment(problemRef.current.id, f);
      }
      const p = await getProblem(problemRef.current.id);
      setProblem((prev) => (prev ? { ...prev, attachments: p.attachments } : prev));
      showToast("画像を添付しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRemoveAttachment = async (id: number) => {
    if (!(await confirm("この添付を問題から外しますか？"))) return;
    try {
      await removeAttachment(id);
      setProblem((prev) =>
        prev ? { ...prev, attachments: prev.attachments.filter((a) => a.id !== id) } : prev,
      );
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  return (
    <div className="problem-editor editor-split flex h-full min-w-0">
      {/* 中央: 編集 */}
      <div className="flex min-w-0 flex-1 flex-col border-r" style={{ borderColor: "var(--border)" }}>
        <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
          <button onClick={() => void onBackToList()} className="btn btn-ghost btn-sm" title="問題一覧へ戻る">
            ← 一覧
          </button>
          <input
            value={problem.title}
            onChange={(e) => patch({ title: e.target.value })}
            className="input min-w-48 flex-[1_1_18rem] font-semibold"
            placeholder="問題タイトル"
          />
          <select
            value={problem.difficulty}
            onChange={(e) => patch({ difficulty: e.target.value })}
            className="select shrink-0"
          >
            <option>基礎</option>
            <option>標準</option>
            <option>発展</option>
          </select>
          <DifficultyRankBadge rank={problem.difficulty_rank} required={problem.is_required} />
          <span className="flex min-w-0 flex-wrap items-center gap-1" role="group" aria-label="A/B/C/D難易度">
            {DIFFICULTY_RANKS.map((r) => (
              <button
                key={r.rank}
                type="button"
                onClick={() => patch({ difficulty_rank: r.rank })}
                className={`btn btn-sm ${problem.difficulty_rank === r.rank ? "btn-outline" : "btn-ghost"}`}
                title={`${r.rank}: ${r.description}`}
                aria-pressed={problem.difficulty_rank === r.rank}
              >
                {r.rank} {r.label}
              </button>
            ))}
            <button
              type="button"
              onClick={() => patch({ difficulty_rank: null })}
              className="btn btn-ghost btn-sm"
              title="A/B/C/Dを未設定に戻す"
            >
              未設定
            </button>
          </span>
          <label className="flex items-center gap-1 text-xs whitespace-nowrap" style={{ color: "var(--muted)" }}>
            <input
              type="checkbox"
              checked={problem.is_required}
              onChange={(e) => patch({ is_required: e.target.checked })}
            />
            ★ 最低限
          </label>
          {dirty && (
            <span className="badge badge-warn" title="保存されていない変更があります">
              ● 未保存
            </span>
          )}
          <button onClick={() => save()} disabled={saving} className="btn btn-solid shrink-0">
            {saving ? "保存中..." : "保存 (Ctrl+S)"}
          </button>
        </div>

        {/* タグ・操作行 */}
        <div
          className="flex flex-wrap items-center gap-2 border-b px-3 py-1.5"
          style={{ borderColor: "var(--border)" }}
        >
          <span className="section-label">タグ</span>
          {problem.tags.map((t) => (
            <span key={t} className="chip">
              {t}
              <button
                onClick={() => patch({ tags: problem.tags.filter((x) => x !== t) })}
                className="opacity-60 hover:opacity-100"
              >
                ✕
              </button>
            </span>
          ))}
          <input
            value={tagInput}
            onChange={(e) => setTagInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addTag();
              }
            }}
            className="input w-28 px-1.5 py-0.5 text-xs"
            placeholder="タグ追加→Enter"
          />
          <span className="problem-editor-actions ml-auto flex gap-1">
            <button onClick={() => setShowMove(true)} className="btn btn-ghost btn-sm" title="別の単元へ移動">
              移動
            </button>
            <button onClick={openHistory} className="btn btn-ghost btn-sm">
              履歴
            </button>
            <button onClick={onDuplicate} className="btn btn-ghost btn-sm">
              複製
            </button>
            <button onClick={onDelete} className="btn btn-danger btn-sm">
              削除
            </button>
          </span>
        </div>

        {/* タブ */}
        <div className="flex border-b" style={{ borderColor: "var(--border)" }}>
          {(Object.keys(TAB_LABELS) as Tab[]).map((t) => (
            <button key={t} onClick={() => setTab(t)} className={`tab ${tab === t ? "tab-active" : ""}`}>
              {TAB_LABELS[t]}
            </button>
          ))}
        </div>

        {/* 挿入補助 */}
        <div
          className="flex flex-wrap gap-1 border-b px-2 py-1"
          style={{ borderColor: "var(--border)" }}
        >
          {SNIPPETS.map((s) => (
            <button
              key={s.label}
              onClick={() => insertSnippet(s.text, s.cursorOffset)}
              className="rounded border px-1.5 py-0.5 font-mono text-[11px] transition-colors"
              style={{ borderColor: "var(--border)", color: "var(--muted)", background: "var(--panel-2)" }}
              onMouseEnter={(e) => (e.currentTarget.style.color = "var(--accent)")}
              onMouseLeave={(e) => (e.currentTarget.style.color = "var(--muted)")}
              title={s.text}
            >
              {s.label}
            </button>
          ))}
          <button onClick={onInsertGraph} disabled={graphBusy} className="btn btn-outline btn-sm">
            {graphBusy ? "グラフ連携中..." : "2D・3Dグラフを挿入"}
          </button>
          <button onClick={openGraphReeditList} disabled={graphBusy} className="btn btn-ghost btn-sm">
            グラフを再編集
          </button>
          <button
            onClick={() => setShowAi(true)}
            className="btn btn-outline btn-sm"
            title="写真やテキストをAIでLaTeXへ変換して挿入"
            style={{ borderColor: "rgba(157,108,242,0.52)", color: "var(--purple)", background: "var(--purple-dim)" }}
          >
            <Icon name="sparkle" size={15} /> AI変換
          </button>
        </div>

        <div className="min-h-0 flex-1 p-2">
          <LatexEditor
            key={`${problem.id}-${tab}`}
            ref={textareaRef}
            value={currentText}
            onChange={(v) => patch({ [fieldKey[tab]]: v } as Partial<ProblemFull>)}
            className="h-full"
            placeholder={`${TAB_LABELS[tab]}のLaTeXソースを入力（既存のソースを貼り付け可能）`}
          />
        </div>

        {/* 添付・メモ */}
        <div className="border-t px-3 py-2" style={{ borderColor: "var(--border)" }}>
          <div className="attachment-toolbar mb-1 flex items-center gap-2">
            <span className="section-label">添付画像 (PNG/JPG/PDF)</span>
            <button onClick={onAddAttachment} className="btn btn-ghost btn-sm">
              ＋追加
            </button>
            <input
              ref={webFileInputRef}
              type="file"
              accept="image/png,image/jpeg,image/webp,application/pdf"
              multiple
              className="hidden"
              onChange={(e) => {
                onWebFilePicked(e.target.files);
                e.target.value = "";
              }}
            />
            {problem.attachments.length > 0 && (
              <span className="ml-auto flex items-center gap-1">
                <span style={{ color: "var(--muted)" }}>幅</span>
                <select
                  value={imgWidth}
                  onChange={(e) => setImgWidth(e.target.value)}
                  className="select px-1 py-0.5 text-xs"
                  title="挿入する図の幅"
                >
                  {IMG_WIDTHS.map((w) => (
                    <option key={w} value={w}>
                      {w === "\\linewidth" ? "幅いっぱい" : w}
                    </option>
                  ))}
                </select>
              </span>
            )}
          </div>
          {problem.attachments.length > 0 && (
            <ul className="mb-1 space-y-0.5">
              {problem.attachments.map((a) => (
                <li key={a.id} className="attachment-row flex items-center gap-2 text-xs" style={{ color: "var(--muted)" }}>
                  <span className="truncate">{a.file_name}</span>
                  <code
                    className="rounded px-1"
                    style={{ background: "var(--panel-3)", color: "var(--accent)" }}
                  >
                    {a.stored_name}
                  </code>
                  <button
                    onClick={() => insertSnippet(centerFigureSnippet(a.stored_name, imgWidth))}
                    className="btn btn-outline btn-sm"
                    title="カーソル位置に中央寄せの図を挿入（フロートしないので指定位置に確実に入ります）"
                  >
                    図を挿入
                  </button>
                  <button
                    onClick={() => insertSnippet(floatFigureSnippet(a.stored_name, imgWidth))}
                    className="btn btn-ghost btn-sm"
                    title="番号・キャプション付きの figure[H] を挿入（[H]で位置固定・float パッケージが必要）"
                  >
                    番号付き
                  </button>
                  <button onClick={() => onRemoveAttachment(a.id)} className="btn btn-danger btn-sm">
                    ✕
                  </button>
                </li>
              ))}
            </ul>
          )}
          <input
            value={problem.memo}
            onChange={(e) => patch({ memo: e.target.value })}
            className="input w-full text-xs"
            placeholder="メモ"
          />
        </div>
      </div>

      {/* 右: プレビュー（白い紙 = PDF出力イメージ） */}
      <div className="editor-preview-pane flex w-[38%] min-w-[280px] flex-col" style={{ background: "var(--panel)" }}>
        <div
          className="flex flex-wrap items-center gap-1.5 border-b px-3 py-1.5"
          style={{ borderColor: "var(--border)" }}
        >
          <span className="section-label mr-auto min-w-0 truncate">
            プレビュー{previewMode === "quick" ? `（${TAB_LABELS[tab]}・簡易）` : "（PDF）"}
          </span>
          <button
            onClick={() => setPreviewMode("quick")}
            className={`btn btn-sm ${previewMode === "quick" ? "btn-outline" : "btn-ghost"}`}
          >
            簡易
          </button>
          <button
            onClick={() => (pdfSrc ? setPreviewMode("pdf") : onPdfPreview())}
            className={`btn btn-sm ${previewMode === "pdf" ? "btn-outline" : "btn-ghost"}`}
            disabled={pdfBusy}
          >
            PDF
          </button>
          <button
            onClick={onPdfPreview}
            disabled={pdfBusy}
            className="btn btn-solid btn-sm"
            title="この問題だけをuplatexでコンパイルしてPDF表示（保存不要・編集中の内容で生成）"
          >
            {pdfBusy ? "生成中..." : <><Icon name="play" size={15} /> コンパイル</>}
          </button>
          <span className="mx-0.5 h-4 w-px" style={{ background: "var(--border)" }} />
          <button onClick={() => changeZoom(-10)} className="btn btn-ghost btn-sm" title="縮小 (Ctrl+ホイール下)">
            －
          </button>
          <button
            onClick={() => setZoom(100)}
            className="btn btn-ghost btn-sm w-12 justify-center font-mono"
            title="クリックで100%に戻す"
          >
            {zoom}%
          </button>
          <button onClick={() => changeZoom(10)} className="btn btn-ghost btn-sm" title="拡大 (Ctrl+ホイール上)">
            ＋
          </button>
        </div>
        <div ref={previewBoxRef} className="min-h-0 flex-1 overflow-auto p-3">
          {previewMode === "quick" ? (
            <div className="paper" style={{ zoom: zoom / 100 }}>
              <LatexPreview source={currentText} />
            </div>
          ) : pdfSrc ? (
            <PdfCanvasViewer src={pdfSrc} zoom={zoom} />
          ) : (
            <p className="p-4 text-center text-xs" style={{ color: "var(--muted)" }}>
              「コンパイル」でこの問題だけのPDFプレビューを生成します
            </p>
          )}
        </div>
        <div
          className="border-t px-3 py-1.5 text-[11px]"
          style={{ borderColor: "var(--border)", color: "var(--muted)" }}
        >
          更新: {problem.updated_at}　作成: {problem.created_at}
        </div>
      </div>

      {/* AI変換ダイアログ */}
      {showAi && (
        <AiConvertDialog
          onClose={() => setShowAi(false)}
          insertTargets={(["statement", "answer", "explanation"] as Tab[]).map((t) => ({
            label: TAB_LABELS[t],
            field: fieldKey[t],
            entityType: "problem",
            entityId: problem.id,
            insert: (latexText: string) => {
              const latest = problemRef.current;
              if (!latest) return;
              const key = fieldKey[t];
              if (t === tab && textareaRef.current) {
                // 現在表示中のタブならカーソル位置へ挿入
                const ta = textareaRef.current;
                const start = ta.selectionStart;
                const end = ta.selectionEnd;
                patch({ [key]: insertTextAtRange(latest[key], latexText, start, end) } as Partial<ProblemFull>);
              } else {
                // 別タブなら末尾に追記してそのタブへ切り替え
                const base = latest[key];
                patch({ [key]: base ? `${base}\n${latexText}` : latexText } as Partial<ProblemFull>);
                setTab(t);
              }
            },
          }))}
        />
      )}

      {graphAssets && (
        <Modal title="この問題のグラフを再編集" onClose={() => setGraphAssets(null)}>
          {graphAssets.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--muted)" }}>再編集できるグラフはありません。</p>
          ) : (
            <div className="max-h-[60vh] space-y-2 overflow-y-auto">
              {graphAssets.map((asset) => (
                <button
                  key={asset.assetId}
                  className="card w-full px-3 py-2 text-left"
                  disabled={graphBusy}
                  onClick={() => void onReeditGraph(asset)}
                >
                  <div className="font-semibold">{asset.displayName || asset.assetId}</div>
                  <div className="mt-1 text-[11px]" style={{ color: "var(--muted)" }}>更新: {asset.updatedAt}</div>
                </button>
              ))}
            </div>
          )}
        </Modal>
      )}

      {/* 競合解決ダイアログ */}
      {conflict && (
        <ConflictDialog
          title={`問題「${problem.title}」`}
          fields={[
            { label: "問題文", mine: problem.statement_latex, server: conflict.statement_latex },
            { label: "解答", mine: problem.answer_latex, server: conflict.answer_latex },
            { label: "解説", mine: problem.explanation_latex, server: conflict.explanation_latex },
            { label: "タイトル", mine: problem.title, server: conflict.title },
          ]}
          onResolve={resolveConflict}
          onClose={() => setConflict(null)}
        />
      )}

      {/* 移動モーダル */}
      {showMove && (
        <UnitPicker
          title={`「${problem.title}」を移動`}
          excludeUnitId={problem.unit_id}
          onClose={() => setShowMove(false)}
          onPick={async (unitId) => {
            try {
              await moveProblems([problem.id], unitId);
              setShowMove(false);
              setProblem((p) => (p ? { ...p, unit_id: unitId } : p));
              await refreshTree();
              showToast("移動しました");
            } catch (e) {
              showToast(String(e), "error");
            }
          }}
        />
      )}

      {/* 履歴モーダル */}
      {versions && (
        <Modal title="変更履歴" onClose={() => setVersions(null)} wide={versionView != null}>
          {versions.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--muted)" }}>
              履歴はまだありません（保存すると記録されます）。
            </p>
          ) : (
            <div className="flex gap-4">
              <ul className="max-h-[60vh] w-56 shrink-0 space-y-1 overflow-y-auto">
                {versions.map((v) => (
                  <li key={v.id}>
                    <button
                      onClick={async () => setVersionView(await getVersion(v.id))}
                      className="card w-full px-2 py-1.5 text-left text-xs"
                      style={
                        versionView?.id === v.id
                          ? { borderColor: "var(--accent)", background: "var(--accent-dim)" }
                          : undefined
                      }
                    >
                      <div className="font-semibold">{v.saved_at}</div>
                      <div className="truncate" style={{ color: "var(--muted)" }}>
                        {v.title}
                      </div>
                    </button>
                  </li>
                ))}
              </ul>
              {versionView && (
                <div className="min-w-0 flex-1">
                  <div className="mb-2 flex items-center justify-between">
                    <span className="text-sm font-bold">{versionView.title}</span>
                    <button onClick={() => onRestore(versionView.id)} className="btn btn-solid btn-sm">
                      この内容に復元
                    </button>
                  </div>
                  <div className="max-h-[55vh] space-y-2 overflow-y-auto text-xs">
                    <p className="section-label">問題文</p>
                    <pre
                      className="rounded p-2 font-mono whitespace-pre-wrap"
                      style={{ background: "var(--panel-2)" }}
                    >
                      {versionView.statement_latex}
                    </pre>
                    <p className="section-label">解答</p>
                    <pre
                      className="rounded p-2 font-mono whitespace-pre-wrap"
                      style={{ background: "var(--panel-2)" }}
                    >
                      {versionView.answer_latex}
                    </pre>
                    {versionView.explanation_latex && (
                      <>
                        <p className="section-label">解説</p>
                        <pre
                          className="rounded p-2 font-mono whitespace-pre-wrap"
                          style={{ background: "var(--panel-2)" }}
                        >
                          {versionView.explanation_latex}
                        </pre>
                      </>
                    )}
                  </div>
                </div>
              )}
            </div>
          )}
        </Modal>
      )}
    </div>
  );
}
