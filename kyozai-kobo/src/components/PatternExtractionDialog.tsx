import { useEffect, useMemo, useRef, useState } from "react";
import {
  aiCancelJob,
  aiGetJob,
  applyPatternProposal,
  startPatternExtraction,
  startPatternGeneralization,
} from "../api";
import { useApp } from "../store";
import type {
  AiJob,
  AiPatternExtractionResult,
  ApplyPatternProposalPayload,
  PatternExtractionStyle,
  PatternProposal,
  ProblemFull,
  ProblemPatternRelationType,
} from "../types";
import {
  MAX_GENERALIZATION_PASSES,
  ProposalEditor,
  ProposalPreview,
  RECOMMENDATION_LABELS,
  TYPE_LABELS,
  existingAction,
  existingButtonLabel,
  withProposalDefaults,
} from "./PatternProposalReview";
import { Modal } from "./ui";

const RUNNING = new Set([
  "queued",
  "preprocessing",
  "waiting_for_codex",
  "converting",
  "validating",
  "compiling",
]);

const STYLE_LABELS: Record<PatternExtractionStyle, string> = {
  standard: "標準",
  more_general: "もっと一般的に",
  exam_pattern_focused: "入試の定石単位で",
  custom: "指示を書く",
};

type ProposalStatus =
  | { kind: "pending" }
  | { kind: "rejected" }
  | { kind: "applied"; patternId: number; message: string };

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function extractionJobKey(problemId: number): string {
  return `kk-pattern-extraction-job-${problemId}`;
}

function rememberedExtractionJob(problemId: number): number | null {
  try {
    const value = Number(localStorage.getItem(extractionJobKey(problemId)));
    return Number.isInteger(value) && value > 0 ? value : null;
  } catch {
    return null;
  }
}

function rememberExtractionJob(problemId: number, jobId: number): void {
  try {
    localStorage.setItem(extractionJobKey(problemId), String(jobId));
  } catch {
    // localStorageを利用できない環境でも、現在のダイアログ内では処理を継続する。
  }
}

function forgetExtractionJob(problemId: number): void {
  try {
    localStorage.removeItem(extractionJobKey(problemId));
  } catch {
    // 保存された再開情報がない場合と同じ扱いにする。
  }
}

/** ジョブが終わるまで待つ。ダイアログが閉じられたら途中の状態のまま返す。 */
async function waitForJob(
  job: AiJob,
  alive: () => boolean,
  onProgress: (job: AiJob) => void,
): Promise<AiJob> {
  let current = job;
  onProgress(current);
  while (RUNNING.has(current.status) && alive()) {
    await sleep(1100);
    if (!alive()) return current;
    current = await aiGetJob(current.id);
    onProgress(current);
  }
  return current;
}

function extractionResult(job: AiJob): AiPatternExtractionResult {
  const result = job.structuredResult;
  if (!result || !("kind" in result) || result.kind !== "pattern-extraction") {
    throw new Error("AIから定石候補の構造化データを取得できませんでした");
  }
  return result as AiPatternExtractionResult;
}

export function PatternExtractionDialog({
  problem,
  onClose,
  onApplied,
}: {
  problem: ProblemFull;
  onClose: () => void;
  onApplied?: () => void;
}) {
  const { confirm, openPattern, showToast } = useApp();
  const [job, setJob] = useState<AiJob | null>(null);
  const [proposals, setProposals] = useState<PatternProposal[]>([]);
  const [status, setStatus] = useState<Record<string, ProposalStatus>>({});
  const [editing, setEditing] = useState<Record<string, boolean>>({});
  const [relations, setRelations] = useState<Record<string, ProblemPatternRelationType | "none">>({});
  const [applying, setApplying] = useState<string | null>(null);
  const [generalizing, setGeneralizing] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);
  const [retryStyle, setRetryStyle] = useState<PatternExtractionStyle>("standard");
  const [retryInstruction, setRetryInstruction] = useState("");
  // 再抽出は成功してから差し替える。前回の候補は比較・復帰のために保持する。
  const [previousProposals, setPreviousProposals] = useState<PatternProposal[] | null>(null);
  // 「さらに一般化」で内容が悪化した場合に、保存前へ戻せるようにする。
  const [beforeGeneralization, setBeforeGeneralization] = useState<Record<string, PatternProposal>>({});
  const [error, setError] = useState("");
  const activeRef = useRef(true);
  const startedRef = useRef(false);
  const jobRef = useRef<AiJob | null>(null);

  useEffect(() => {
    activeRef.current = true;
    if (!startedRef.current) {
      startedRef.current = true;
      void (async () => {
        try {
          let current: AiJob | null = null;
          const rememberedJobId = rememberedExtractionJob(problem.id);
          if (rememberedJobId) {
            try {
              const remembered = await aiGetJob(rememberedJobId);
              const sourceVersion = Number(remembered.options.patternExtractionSourceVersion);
              if (
                remembered.conversionMode === "pattern_extraction" &&
                remembered.targetEntityId === problem.id &&
                sourceVersion === problem.version &&
                remembered.status !== "failed" &&
                remembered.status !== "cancelled"
              ) {
                current = remembered;
              } else {
                forgetExtractionJob(problem.id);
              }
            } catch {
              forgetExtractionJob(problem.id);
            }
          }
          if (!current) {
            current = await startPatternExtraction(problem.id);
            // 閉じる操作が直後に行われても、開始済みジョブへ戻れるよう先に記録する。
            rememberExtractionJob(problem.id, current.id);
          }
          if (!activeRef.current) return;
          jobRef.current = current;
          setJob(current);
          while (RUNNING.has(current.status) && activeRef.current) {
            await sleep(1100);
            if (!activeRef.current) return;
            current = await aiGetJob(current.id);
            jobRef.current = current;
            setJob(current);
          }
          if (!activeRef.current) return;
          if (current.status !== "completed") {
            forgetExtractionJob(problem.id);
            throw new Error(current.errorMessage || "定石候補の抽出に失敗しました");
          }
          const result = extractionResult(current);
          if (result.patterns.length === 0) throw new Error("再利用可能な定石候補が見つかりませんでした");
          setProposals(result.patterns.map(withProposalDefaults));
          setStatus(Object.fromEntries(result.patterns.map((proposal) => [proposal.proposalId, { kind: "pending" }])));
          setRelations(Object.fromEntries(result.patterns.map((proposal) => [
            proposal.proposalId,
            proposal.sourceType === "ai_inferred" ? "applicable" : "used",
          ])));
        } catch (caught) {
          if (activeRef.current) setError(String(caught));
        }
      })();
    }
    return () => {
      activeRef.current = false;
      // ダイアログを閉じてもジョブは停止しない。明示的なキャンセルだけが停止する。
    };
  }, [problem.id]);

  const pendingCount = useMemo(
    () => proposals.filter((proposal) => (status[proposal.proposalId]?.kind ?? "pending") === "pending").length,
    [proposals, status],
  );

  const updateProposal = (proposalId: string, proposal: PatternProposal) => {
    setProposals((current) => current.map((item) => item.proposalId === proposalId ? proposal : item));
  };

  const cancelJob = async () => {
    if (!job || !RUNNING.has(job.status)) return;
    try {
      await aiCancelJob(job.id);
      const current = await aiGetJob(job.id);
      jobRef.current = current;
      setJob(current);
      forgetExtractionJob(problem.id);
    } catch (caught) {
      setError(String(caught));
    }
  };

  const closeDialog = () => {
    if (!error && proposals.length === 0 && (!job || RUNNING.has(job.status))) {
      showToast("定石抽出はバックグラウンドで続行します。もう一度開くと進捗・結果へ戻れます");
    }
    onClose();
  };

  /** 新しい候補一覧を、付随する状態ごと差し替える。 */
  const adoptProposals = (next: PatternProposal[]) => {
    const normalized = next.map(withProposalDefaults);
    setProposals(normalized);
    setStatus(Object.fromEntries(normalized.map((item) => [item.proposalId, { kind: "pending" }])));
    setEditing({});
    setBeforeGeneralization({});
    setRelations(
      Object.fromEntries(
        normalized.map((item) => [
          item.proposalId,
          item.sourceType === "ai_inferred" ? "applicable" : "used",
        ]),
      ),
    );
  };

  const retryExtraction = async () => {
    if (retryStyle === "custom" && !retryInstruction.trim()) {
      showToast("方針に「指示を書く」を選んだ場合は追加指示を入力してください", "error");
      return;
    }
    setRetrying(true);
    try {
      const started = await startPatternExtraction(
        problem.id,
        retryStyle,
        retryInstruction.trim() || undefined,
      );
      rememberExtractionJob(problem.id, started.id);
      const finished = await waitForJob(started, () => activeRef.current, (current) => {
        jobRef.current = current;
        setJob(current);
      });
      if (!activeRef.current) return;
      if (finished.status !== "completed") {
        throw new Error(finished.errorMessage || "抽出のやり直しに失敗しました");
      }
      const result = extractionResult(finished);
      if (result.patterns.length === 0) {
        throw new Error("再抽出で定石候補が見つかりませんでした");
      }
      // 成功したときだけ差し替える。失敗しても前回の候補は残る。
      setPreviousProposals(proposals);
      adoptProposals(result.patterns);
      showToast(`${result.patterns.length}件の候補で抽出し直しました`);
    } catch (caught) {
      showToast(String(caught), "error");
    } finally {
      setRetrying(false);
    }
  };

  const restorePreviousProposals = () => {
    if (!previousProposals) return;
    adoptProposals(previousProposals);
    setPreviousProposals(null);
  };

  const generalize = async (proposal: PatternProposal) => {
    setGeneralizing(proposal.proposalId);
    try {
      let current = await startPatternGeneralization(problem.id, proposal);
      while (RUNNING.has(current.status) && activeRef.current) {
        await sleep(1100);
        current = await aiGetJob(current.id);
      }
      if (!activeRef.current) return;
      if (current.status !== "completed") {
        throw new Error(current.errorMessage || "さらに一般化できませんでした");
      }
      const next = extractionResult(current).patterns[0];
      if (!next) throw new Error("一般化した定石候補を取得できませんでした");
      const improved = withProposalDefaults({ ...next, proposalId: proposal.proposalId });
      setBeforeGeneralization((state) => ({ ...state, [proposal.proposalId]: proposal }));
      updateProposal(proposal.proposalId, improved);
      showToast(
        improved.generalizationDecision === "keep_as_is"
          ? "AIはこの粒度が定石として適切と判断しました"
          : "さらに一般化しました。内容を確認してから保存してください",
      );
    } catch (caught) {
      showToast(String(caught), "error");
    } finally {
      setGeneralizing(null);
    }
  };

  const undoGeneralization = (proposalId: string) => {
    const previous = beforeGeneralization[proposalId];
    if (!previous) return;
    updateProposal(proposalId, previous);
    setBeforeGeneralization((state) => {
      const next = { ...state };
      delete next[proposalId];
      return next;
    });
  };

  const apply = async (proposal: PatternProposal, action: ApplyPatternProposalPayload["action"]) => {
    if (action !== "create_new" && !proposal.matchedPatternId) {
      showToast("追加先の既存定石が見つかりません", "error");
      return;
    }
    if (action === "create_child_pattern") {
      const accepted = await confirm(
        `既存定石「${proposal.matchedPatternTitle ?? ""}」の特殊化として新しい定石を作成し、上位・下位の関連を追加します。よろしいですか？`,
      );
      if (!accepted) return;
    } else if (action !== "create_new" && action !== "link_existing") {
      const accepted = await confirm(
        `既存定石「${proposal.matchedPatternTitle ?? ""}」へAI候補を追記します。変更履歴を保存して反映しますか？`,
      );
      if (!accepted) return;
    }
    setApplying(proposal.proposalId);
    try {
      const relation = relations[proposal.proposalId] ?? "none";
      const result = await applyPatternProposal({
        problemId: problem.id,
        proposal,
        action,
        targetPatternId:
          action === "create_new" || action === "create_child_pattern"
            ? null
            : proposal.matchedPatternId,
        parentPatternId: action === "create_child_pattern" ? proposal.matchedPatternId : null,
        linkRelationType: relation === "none" ? null : relation,
      });
      const message = result.created
        ? action === "create_child_pattern"
          ? `定石 #${result.patternId} を特殊化として作成しました`
          : `定石 #${result.patternId} を作成しました`
        : action === "link_existing"
          ? `既存定石 #${result.patternId} を関連付けました`
          : `既存定石 #${result.patternId} を更新しました`;
      setStatus((current) => ({
        ...current,
        [proposal.proposalId]: { kind: "applied", patternId: result.patternId, message },
      }));
      setEditing((current) => ({ ...current, [proposal.proposalId]: false }));
      showToast(message);
      onApplied?.();
    } catch (caught) {
      showToast(String(caught), "error");
    } finally {
      setApplying(null);
    }
  };

  return (
    <Modal title={`「${problem.title}」から定石を抽出`} onClose={closeDialog} wide>
      {error ? (
        <div className="space-y-4">
          <div className="rounded border p-4 text-sm" style={{ borderColor: "rgba(241,106,117,0.5)", color: "var(--danger)" }}>
            {error}
          </div>
          <div className="flex justify-end"><button className="btn btn-ghost" onClick={closeDialog}>閉じる</button></div>
        </div>
      ) : proposals.length === 0 ? (
        <div className="space-y-4 py-8 text-center">
          <div className="text-sm font-semibold">{job?.progressMessage || "抽出ジョブを準備しています…"}</div>
          <p className="text-xs" style={{ color: "var(--muted)" }}>
            問題固有の設定を除き、日本の高校数学で一般的な用語・表記のSituation・候補手法・適用条件へ一般化しています。
            閉じても抽出はバックグラウンドで続き、同じ問題から再度開くと復帰できます。
          </p>
          {job && RUNNING.has(job.status) && (
            <button className="btn btn-ghost btn-sm" onClick={() => void cancelJob()}>キャンセル</button>
          )}
        </div>
      ) : (
        <div className="space-y-4">
          <div className="rounded border px-3 py-2 text-xs" style={{ borderColor: "var(--border)", background: "var(--panel-2)", color: "var(--muted)" }}>
            {proposals.length}件の候補を抽出しました。用語・数式表記は日本の高校数学教材の基準で検証済みです。AI候補は未保存で、各候補の反映方法を選んだときだけ定石ライブラリへ保存されます。
          </div>

          <div className="rounded border p-3" style={{ borderColor: "var(--border)" }}>
            <div className="flex flex-wrap items-center gap-2">
              <span className="section-label">抽出をやり直す</span>
              <select
                className="input text-xs"
                value={retryStyle}
                disabled={retrying}
                onChange={(event) => setRetryStyle(event.target.value as PatternExtractionStyle)}
              >
                {Object.entries(STYLE_LABELS).map(([value, label]) => (
                  <option key={value} value={value}>{label}</option>
                ))}
              </select>
              <button className="btn btn-outline btn-sm" disabled={retrying} onClick={() => void retryExtraction()}>
                {retrying ? "再抽出中…" : "やり直す"}
              </button>
              {previousProposals && !retrying && (
                <button className="btn btn-ghost btn-sm" onClick={restorePreviousProposals}>
                  前回の候補に戻す
                </button>
              )}
              <span className="text-[11px]" style={{ color: "var(--muted)" }}>
                新しい結果が出るまで、いまの候補はそのまま残ります
              </span>
            </div>
            <textarea
              className="input mt-2 w-full resize-y text-xs"
              rows={2}
              disabled={retrying}
              value={retryInstruction}
              placeholder="追加の指示（任意）　例: 最後の計算よりも、場合分けを減らす発想を抽出して"
              onChange={(event) => setRetryInstruction(event.target.value)}
            />
          </div>

          {proposals.map((proposal, index) => {
            const proposalStatus = status[proposal.proposalId] ?? { kind: "pending" };
            const isEditing = editing[proposal.proposalId] === true;
            return (
              <article key={proposal.proposalId} className="card overflow-hidden">
                <header className="flex flex-wrap items-start gap-2 border-b px-4 py-3" style={{ borderColor: "var(--border)", background: "var(--panel-2)" }}>
                  <div>
                    <div className="text-[11px]" style={{ color: "var(--muted)" }}>候補 {index + 1}</div>
                    <h3 className="text-base font-bold">{proposal.title}</h3>
                  </div>
                  <span className="badge badge-standard ml-auto">{TYPE_LABELS[proposal.patternType]}</span>
                  {proposalStatus.kind === "applied" && <span className="badge badge-basic">反映済み</span>}
                  {proposalStatus.kind === "rejected" && <span className="badge badge-muted">却下</span>}
                </header>

                <div className="space-y-4 p-4">
                  {!proposal.matchedPatternId && proposal.actionRecommendation === "ignore" && (
                    <div className="rounded border px-3 py-2 text-sm" style={{ borderColor: "rgba(241,106,117,0.5)", color: "var(--danger)" }}>
                      <strong>AI推奨：</strong>定石として保存する価値が低いと判定されました。内容を編集するか却下してください。
                    </div>
                  )}

                  {proposal.matchedPatternId && (
                    <div className="rounded border px-3 py-2 text-sm" style={{ borderColor: "rgba(157,108,242,0.45)", background: "var(--purple-dim)" }}>
                      <div><strong>類似する既存定石：</strong>{proposal.matchedPatternTitle}</div>
                      {proposal.similarityReason && <div className="mt-1 text-xs">{proposal.similarityReason}</div>}
                      <div className="mt-1 text-xs"><strong>AI推奨：</strong>{RECOMMENDATION_LABELS[proposal.actionRecommendation]}</div>
                    </div>
                  )}

                  {isEditing ? (
                    <ProposalEditor proposal={proposal} onChange={(next) => updateProposal(proposal.proposalId, next)} />
                  ) : (
                    <ProposalPreview proposal={proposal} />
                  )}

                  {proposalStatus.kind === "pending" && (
                    <div className="border-t pt-3" style={{ borderColor: "var(--border)" }}>
                      <div className="mb-3 flex flex-wrap items-center gap-2">
                        <span className="section-label">Problemとの関連</span>
                        <select
                          className="input text-xs"
                          value={relations[proposal.proposalId] ?? "none"}
                          onChange={(event) => setRelations((current) => ({
                            ...current,
                            [proposal.proposalId]: event.target.value as ProblemPatternRelationType | "none",
                          }))}
                        >
                          <option value="used">使用した定石</option>
                          <option value="applicable">候補となる定石</option>
                          <option value="none">関連付けない</option>
                        </select>
                        <span className="text-[11px]" style={{ color: "var(--muted)" }}>保存ボタンを押すまで反映されません</span>
                      </div>
                      <div className="flex flex-wrap justify-end gap-2">
                        <button className="btn btn-ghost btn-sm" onClick={() => setEditing((current) => ({ ...current, [proposal.proposalId]: !isEditing }))}>
                          {isEditing ? "編集を閉じる" : "編集"}
                        </button>
                        <button
                          className="btn btn-outline btn-sm"
                          disabled={
                            generalizing !== null ||
                            applying === proposal.proposalId ||
                            proposal.generalizationPassCount >= MAX_GENERALIZATION_PASSES
                          }
                          title={
                            proposal.generalizationPassCount >= MAX_GENERALIZATION_PASSES
                              ? `一般化は最大${MAX_GENERALIZATION_PASSES}回までです`
                              : proposal.generalizationDecision === "keep_as_is"
                                ? "AIはこの粒度が定石として適切と判断しています。実行しても内容が変わらないことがあります"
                                : "問題固有の要素をさらに取り除いた定石へ変換します"
                          }
                          onClick={() => void generalize(proposal)}
                        >
                          {generalizing === proposal.proposalId ? "一般化中…" : "さらに一般化"}
                        </button>
                        {beforeGeneralization[proposal.proposalId] && (
                          <button className="btn btn-ghost btn-sm" onClick={() => undoGeneralization(proposal.proposalId)}>
                            一般化前へ戻す
                          </button>
                        )}
                        <button className="btn btn-ghost btn-sm" onClick={() => setStatus((current) => ({ ...current, [proposal.proposalId]: { kind: "rejected" } }))}>
                          却下
                        </button>
                        {proposal.matchedPatternId && (
                          <button disabled={applying === proposal.proposalId} className="btn btn-outline btn-sm" onClick={() => void apply(proposal, existingAction(proposal))}>
                            {existingButtonLabel(proposal)}
                          </button>
                        )}
                        <button disabled={applying === proposal.proposalId} className="btn btn-solid btn-sm" onClick={() => void apply(proposal, "create_new")}>
                          {applying === proposal.proposalId ? "反映中…" : "新規定石として作成"}
                        </button>
                      </div>
                    </div>
                  )}

                  {proposalStatus.kind === "applied" && (
                    <div className="flex items-center justify-between rounded border px-3 py-2 text-sm" style={{ borderColor: "var(--border)" }}>
                      <span>{proposalStatus.message}</span>
                      <button className="btn btn-outline btn-sm" onClick={() => openPattern(proposalStatus.patternId)}>定石を開く</button>
                    </div>
                  )}
                </div>
              </article>
            );
          })}

          <div className="flex items-center justify-between border-t pt-3" style={{ borderColor: "var(--border)" }}>
            <span className="text-xs" style={{ color: "var(--muted)" }}>未処理 {pendingCount}件</span>
            <button className="btn btn-ghost" onClick={closeDialog}>閉じる</button>
          </div>
        </div>
      )}
    </Modal>
  );
}
