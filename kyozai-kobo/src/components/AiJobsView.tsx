import { useEffect, useState } from "react";
import {
  aiApplySourceRevision,
  aiCancelJob,
  aiDeleteJob,
  aiGetJob,
  aiInsertIntoTargetProblem,
  aiListJobs,
  aiRetryJob,
  aiUpdateJobLatex,
} from "../api";
import { useApp } from "../store";
import type { AiJob, AiJobStatus } from "../types";
import { AiConvertDialog, AiJobReviewModal } from "./AiConvertDialog";

const STATUS_LABELS: Record<AiJobStatus, string> = {
  queued: "順番待ち",
  preprocessing: "前処理中",
  waiting_for_codex: "Codex接続中",
  converting: "変換中",
  validating: "検証中",
  compiling: "コンパイル中",
  completed: "完了",
  failed: "失敗",
  cancelled: "キャンセル",
};

const RUNNING: AiJobStatus[] = [
  "queued",
  "preprocessing",
  "waiting_for_codex",
  "converting",
  "validating",
  "compiling",
];

function jobModeLabel(job: AiJob): string | null {
  const labels: Record<string, string> = {
    generate_answer: "解答生成",
    generate_explanation: "解説生成",
    generate_strategy_explanation: "確定解答から解説生成",
    generate_strategy_solution: "選択解法から答案生成",
    generate_topic_guide: "解説部品生成",
    generate_problem_layouts: "表示形式生成",
    problem_bank_import: "問題取込",
    revise_source: "ソース修正",
    project_review: "教材全体のAI確認",
    content_review: "問題・部品のAIチェック",
  };
  return labels[job.conversionMode] ?? null;
}

function StatusBadge({ status }: { status: AiJobStatus }) {
  const style =
    status === "completed"
      ? { color: "var(--success)", borderColor: "rgba(197,183,223,0.4)", background: "var(--success-dim)" }
      : status === "failed"
        ? { color: "var(--danger)", borderColor: "rgba(241,106,117,0.4)", background: "var(--danger-dim)" }
        : status === "cancelled"
          ? undefined
          : { color: "var(--warn)", borderColor: "rgba(251,191,36,0.4)", background: "var(--warn-dim)" };
  return (
    <span className={`badge ${!style ? "badge-muted" : ""}`} style={style}>
      {STATUS_LABELS[status] ?? status}
    </span>
  );
}

function directInsertLabel(job: AiJob): "解答" | "解説" | null {
  if (
    job.conversionMode === "revise_source"
    || job.status !== "completed"
    || job.compileStatus !== "ok"
    || !!job.insertedAt
    || job.targetEntityType !== "problem"
    || job.targetEntityId === null
  ) {
    return null;
  }
  if (job.targetField === "answer_latex") return "解答";
  if (job.targetField === "explanation_latex") return "解説";
  return null;
}

function sourceRevisionLabel(job: AiJob): "問題文" | "二段組用問題文" | "解答" | "解説" | "部品" | null {
  if (
    job.conversionMode !== "revise_source"
    || job.status !== "completed"
    || job.compileStatus !== "ok"
    || !!job.insertedAt
    || job.targetEntityId === null
    || typeof job.options.revisionSourceVersion !== "number"
    || job.options.revisionApplied === true
  ) {
    return null;
  }
  if (job.targetEntityType === "part" && job.targetField === "latex_source") return "部品";
  if (job.targetEntityType !== "problem") return null;
  if (job.targetField === "statement_latex") return "問題文";
  if (job.targetField === "statement_latex_two_column") return "二段組用問題文";
  if (job.targetField === "answer_latex") return "解答";
  if (job.targetField === "explanation_latex") return "解説";
  return null;
}

function targetFieldLabel(field: string): string | null {
  if (field === "statement_latex" || field === "snap_statement") return "問題文";
  if (field === "statement_latex_two_column" || field === "snap_statement_two_column") {
    return "二段組用問題文";
  }
  if (field === "answer_latex" || field === "snap_answer") return "解答";
  if (field === "explanation_latex" || field === "snap_explanation") return "解説";
  if (field === "latex_source") return "部品本文";
  if (field === "review") return "AIチェック";
  return null;
}

function targetDisplayLabel(job: AiJob): string | null {
  if (job.targetEntityId === null) return null;
  const name = job.targetEntityName.trim();
  const named = (kind: string) => name ? `${kind}「${name}」` : `${kind} #${job.targetEntityId}`;
  let target: string;
  if (job.targetEntityType === "problem") {
    target = named("問題");
  } else if (job.targetEntityType === "problem_batch") {
    const count = job.structuredResult?.problems?.length ?? 0;
    target = named("問題");
    if (count > 1) target += `ほか${count - 1}件`;
  } else if (job.targetEntityType === "part") {
    target = named("部品");
  } else if (job.targetEntityType === "project") {
    target = named("教材");
  } else if (job.targetEntityType === "template") {
    target = named("テンプレート");
  } else if (job.targetEntityType === "project_item") {
    target = name ? `教材項目「${name}」` : `教材項目 #${job.targetEntityId}`;
  } else {
    return null;
  }
  const field = targetFieldLabel(job.targetField);
  return field ? `${target}・${field}` : target;
}

function generatedEntityLabel(job: AiJob): string | null {
  if (targetDisplayLabel(job)) return null;
  const problems = job.structuredResult?.problems ?? [];
  if (problems.length > 0) {
    const first = problems[0].title.trim() || "名称未設定";
    return problems.length === 1
      ? `問題候補「${first}」`
      : `問題候補「${first}」ほか${problems.length - 1}件`;
  }
  if (job.conversionMode === "generate_topic_guide") {
    const title = job.inputText.split(/\r?\n/).map((line) => line.trim()).find(Boolean);
    if (title) return `部品候補「${title.slice(0, 50)}」`;
  }
  return null;
}

/** AI変換のジョブ履歴・新規変換 */
export function AiJobsView() {
  const { showToast, confirm, bumps } = useApp();
  const [jobs, setJobs] = useState<AiJob[]>([]);
  const [openNew, setOpenNew] = useState(false);
  const [reviewJob, setReviewJob] = useState<AiJob | null>(null);
  const [insertingJobId, setInsertingJobId] = useState<number | null>(null);

  const load = async () => {
    try {
      setJobs((await aiListJobs(100)).filter((job) => job.options.hideFromHistory !== true));
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  useEffect(() => {
    load();
  }, [bumps.ai_job]);

  // 実行中ジョブがあれば定期更新
  useEffect(() => {
    if (!jobs.some((j) => RUNNING.includes(j.status))) return;
    const timer = setInterval(load, 3000);
    return () => clearInterval(timer);
  }, [jobs.map((j) => j.status).join(",")]);

  const onCancel = async (job: AiJob) => {
    try {
      await aiCancelJob(job.id);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRetry = async (job: AiJob) => {
    try {
      const j = await aiRetryJob(job.id);
      setReviewJob(j);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDelete = async (job: AiJob) => {
    if (!(await confirm(`ジョブ #${job.id} を削除しますか？\n（入力画像・プレビューPDFも削除されます）`))) return;
    try {
      await aiDeleteJob(job.id);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onOpen = async (job: AiJob) => {
    try {
      // 保存済みジョブにも現在の検査規則を適用し、旧バージョンの誤警告を
      // 開いた直後から残さない。同一LaTeXの更新ではコンパイル結果は維持される。
      if (job.status === "completed" && job.outputLatex.trim()) {
        await aiUpdateJobLatex(job.id, job.outputLatex);
      }
      const refreshedJob = await aiGetJob(job.id);
      setReviewJob(refreshedJob);
      setJobs((current) => current.map((item) => item.id === job.id ? refreshedJob : item));
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDirectInsert = async (job: AiJob) => {
    const label = directInsertLabel(job);
    if (!label || job.targetEntityId === null) return;
    const problemName = job.targetEntityName.trim()
      ? `問題「${job.targetEntityName.trim()}」`
      : `問題 #${job.targetEntityId}`;
    const hasReviewItems = job.warnings.length > 0 || job.uncertainFragments.length > 0;
    const backgroundExplanation = job.options.backgroundWorkflowResult === "solution_explanation";
    const accepted = await confirm(
      `${problemName}の${label}へ生成結果を${backgroundExplanation ? "反映" : "挿入"}しますか？\n`
      + (backgroundExplanation
        ? "生成元の解答が変更されている場合は、安全のため反映を中止します。"
        : "既存の内容がある場合は末尾へ追記します。")
      + (hasReviewItems ? "\n警告・要確認箇所があるため、内容を確認してから実行してください。" : ""),
    );
    if (!accepted) return;
    setInsertingJobId(job.id);
    try {
      await aiInsertIntoTargetProblem(job.id, true);
      showToast(`${problemName}の${label}へ${backgroundExplanation ? "反映" : "挿入"}しました`);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setInsertingJobId(null);
    }
  };

  const onApplyRevision = async (job: AiJob) => {
    const label = sourceRevisionLabel(job);
    if (!label || job.targetEntityId === null) return;
    const targetName = job.targetEntityName.trim()
      ? `${job.targetEntityType === "part" ? "部品" : "問題"}「${job.targetEntityName.trim()}」`
      : job.targetEntityType === "part"
        ? `部品 #${job.targetEntityId}`
        : `問題 #${job.targetEntityId}`;
    const hasReviewItems = job.warnings.length > 0 || job.uncertainFragments.length > 0;
    const accepted = await confirm(
      `${targetName}の${label}をAIの修正結果で置き換えますか？\n`
      + "AI修正の開始後に対象が更新されている場合は、安全のため適用を中止します。"
      + (hasReviewItems ? "\n警告・要確認箇所があるため、内容を確認してから実行してください。" : ""),
    );
    if (!accepted) return;
    setInsertingJobId(job.id);
    try {
      await aiApplySourceRevision(job.id, true);
      showToast(`${targetName}の${label}をAIの修正結果で置き換えました`);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setInsertingJobId(null);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b px-4 py-2" style={{ borderColor: "var(--border)" }}>
        <h1 className="text-sm font-bold">AI変換（写真・テキスト → LaTeX）</h1>
        <button onClick={() => setOpenNew(true)} className="btn btn-solid btn-sm ml-auto">
          ＋ 新しい変換
        </button>
        <button onClick={load} className="btn btn-ghost btn-sm" title="一覧を更新">
          ⟳
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {jobs.length === 0 ? (
          <div className="py-16 text-center text-sm" style={{ color: "var(--muted)" }}>
            <p className="mb-3">まだ変換履歴がありません。</p>
            <p className="text-xs">
              「＋ 新しい変換」から、プリントの写真や貼り付けたテキストをLaTeXへ変換できます。
              <br />
              問題編集画面・部品編集画面の「AI変換」ボタンからは、変換結果をそのままエディタへ挿入できます。
            </p>
          </div>
        ) : (
          <ul className="space-y-2">
            {jobs.map((job) => (
              <li key={job.id} className="card flex flex-wrap items-center gap-2 px-3 py-2">
                <span className="font-mono text-xs" style={{ color: "var(--muted)" }}>
                  #{job.id}
                </span>
                <StatusBadge status={job.status} />
                <span className="badge badge-muted">
                  {job.sourceType === "image" ? `画像${job.inputAssetPaths.length}枚` : "テキスト"}
                </span>
                {jobModeLabel(job) && (
                  <span className="badge badge-muted">{jobModeLabel(job)}</span>
                )}
                {(targetDisplayLabel(job) || generatedEntityLabel(job)) && (
                  <span className="badge badge-muted">
                    {targetDisplayLabel(job) || generatedEntityLabel(job)}
                  </span>
                )}
                {!!job.insertedAt && (
                  <span
                    className="badge"
                    title={`挿入・反映日時: ${job.insertedAt}`}
                    style={{
                      color: "var(--success)",
                      borderColor: "rgba(197,183,223,0.4)",
                      background: "var(--success-dim)",
                    }}
                  >
                    挿入済み
                  </span>
                )}
                <span className="min-w-0 flex-1 truncate text-xs">
                  {job.status === "failed"
                    ? job.errorMessage
                    : job.outputLatex
                      ? job.outputLatex.slice(0, 80)
                      : job.progressMessage}
                </span>
                {job.compileStatus === "ok" && (
                  <span className="badge" style={{ color: "var(--success)", borderColor: "rgba(197,183,223,0.4)" }}>
                    PDF✓
                  </span>
                )}
                <span className="text-[10px] whitespace-nowrap" style={{ color: "var(--muted)" }}>
                  {job.createdAt}
                </span>
                <span className="flex gap-1">
                  {RUNNING.includes(job.status) ? (
                    <>
                      <button onClick={() => onOpen(job)} className="btn btn-outline btn-sm">
                        進捗
                      </button>
                      <button onClick={() => onCancel(job)} className="btn btn-ghost btn-sm">
                        キャンセル
                      </button>
                    </>
                  ) : (
                    <>
                      {sourceRevisionLabel(job) && (
                        <button
                          onClick={() => void onApplyRevision(job)}
                          disabled={insertingJobId !== null}
                          className="btn btn-solid btn-sm"
                        >
                          {insertingJobId === job.id
                            ? "適用中..."
                            : `${sourceRevisionLabel(job)}を置き換え`}
                        </button>
                      )}
                      {directInsertLabel(job) && (
                        <button
                          onClick={() => void onDirectInsert(job)}
                          disabled={insertingJobId !== null}
                          className="btn btn-solid btn-sm"
                        >
                          {insertingJobId === job.id
                            ? "反映中..."
                            : job.options.backgroundWorkflowResult === "solution_explanation"
                              ? "解説を問題へ反映"
                              : `${directInsertLabel(job)}へ挿入`}
                        </button>
                      )}
                      <button onClick={() => onOpen(job)} className="btn btn-outline btn-sm">
                        開く
                      </button>
                      <button onClick={() => onRetry(job)} className="btn btn-ghost btn-sm" title="同じ入力で再変換">
                        再実行
                      </button>
                      <button onClick={() => onDelete(job)} className="btn btn-danger btn-sm">
                        削除
                      </button>
                    </>
                  )}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {openNew && <AiConvertDialog onClose={() => { setOpenNew(false); load(); }} />}
      {reviewJob && <AiJobReviewModal job={reviewJob} onClose={() => { setReviewJob(null); load(); }} />}
    </div>
  );
}
