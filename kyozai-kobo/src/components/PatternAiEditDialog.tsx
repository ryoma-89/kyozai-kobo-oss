import { useEffect, useRef, useState } from "react";
import { aiCancelJob, aiGetJob, applyPatternEdit, startPatternEdit } from "../api";
import { useApp } from "../store";
import { ConflictError } from "../transport";
import type { AiJob, AiPatternExtractionResult, PatternFull, PatternProposal } from "../types";
import { ProposalEditor, ProposalPreview, withProposalDefaults } from "./PatternProposalReview";
import { Modal } from "./ui";

const RUNNING = new Set([
  "queued",
  "preprocessing",
  "waiting_for_codex",
  "converting",
  "validating",
  "compiling",
]);

/** よく使う指示。押すと入力欄へ差し込む。 */
const PRESETS = [
  "数式で書ける部分は数式にして、日本語を減らしてください",
  "候補手法をもう1つ追加してください",
  "使う場面が分かるように状況を書き足してください",
  "注意点を1つ追加してください",
  "タイトルを短い名詞句にしてください",
  "全体をもっと簡潔にしてください",
];

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function editResult(job: AiJob): AiPatternExtractionResult {
  const result = job.structuredResult;
  if (!result || !("kind" in result) || result.kind !== "pattern-extraction") {
    throw new Error("AIから書き直した定石を取得できませんでした");
  }
  return result as AiPatternExtractionResult;
}

export function PatternAiEditDialog({
  pattern,
  onClose,
  onApplied,
}: {
  pattern: PatternFull;
  onClose: () => void;
  onApplied: () => Promise<void> | void;
}) {
  const { confirm, showToast } = useApp();
  const [instruction, setInstruction] = useState("");
  const [job, setJob] = useState<AiJob | null>(null);
  const [running, setRunning] = useState(false);
  const [proposal, setProposal] = useState<PatternProposal | null>(null);
  const [editing, setEditing] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState("");
  const activeRef = useRef(true);

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
    };
  }, []);

  const addPreset = (preset: string) => {
    setInstruction((current) => (current.trim() ? `${current.trim()}\n${preset}` : preset));
  };

  const run = async () => {
    if (!instruction.trim()) {
      showToast("AIへの指示を入力してください", "error");
      return;
    }
    setRunning(true);
    setError("");
    try {
      let current = await startPatternEdit(pattern.id, instruction.trim());
      setJob(current);
      while (RUNNING.has(current.status) && activeRef.current) {
        await sleep(1100);
        if (!activeRef.current) return;
        current = await aiGetJob(current.id);
        setJob(current);
      }
      if (!activeRef.current) return;
      if (current.status !== "completed") {
        throw new Error(current.errorMessage || "定石の書き直しに失敗しました");
      }
      const next = editResult(current).patterns[0];
      if (!next) throw new Error("書き直した定石を取得できませんでした");
      setProposal(withProposalDefaults(next));
      showToast("書き直した内容を作成しました。確認して保存してください");
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

  const apply = async () => {
    if (!proposal) return;
    if (!(await confirm("この内容で定石を更新しますか？　更新前の内容は変更履歴へ保存されます。"))) {
      return;
    }
    setApplying(true);
    try {
      await applyPatternEdit(pattern.id, pattern.version, proposal);
      showToast("定石を更新しました");
      await onApplied();
      onClose();
    } catch (caught) {
      if (caught instanceof ConflictError) {
        showToast("別の場所で定石が更新されています。開き直してからやり直してください", "error");
      } else {
        showToast(String(caught), "error");
      }
    } finally {
      setApplying(false);
    }
  };

  return (
    <Modal title={`AIで「${pattern.title}」を編集`} onClose={onClose} wide>
      <div className="space-y-4">
        {error && (
          <div
            className="rounded border p-3 text-sm"
            style={{ borderColor: "rgba(241,106,117,0.5)", color: "var(--danger)" }}
          >
            {error}
          </div>
        )}

        <div>
          <span className="section-label mb-1 block">AIへの指示</span>
          <textarea
            className="input w-full resize-y text-sm"
            rows={3}
            value={instruction}
            disabled={running}
            placeholder="例: 平均値の定理を使う手法を追加して、条件を数式で書いてください"
            onChange={(event) => setInstruction(event.target.value)}
          />
          <div className="mt-2 flex flex-wrap gap-1">
            {PRESETS.map((preset) => (
              <button
                key={preset}
                type="button"
                className="chip"
                disabled={running}
                onClick={() => addPreset(preset)}
              >
                {preset}
              </button>
            ))}
          </div>
          <p className="mt-2 text-[11px]" style={{ color: "var(--muted)" }}>
            指示された箇所だけを書き直します。保存を押すまで定石ライブラリは変わりません。
          </p>
        </div>

        {running ? (
          <div className="space-y-2 py-4 text-center">
            <div className="text-sm font-semibold">
              {job?.progressMessage || "AIが書き直しています…"}
            </div>
            {job && RUNNING.has(job.status) && (
              <button className="btn btn-ghost btn-sm" onClick={() => void cancelJob()}>
                キャンセル
              </button>
            )}
          </div>
        ) : (
          <div className="flex flex-wrap justify-end gap-2">
            <button className="btn btn-ghost" onClick={onClose}>
              閉じる
            </button>
            <button className="btn btn-solid" disabled={!instruction.trim()} onClick={() => void run()}>
              {proposal ? "この指示でやり直す" : "AIに書き直してもらう"}
            </button>
          </div>
        )}

        {proposal && !running && (
          <div className="space-y-3 border-t pt-4" style={{ borderColor: "var(--border)" }}>
            <div className="flex flex-wrap items-center gap-2">
              <span className="section-label mr-auto">書き直した内容（未保存）</span>
              <button className="btn btn-ghost btn-sm" onClick={() => setEditing((value) => !value)}>
                {editing ? "編集を閉じる" : "手直しする"}
              </button>
              <button className="btn btn-ghost btn-sm" onClick={() => setProposal(null)}>
                破棄
              </button>
              <button className="btn btn-solid btn-sm" disabled={applying} onClick={() => void apply()}>
                {applying ? "保存中…" : "この内容で保存"}
              </button>
            </div>
            {editing ? (
              <ProposalEditor proposal={proposal} onChange={setProposal} />
            ) : (
              <ProposalPreview proposal={proposal} />
            )}
          </div>
        )}
      </div>
    </Modal>
  );
}
