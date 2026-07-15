import { useEffect, useRef, useState } from "react";
import {
  aiCancelJob,
  aiCreateJob,
  aiGetJob,
  aiMarkInserted,
  aiRecompileJob,
  aiRetryJob,
  aiSaveAsPart,
  aiSaveAsProblem,
  aiStoreInputImage,
  aiUpdateJobLatex,
  getSettings,
} from "../api";
import { useApp } from "../store";
import { buildFileUrl, isTauri } from "../transport";
import type { AiJob } from "../types";
import { LatexEditor } from "./LatexEditor";
import { UnitPicker } from "./ProblemList";
import { Icon } from "./Icon";

/** 変換モード一覧 */
const MODES: { value: string; label: string; experimental?: boolean }[] = [
  { value: "auto", label: "自動判定" },
  { value: "math_only", label: "数式のみ" },
  { value: "problem", label: "問題文" },
  { value: "problem_with_subquestions", label: "問題文＋小問" },
  { value: "answer_explanation", label: "解答・解説" },
  { value: "table", label: "表" },
  { value: "matrix", label: "行列" },
  { value: "cases", label: "場合分け" },
  { value: "part", label: "教材部品" },
  { value: "tikz", label: "TikZ候補（実験的）", experimental: true },
  { value: "verbatim", label: "原文を整形せず転記" },
];

const RUNNING_STATUSES = [
  "queued",
  "preprocessing",
  "waiting_for_codex",
  "converting",
  "validating",
  "compiling",
];

const MAX_INPUT_IMAGES = 8;

function booleanOption(
  options: Record<string, unknown> | undefined,
  key: string,
  fallback: boolean,
): boolean {
  const value = options?.[key];
  return typeof value === "boolean" ? value : fallback;
}

function reformatOption(options: Record<string, unknown> | undefined): boolean {
  if (typeof options?.reformat === "boolean") return options.reformat;
  if (typeof options?.faithful === "boolean") return !options.faithful;
  return false;
}

interface LocalImage {
  /** サーバー側の保存名（uploads内） */
  name: string;
  /** ローカルプレビュー用DataURL */
  dataUrl: string;
}

export interface InsertTarget {
  label: string;
  field: string;
  entityType: string;
  entityId: number;
  insert: (latex: string) => void;
}

/** 画像ファイル → 向き補正・縮小済みのJPEG/PNG DataURL */
async function fileToProcessed(file: File): Promise<{ base64: string; dataUrl: string }> {
  const bitmap = await createImageBitmap(file).catch(() => null);
  if (!bitmap) throw new Error(`${file.name} を画像として読み込めません（HEIC等は未対応です）`);
  const MAX = 2200;
  const scale = Math.min(1, MAX / Math.max(bitmap.width, bitmap.height));
  const w = Math.max(1, Math.round(bitmap.width * scale));
  const h = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, w, h);
  ctx.drawImage(bitmap, 0, 0, w, h);
  const isPng = file.type === "image/png";
  const dataUrl = canvas.toDataURL(isPng ? "image/png" : "image/jpeg", 0.9);
  return { base64: dataUrl.split(",")[1], dataUrl };
}

/** DataURL画像を90度回転する */
async function rotateDataUrl(dataUrl: string): Promise<{ base64: string; dataUrl: string }> {
  const img = new Image();
  await new Promise<void>((resolve, reject) => {
    img.onload = () => resolve();
    img.onerror = () => reject(new Error("画像の回転に失敗しました"));
    img.src = dataUrl;
  });
  const canvas = document.createElement("canvas");
  canvas.width = img.height;
  canvas.height = img.width;
  const ctx = canvas.getContext("2d")!;
  ctx.translate(canvas.width / 2, canvas.height / 2);
  ctx.rotate(Math.PI / 2);
  ctx.drawImage(img, -img.width / 2, -img.height / 2);
  const rotated = canvas.toDataURL("image/jpeg", 0.9);
  return { base64: rotated.split(",")[1], dataUrl: rotated };
}

/**
 * 写真・テキスト → LaTeX 変換ダイアログ。
 * insertTargets を渡すと、変換結果を呼び出し元エディタへ挿入できる。
 */
export function AiConvertDialog({
  onClose,
  insertTargets,
  initialJob,
}: {
  onClose: () => void;
  insertTargets?: InsertTarget[];
  initialJob?: AiJob | null;
}) {
  const { showToast, confirm, bumps } = useApp();
  const [step, setStep] = useState<"input" | "running" | "review">(
    initialJob ? (RUNNING_STATUSES.includes(initialJob.status) ? "running" : "review") : "input",
  );
  const [sourceType, setSourceType] = useState<"image" | "text">("image");
  const [images, setImages] = useState<LocalImage[]>([]);
  const [text, setText] = useState("");
  const [mode, setMode] = useState(initialJob?.conversionMode ?? "auto");
  const [reformat, setReformat] = useState(() => reformatOption(initialJob?.options));
  const [enumerateSub, setEnumerateSub] = useState(() =>
    booleanOption(initialJob?.options, "enumerateSubquestions", true),
  );
  const [displayMath, setDisplayMath] = useState(() =>
    booleanOption(initialJob?.options, "displayMath", false),
  );
  const [useTemplateContext, setUseTemplateContext] = useState(() =>
    booleanOption(initialJob?.options, "useTemplateContext", true),
  );
  const [busy, setBusy] = useState(false);
  const [job, setJob] = useState<AiJob | null>(initialJob ?? null);
  const [latex, setLatex] = useState(initialJob?.outputLatex ?? "");
  const [confirmedUncertain, setConfirmedUncertain] = useState<Record<string, boolean>>({});
  const [overrideUncertain, setOverrideUncertain] = useState(false);
  const [showLog, setShowLog] = useState(false);
  const [showUnitPicker, setShowUnitPicker] = useState(false);
  const [dataDir, setDataDir] = useState<string | null>(null);
  const [reviewPane, setReviewPane] = useState<"source" | "preview" | "latex" | "warnings">("latex");
  const [isNarrow, setIsNarrow] = useState(() => window.innerWidth < 900);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const cameraInputRef = useRef<HTMLInputElement>(null);
  const jobRef = useRef<AiJob | null>(job);
  /** 登録済み＋処理中の予約数。並行addFilesでも8枚を超えないための同期カウンター。 */
  const imageSlotCountRef = useRef(images.length);
  jobRef.current = job;

  useEffect(() => {
    const onResize = () => setIsNarrow(window.innerWidth < 900);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    getSettings()
      .then((s) => setDataDir(s.data_dir ?? null))
      .catch(() => {});
  }, []);

  // 実行中はポーリング＋イベントで状態を追う
  useEffect(() => {
    if (step !== "running" || !job) return;
    let stop = false;
    const tick = async () => {
      if (stop) return;
      try {
        const j = await aiGetJob(job.id);
        if (stop) return;
        setJob(j);
        if (!RUNNING_STATUSES.includes(j.status)) {
          if (j.status === "completed") {
            setLatex(j.outputLatex);
            setStep("review");
          } else if (j.status === "failed") {
            setStep("review");
          } else {
            onClose();
          }
        }
      } catch {
        /* 次のtickで再試行 */
      }
    };
    const timer = setInterval(tick, 2000);
    tick();
    return () => {
      stop = true;
      clearInterval(timer);
    };
  }, [step, job?.id, bumps.ai_job]);

  const addFiles = async (files: FileList | File[]) => {
    const requested = Array.from(files);
    const available = Math.max(0, MAX_INPUT_IMAGES - imageSlotCountRef.current);
    const accepted = requested.slice(0, available);
    if (accepted.length < requested.length) {
      showToast(`画像は最大${MAX_INPUT_IMAGES}枚までです`, "error");
    }
    if (accepted.length === 0) return;

    // awaitより前に枠を予約し、同時に呼ばれたaddFilesからも見えるようにする。
    imageSlotCountRef.current += accepted.length;
    let added = 0;
    setBusy(true);
    try {
      for (const file of accepted) {
        const { base64, dataUrl } = await fileToProcessed(file);
        const stored = await aiStoreInputImage(base64, file.name);
        added += 1;
        setImages((prev) => [...prev, { name: stored.name, dataUrl }]);
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      // 失敗により追加されなかった予約枠だけを戻す。
      imageSlotCountRef.current -= accepted.length - added;
      setBusy(false);
    }
  };

  const removeImage = (index: number) => {
    if (!images[index]) return;
    imageSlotCountRef.current = Math.max(0, imageSlotCountRef.current - 1);
    setImages((prev) => prev.filter((_, current) => current !== index));
  };

  // クリップボード貼り付け（画像・テキスト）
  useEffect(() => {
    if (step !== "input") return;
    const handler = (e: ClipboardEvent) => {
      const items = e.clipboardData?.items;
      if (!items) return;
      const files: File[] = [];
      for (const item of items) {
        if (item.type.startsWith("image/")) {
          const f = item.getAsFile();
          if (f) files.push(f);
        }
      }
      if (files.length > 0) {
        e.preventDefault();
        setSourceType("image");
        addFiles(files);
      }
    };
    window.addEventListener("paste", handler);
    return () => window.removeEventListener("paste", handler);
  }, [step, images.length]);

  const rotateImage = async (index: number) => {
    const img = images[index];
    setBusy(true);
    try {
      const { base64, dataUrl } = await rotateDataUrl(img.dataUrl);
      const stored = await aiStoreInputImage(base64, "rotated.jpg");
      setImages((prev) => prev.map((x, i) => (i === index ? { name: stored.name, dataUrl } : x)));
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const moveImage = (index: number, delta: number) => {
    setImages((prev) => {
      const next = [...prev];
      const to = index + delta;
      if (to < 0 || to >= next.length) return prev;
      [next[index], next[to]] = [next[to], next[index]];
      return next;
    });
  };

  const run = async () => {
    setBusy(true);
    try {
      const j = await aiCreateJob({
        sourceType,
        conversionMode: mode,
        options: {
          faithful: !reformat,
          reformat,
          enumerateSubquestions: enumerateSub,
          displayMath,
          useTemplateContext,
          suggestPackages: true,
        },
        inputText: text,
        inputNames: sourceType === "image" ? images.map((i) => i.name) : [],
      });
      setJob(j);
      setConfirmedUncertain({});
      setOverrideUncertain(false);
      setStep("running");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    if (!job) return;
    try {
      await aiCancelJob(job.id);
      showToast("キャンセルしました");
      onClose();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const recompile = async () => {
    if (!job) return;
    setBusy(true);
    try {
      await aiUpdateJobLatex(job.id, latex);
      const j = await aiRecompileJob(job.id);
      setJob(j);
      showToast(j.compileStatus === "ok" ? "コンパイル成功" : "コンパイルに失敗しました", j.compileStatus === "ok" ? "info" : "error");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const retry = async () => {
    if (!job) return;
    setBusy(true);
    try {
      const j = await aiRetryJob(job.id, mode, {
        ...job.options,
        faithful: !reformat,
        reformat,
        enumerateSubquestions: enumerateSub,
        displayMath,
        useTemplateContext,
        suggestPackages: booleanOption(job.options, "suggestPackages", true),
      });
      setJob(j);
      setStep("running");
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const uncertainList = job?.uncertainFragments ?? [];
  const allConfirmed =
    uncertainList.length === 0 ||
    overrideUncertain ||
    uncertainList.every((u) => confirmedUncertain[u.id]);

  const guardInsert = async (): Promise<boolean> => {
    if (allConfirmed) return true;
    return await confirm(
      "未確認の「要確認箇所」が残っています。\nこのまま挿入しますか？（内容をよく確認してください）",
    );
  };

  const doInsert = async (target: InsertTarget) => {
    if (!job) return;
    const confirmed = await guardInsert();
    if (!confirmed) return;
    try {
      await aiUpdateJobLatex(job.id, latex);
      await aiMarkInserted(
        job.id,
        target.entityType,
        target.entityId,
        target.field,
        confirmed,
      );
      target.insert(latex);
      showToast(`${target.label}へ挿入しました（Ctrl+Sで保存してください）`);
      onClose();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const saveAsPart = async () => {
    if (!job) return;
    const confirmed = await guardInsert();
    if (!confirmed) return;
    const title = window.prompt("部品のタイトル", "AI変換部品");
    if (title == null) return;
    try {
      await aiUpdateJobLatex(job.id, latex);
      const id = await aiSaveAsPart(job.id, title, undefined, confirmed);
      showToast(`部品として保存しました (ID: ${id})`);
      onClose();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const saveAsProblem = async (unitId: number) => {
    if (!job) return;
    const confirmed = await guardInsert();
    if (!confirmed) return;
    const title = window.prompt("問題のタイトル", "AI変換問題");
    if (title == null) return;
    try {
      await aiUpdateJobLatex(job.id, latex);
      const id = await aiSaveAsProblem(job.id, unitId, title, confirmed);
      showToast(`新規問題として保存しました (ID: ${id})`);
      setShowUnitPicker(false);
      onClose();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  /** ジョブ入力画像のURL（履歴から開いた場合用） */
  const jobImageUrl = (name: string): string | null => {
    if (!job) return null;
    if (isTauri) {
      if (!dataDir) return null;
      return buildFileUrl(`${dataDir}\\ai_jobs\\${job.jobUuid}\\${name}`);
    }
    return `/api/files/ai-job/${encodeURIComponent(job.jobUuid)}/${encodeURIComponent(name)}`;
  };

  const severityColor = (s: string) =>
    s === "error" ? "var(--danger)" : s === "warning" ? "var(--warn)" : "var(--muted)";

  // ---- 入力ステップ ----
  const renderInput = () => (
    <div className="space-y-4">
      <div className="flex gap-1">
        <button
          onClick={() => setSourceType("image")}
          className={`btn ${sourceType === "image" ? "btn-outline" : "btn-ghost"}`}
        >
          📷 写真・画像
        </button>
        <button
          onClick={() => setSourceType("text")}
          className={`btn ${sourceType === "text" ? "btn-outline" : "btn-ghost"}`}
        >
          📝 テキスト
        </button>
      </div>

      {sourceType === "image" ? (
        <div
          className="rounded border-2 border-dashed p-3"
          style={{ borderColor: "var(--border-strong)" }}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            if (e.dataTransfer.files.length > 0) addFiles(e.dataTransfer.files);
          }}
        >
          <div className="mb-2 flex flex-wrap gap-2">
            <button onClick={() => cameraInputRef.current?.click()} className="btn btn-outline">
              📸 カメラで撮影
            </button>
            <button onClick={() => fileInputRef.current?.click()} className="btn btn-outline">
              🖼 画像を選択
            </button>
            <span className="self-center text-[11px]" style={{ color: "var(--muted)" }}>
              ドラッグ＆ドロップ / Ctrl+V 貼り付け対応・最大8枚（PNG/JPEG/WEBP）
            </span>
          </div>
          <input
            ref={cameraInputRef}
            type="file"
            accept="image/*"
            capture="environment"
            className="hidden"
            onChange={(e) => {
              if (e.target.files) addFiles(e.target.files);
              e.target.value = "";
            }}
          />
          <input
            ref={fileInputRef}
            type="file"
            accept="image/png,image/jpeg,image/webp"
            multiple
            className="hidden"
            onChange={(e) => {
              if (e.target.files) addFiles(e.target.files);
              e.target.value = "";
            }}
          />
          {images.length === 0 ? (
            <p className="py-6 text-center text-xs" style={{ color: "var(--muted)" }}>
              画像がまだありません
            </p>
          ) : (
            <ul className="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4">
              {images.map((img, i) => (
                <li key={img.name} className="card p-1.5">
                  <img
                    src={img.dataUrl}
                    alt={`入力${i + 1}`}
                    className="mb-1 h-28 w-full rounded object-contain"
                    style={{ background: "#fff" }}
                  />
                  <div className="flex items-center justify-between gap-0.5">
                    <span className="text-[10px]" style={{ color: "var(--muted)" }}>
                      {i + 1}枚目
                    </span>
                    <span className="flex gap-0.5">
                      <button onClick={() => moveImage(i, -1)} className="btn btn-ghost btn-sm" title="前へ">
                        ←
                      </button>
                      <button onClick={() => moveImage(i, 1)} className="btn btn-ghost btn-sm" title="後へ">
                        →
                      </button>
                      <button onClick={() => rotateImage(i)} className="btn btn-ghost btn-sm" title="90度回転">
                        ⟳
                      </button>
                      <button
                        onClick={() => removeImage(i)}
                        className="btn btn-danger btn-sm"
                        title="削除"
                      >
                        ✕
                      </button>
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          )}
          <div className="mt-2">
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="input w-full font-mono text-xs"
              rows={2}
              placeholder="補足テキスト（任意）: 画像に加えて伝えたい原文・注記など"
            />
          </div>
        </div>
      ) : (
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          className="input w-full font-mono text-xs"
          rows={10}
          placeholder="変換したい文章・数式を貼り付けてください（Word・PDFからのコピー等）"
        />
      )}

      <div>
        <p className="section-label mb-1">変換モード</p>
        <div className="flex flex-wrap gap-1">
          {MODES.map((m) => (
            <button
              key={m.value}
              onClick={() => setMode(m.value)}
              className={`btn btn-sm ${mode === m.value ? "btn-outline" : "btn-ghost"}`}
              title={m.experimental ? "実験的機能: 結果は自動挿入されません" : undefined}
            >
              {m.label}
            </button>
          ))}
        </div>
        {mode === "tikz" && (
          <p className="mt-1 text-[11px]" style={{ color: "var(--warn)" }}>
            TikZ候補は実験的機能です。生成されたコードは必ず内容を確認してから使用してください。
            関数グラフの場合は、教材編集画面の「グラフを挿入」（グラフ作成アプリ連携）の方が正確です。
          </p>
        )}
      </div>

      <div className="grid gap-1 text-xs sm:grid-cols-2" style={{ color: "var(--text)" }}>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" checked={!reformat} onChange={(e) => setReformat(!e.target.checked)} />
          原文に忠実（推奨）
        </label>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" checked={reformat} onChange={(e) => setReformat(e.target.checked)} />
          教材向けに体裁を整える
        </label>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" checked={enumerateSub} onChange={(e) => setEnumerateSub(e.target.checked)} />
          小問をenumerateへ変換
        </label>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" checked={displayMath} onChange={(e) => setDisplayMath(e.target.checked)} />
          数式を別行立てにする
        </label>
        <label className="flex items-center gap-1.5">
          <input
            type="checkbox"
            checked={useTemplateContext}
            onChange={(e) => setUseTemplateContext(e.target.checked)}
          />
          普段のテンプレート規則を使用
        </label>
      </div>
      <p className="text-[11px]" style={{ color: "var(--muted)" }}>
        AIは問題を解かず、解答の生成も行いません。不鮮明な箇所は推測せず「要確認」として報告されます。
      </p>

      <div className="flex justify-end gap-2">
        <button onClick={onClose} className="btn btn-ghost">
          閉じる
        </button>
        <button
          onClick={run}
          disabled={busy || (sourceType === "image" ? images.length === 0 : !text.trim())}
          className="btn btn-solid"
        >
          {busy ? "準備中..." : <><Icon name="play" size={15} /> LaTeXへ変換</>}
        </button>
      </div>
    </div>
  );

  // ---- 実行中ステップ ----
  const renderRunning = () => (
    <div className="space-y-4 py-6 text-center">
      <div className="mx-auto h-8 w-8 animate-spin rounded-full border-2 border-t-transparent" style={{ borderColor: "var(--accent)", borderTopColor: "transparent" }} />
      <p className="text-sm font-semibold">{job?.progressMessage || "変換しています…"}</p>
      <p className="text-xs" style={{ color: "var(--muted)" }}>
        状態: {job?.status}　（Codexの利用状況によって数十秒〜数分かかることがあります）
      </p>
      <button onClick={cancel} className="btn btn-ghost">
        キャンセル
      </button>
    </div>
  );

  // ---- 確認ステップ ----
  const renderSourcePane = () => (
    <div className="min-h-0 flex-1 overflow-auto rounded border p-2" style={{ borderColor: "var(--border)" }}>
      {job && job.inputAssetPaths.length > 0 ? (
        <div className="space-y-2">
          {job.inputAssetPaths.map((name, i) => {
            const local = images[i]?.dataUrl;
            const url = local ?? jobImageUrl(name);
            return url ? (
              <a key={name} href={url} target="_blank" rel="noreferrer" title="クリックで原寸表示">
                <img src={url} alt={`元画像${i + 1}`} className="w-full rounded" style={{ background: "#fff" }} />
              </a>
            ) : (
              <p key={name} className="text-xs" style={{ color: "var(--muted)" }}>
                （画像を表示できません）
              </p>
            );
          })}
        </div>
      ) : (
        <pre className="text-xs whitespace-pre-wrap" style={{ color: "var(--text)" }}>
          {job?.inputText || "（入力なし）"}
        </pre>
      )}
    </div>
  );

  const renderPreviewPane = () => (
    <div className="flex min-h-0 flex-1 flex-col rounded border" style={{ borderColor: "var(--border)" }}>
      {job?.compileStatus === "ok" && job.previewPdfPath ? (
        <iframe
          src={buildFileUrl(job.previewPdfPath, Date.parse(job.updatedAt) || Date.now())}
          className="min-h-0 w-full flex-1 rounded"
          style={{ background: "#fff", minHeight: 240 }}
          title="変換結果のPDFプレビュー"
        />
      ) : job?.compileStatus === "failed" ? (
        <div className="min-h-0 flex-1 overflow-auto p-2 text-xs">
          <p className="mb-1 font-semibold" style={{ color: "var(--danger)" }}>
            試験コンパイルに失敗しました。LaTeXを修正して「再コンパイル」してください。
          </p>
          <pre className="whitespace-pre-wrap" style={{ color: "var(--muted)" }}>
            {(job.compileLog || "").split("\n").filter((l) => l.startsWith("!") || /\.tex:\d+/.test(l)).slice(0, 12).join("\n") || job.compileLog.slice(0, 800)}
          </pre>
        </div>
      ) : (
        <p className="p-4 text-center text-xs" style={{ color: "var(--muted)" }}>
          {job?.compileStatus === "skipped"
            ? `試験コンパイルは実行されませんでした: ${job.compileLog}`
            : "PDFプレビューはありません"}
        </p>
      )}
    </div>
  );

  const renderWarningsPane = () => (
    <div className="min-h-0 flex-1 space-y-2 overflow-auto rounded border p-2 text-xs" style={{ borderColor: "var(--border)" }}>
      {job?.status === "failed" && (
        <p style={{ color: "var(--danger)" }}>
          変換に失敗しました: {job.errorMessage}
        </p>
      )}
      {(job?.warnings ?? []).length === 0 && uncertainList.length === 0 && job?.status !== "failed" && (
        <p style={{ color: "var(--success)" }}>警告はありません。</p>
      )}
      {(job?.warnings ?? []).map((w, i) => (
        <div key={i} className="rounded border px-2 py-1" style={{ borderColor: "var(--border)" }}>
          <span className="font-mono text-[10px]" style={{ color: severityColor(w.severity) }}>
            [{w.code}]
          </span>{" "}
          {w.message}
        </div>
      ))}
      {uncertainList.length > 0 && (
        <div>
          <p className="section-label mb-1">要確認箇所（不鮮明・判別不能）</p>
          {uncertainList.map((u) => (
            <label
              key={u.id}
              className="mb-1 flex items-start gap-2 rounded border px-2 py-1"
              style={{ borderColor: confirmedUncertain[u.id] ? "var(--border)" : "rgba(251,191,36,0.5)" }}
            >
              <input
                type="checkbox"
                checked={!!confirmedUncertain[u.id]}
                onChange={(e) =>
                  setConfirmedUncertain((prev) => ({ ...prev, [u.id]: e.target.checked }))
                }
                className="mt-0.5"
              />
              <span>
                <span className="font-semibold">{u.description}</span>
                {u.candidates.length > 0 && (
                  <span style={{ color: "var(--muted)" }}>（候補: {u.candidates.join(" / ")}）</span>
                )}
                <span className="ml-1" style={{ color: "var(--muted)" }}>
                  確認済みにする
                </span>
              </span>
            </label>
          ))}
          <label className="flex items-center gap-1.5 text-[11px]" style={{ color: "var(--muted)" }}>
            <input
              type="checkbox"
              checked={overrideUncertain}
              onChange={(e) => setOverrideUncertain(e.target.checked)}
            />
            すべて確認したものとして扱う
          </label>
        </div>
      )}
      {job?.structuredResult?.requiredPackages && job.structuredResult.requiredPackages.length > 0 && (
        <p style={{ color: "var(--muted)" }}>
          必要パッケージの提案: {job.structuredResult.requiredPackages.join(", ")}
        </p>
      )}
      {job?.structuredResult && (
        <p style={{ color: "var(--muted)" }}>
          判定された種類: {job.structuredResult.detectedType}
          {job.structuredResult.detectedType === "graph" && (
            <span style={{ color: "var(--warn)" }}>
              　※関数グラフはグラフ作成アプリ連携（教材編集画面の「グラフを挿入」）の利用を推奨します
            </span>
          )}
        </p>
      )}
      {showLog && (
        <pre className="max-h-48 overflow-auto rounded p-2 text-[10px] whitespace-pre-wrap" style={{ background: "var(--panel-2)" }}>
          {job?.compileLog || "(ログなし)"}
        </pre>
      )}
      <button onClick={() => setShowLog(!showLog)} className="btn btn-ghost btn-sm">
        {showLog ? "ログを隠す" : "コンパイルログ詳細"}
      </button>
    </div>
  );

  const renderLatexPane = () => (
    <div className="flex min-h-0 flex-1 flex-col">
      <LatexEditor value={latex} onChange={setLatex} className="min-h-[180px] flex-1" placeholder="変換結果のLaTeX" />
    </div>
  );

  const renderReview = () => (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      {isNarrow ? (
        <>
          <div className="flex gap-1">
            {(
              [
                ["source", "元画像・原文"],
                ["preview", "プレビュー"],
                ["latex", "LaTeX"],
                ["warnings", `警告${uncertainList.length > 0 ? `(${uncertainList.length})` : ""}`],
              ] as const
            ).map(([key, label]) => (
              <button
                key={key}
                onClick={() => setReviewPane(key)}
                className={`tab ${reviewPane === key ? "tab-active" : ""}`}
              >
                {label}
              </button>
            ))}
          </div>
          <div className="flex min-h-[300px] flex-1 flex-col">
            {reviewPane === "source" && renderSourcePane()}
            {reviewPane === "preview" && renderPreviewPane()}
            {reviewPane === "latex" && renderLatexPane()}
            {reviewPane === "warnings" && renderWarningsPane()}
          </div>
        </>
      ) : (
        <>
          <div className="grid min-h-[260px] flex-1 grid-cols-2 gap-2">
            <div className="flex min-h-0 flex-col">
              <p className="section-label mb-1">元画像・入力文</p>
              {renderSourcePane()}
            </div>
            <div className="flex min-h-0 flex-col">
              <p className="section-label mb-1">PDFプレビュー</p>
              {renderPreviewPane()}
            </div>
          </div>
          <div className="flex min-h-[160px] flex-col">
            <p className="section-label mb-1">LaTeXソース（編集可能）</p>
            {renderLatexPane()}
          </div>
          <div className="max-h-48 min-h-[80px]">{renderWarningsPane()}</div>
        </>
      )}

      <div className="flex flex-wrap items-center justify-end gap-1.5 border-t pt-2" style={{ borderColor: "var(--border)" }}>
        <button onClick={recompile} disabled={busy} className="btn btn-outline btn-sm">
          ⟳ 再コンパイル
        </button>
        <button onClick={retry} disabled={busy} className="btn btn-ghost btn-sm" title="同じ入力で再変換">
          再変換
        </button>
        <span className="mx-1 h-4 w-px" style={{ background: "var(--border)" }} />
        {(insertTargets ?? []).map((t) => (
          <button
            key={t.field}
            onClick={() => doInsert(t)}
            disabled={busy || !latex.trim() || job?.status === "failed"}
            className="btn btn-solid btn-sm"
          >
            {t.label}へ挿入
          </button>
        ))}
        <button
          onClick={saveAsPart}
          disabled={busy || !latex.trim() || job?.status === "failed"}
          className="btn btn-outline btn-sm"
        >
          部品として保存
        </button>
        <button
          onClick={() => setShowUnitPicker(true)}
          disabled={busy || !latex.trim() || job?.status === "failed"}
          className="btn btn-outline btn-sm"
        >
          新規問題として保存
        </button>
        <button onClick={onClose} className="btn btn-ghost btn-sm">
          閉じる
        </button>
      </div>
    </div>
  );

  return (
    <div
      className="safe-area-overlay fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-2"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && step !== "running") onClose();
      }}
    >
      <div
        className="fade-in flex max-h-[95vh] w-full max-w-5xl flex-col rounded-md border shadow-2xl"
        style={{ background: "var(--panel)", borderColor: "var(--border-strong)" }}
      >
        <div className="flex items-center justify-between border-b px-4 py-2.5" style={{ borderColor: "var(--border)" }}>
          <h2 className="text-sm font-bold">
            <span className="brand-mark mr-1.5">▸</span>
            AI変換（写真・テキスト → LaTeX）
            {job && (
              <span className="badge badge-muted ml-2">ジョブ #{job.id}</span>
            )}
          </h2>
          {step !== "running" && (
            <button onClick={onClose} className="btn btn-ghost btn-sm">
              ✕
            </button>
          )}
        </div>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-4">
          {step === "input" && renderInput()}
          {step === "running" && renderRunning()}
          {step === "review" && renderReview()}
        </div>
      </div>

      {showUnitPicker && (
        <UnitPicker
          title="保存先の単元を選択"
          onClose={() => setShowUnitPicker(false)}
          onPick={saveAsProblem}
        />
      )}
    </div>
  );
}

/** ジョブ履歴から確認画面を開くための薄いラッパー */
export function AiJobReviewModal({ job, onClose }: { job: AiJob; onClose: () => void }) {
  return <AiConvertDialog onClose={onClose} initialJob={job} />;
}
