import { useEffect, useRef, useState } from "react";
import {
  aiCancelJob,
  aiGetJob,
  aiStoreInputImage,
  startPatternImageImport,
} from "../api";
import { useApp } from "../store";
import type { AiJob, AiPatternExtractionResult, PatternProposal } from "../types";
import { withProposalDefaults } from "./PatternProposalReview";
import { PatternProposalReviewList } from "./PatternProposalReviewDialog";
import { Modal } from "./ui";

const RUNNING = new Set([
  "queued",
  "preprocessing",
  "waiting_for_codex",
  "converting",
  "validating",
  "compiling",
]);

/** ai_create_job と同じ上限。 */
const MAX_IMAGES = 8;

type StoredImage = { name: string; fileName: string; dataUrl: string };

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

/** アップロード前に長辺を縮め、白背景で塗りつぶしたJPEG/PNGにする。 */
async function fileToProcessed(file: File): Promise<{ base64: string; dataUrl: string }> {
  const bitmap = await createImageBitmap(file).catch(() => null);
  if (!bitmap) throw new Error(`${file.name} を画像として読み込めません（HEIC等は未対応です）`);
  const MAX = 2200;
  const scale = Math.min(1, MAX / Math.max(bitmap.width, bitmap.height));
  const width = Math.max(1, Math.round(bitmap.width * scale));
  const height = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d")!;
  context.fillStyle = "#fff";
  context.fillRect(0, 0, width, height);
  context.drawImage(bitmap, 0, 0, width, height);
  const isPng = file.type === "image/png";
  const dataUrl = canvas.toDataURL(isPng ? "image/png" : "image/jpeg", 0.9);
  return { base64: dataUrl.split(",")[1], dataUrl };
}

function importResult(job: AiJob): AiPatternExtractionResult {
  const result = job.structuredResult;
  if (!result || !("kind" in result) || result.kind !== "pattern-extraction") {
    throw new Error("AIから定石候補の構造化データを取得できませんでした");
  }
  return result as AiPatternExtractionResult;
}

export function PatternImageImportDialog({
  onClose,
  onApplied,
}: {
  onClose: () => void;
  onApplied?: () => void;
}) {
  const { showToast } = useApp();
  const [images, setImages] = useState<StoredImage[]>([]);
  const [note, setNote] = useState("");
  const [uploading, setUploading] = useState(false);
  const [job, setJob] = useState<AiJob | null>(null);
  const [running, setRunning] = useState(false);
  const [proposals, setProposals] = useState<PatternProposal[]>([]);
  const [error, setError] = useState("");
  const activeRef = useRef(true);

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
    };
  }, []);

  const addFiles = async (files: FileList | null) => {
    if (!files || files.length === 0) return;
    const room = MAX_IMAGES - images.length;
    if (room <= 0) {
      showToast(`画像は最大${MAX_IMAGES}枚までです`, "error");
      return;
    }
    const accepted = Array.from(files).slice(0, room);
    if (accepted.length < files.length) {
      showToast(`画像は最大${MAX_IMAGES}枚までのため、${accepted.length}枚だけ追加します`);
    }
    setUploading(true);
    try {
      for (const file of accepted) {
        const { base64, dataUrl } = await fileToProcessed(file);
        const stored = await aiStoreInputImage(base64, file.name);
        if (!activeRef.current) return;
        setImages((current) => [...current, { name: stored.name, fileName: file.name, dataUrl }]);
      }
    } catch (caught) {
      showToast(String(caught), "error");
    } finally {
      setUploading(false);
    }
  };

  const start = async () => {
    if (images.length === 0) {
      showToast("取り込む画像を選択してください", "error");
      return;
    }
    setRunning(true);
    setError("");
    try {
      let current = await startPatternImageImport(
        images.map((image) => image.name),
        note.trim() || undefined,
      );
      setJob(current);
      while (RUNNING.has(current.status) && activeRef.current) {
        await sleep(1100);
        if (!activeRef.current) return;
        current = await aiGetJob(current.id);
        setJob(current);
      }
      if (!activeRef.current) return;
      if (current.status !== "completed") {
        throw new Error(current.errorMessage || "画像からの取り込みに失敗しました");
      }
      const result = importResult(current);
      if (result.patterns.length === 0) {
        throw new Error("画像から定石を読み取れませんでした");
      }
      const normalized = result.patterns.map(withProposalDefaults);
      setProposals(normalized);
      showToast(`${normalized.length}件の定石候補を読み取りました`);
    } catch (caught) {
      if (activeRef.current) setError(String(caught));
    } finally {
      setRunning(false);
    }
  };

  const cancelJob = async () => {
    if (!job || !RUNNING.has(job.status)) return;
    try {
      await aiCancelJob(job.id);
      setJob(await aiGetJob(job.id));
    } catch (caught) {
      showToast(String(caught), "error");
    }
  };

  return (
    <Modal title="画像・写真から定石を取り込む" onClose={onClose} wide>
      {proposals.length === 0 ? (
        <div className="space-y-4">
          {error && (
            <div
              className="rounded border p-3 text-sm"
              style={{ borderColor: "rgba(241,106,117,0.5)", color: "var(--danger)" }}
            >
              {error}
            </div>
          )}
          <p className="text-xs" style={{ color: "var(--muted)" }}>
            参考書・板書・自作教材などの画像から定石を読み取ります。1枚に複数の定石が並んでいる場合は、見出しや番号を手掛かりに分けて候補にします。
            原文の見出しと粒度は尊重し、勝手に別の定石へ作り替えません。読み取った候補は未保存で、保存を押したときだけ定石ライブラリへ入ります。
          </p>

          <label className="block">
            <span className="section-label mb-1 block">画像（最大{MAX_IMAGES}枚）</span>
            <input
              type="file"
              accept="image/*"
              multiple
              disabled={uploading || running}
              onChange={(event) => {
                void addFiles(event.target.files);
                event.target.value = "";
              }}
              className="input w-full text-sm"
            />
          </label>

          {images.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {images.map((image, index) => (
                <div key={image.name} className="card p-2" style={{ width: 150 }}>
                  <img
                    src={image.dataUrl}
                    alt={image.fileName}
                    className="mb-1 w-full rounded"
                    style={{ maxHeight: 110, objectFit: "contain", background: "#fff" }}
                  />
                  <div className="truncate text-[11px]" style={{ color: "var(--muted)" }}>
                    {index + 1}. {image.fileName}
                  </div>
                  <button
                    className="btn btn-ghost btn-sm mt-1 w-full"
                    disabled={running}
                    onClick={() =>
                      setImages((current) => current.filter((item) => item.name !== image.name))
                    }
                  >
                    削除
                  </button>
                </div>
              ))}
            </div>
          )}

          <label className="block">
            <span className="section-label mb-1 block">補足（任意）</span>
            <textarea
              className="input w-full resize-y text-sm"
              rows={2}
              value={note}
              disabled={running}
              placeholder="例: 上半分のKEY 1〜3だけを取り込んでください"
              onChange={(event) => setNote(event.target.value)}
            />
          </label>

          {running ? (
            <div className="space-y-2 py-4 text-center">
              <div className="text-sm font-semibold">
                {job?.progressMessage || "画像を読み取っています…"}
              </div>
              {job && RUNNING.has(job.status) && (
                <button className="btn btn-ghost btn-sm" onClick={() => void cancelJob()}>
                  キャンセル
                </button>
              )}
            </div>
          ) : (
            <div className="flex justify-end gap-2">
              <button className="btn btn-ghost" onClick={onClose}>
                閉じる
              </button>
              <button
                className="btn btn-solid"
                disabled={uploading || images.length === 0}
                onClick={() => void start()}
              >
                {uploading ? "画像を準備中…" : "定石を読み取る"}
              </button>
            </div>
          )}
        </div>
      ) : (
        <div className="space-y-4">
          <PatternProposalReviewList
            proposals={proposals}
            sourceReference={images.map((image) => image.fileName).join(" / ")}
            onApplied={onApplied}
          />
          <div className="flex justify-end border-t pt-3" style={{ borderColor: "var(--border)" }}>
            <button className="btn btn-ghost" onClick={onClose}>
              閉じる
            </button>
          </div>
        </div>
      )}
    </Modal>
  );
}
