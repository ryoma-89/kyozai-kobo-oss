import { useEffect, useState } from "react";
import { aiCancelJob, aiDeleteJob, aiGetJob, aiListJobs, aiRetryJob } from "../api";
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

/** AI変換のジョブ履歴・新規変換 */
export function AiJobsView() {
  const { showToast, confirm, bumps } = useApp();
  const [jobs, setJobs] = useState<AiJob[]>([]);
  const [openNew, setOpenNew] = useState(false);
  const [reviewJob, setReviewJob] = useState<AiJob | null>(null);

  const load = async () => {
    try {
      setJobs(await aiListJobs(100));
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
      setReviewJob(await aiGetJob(job.id));
    } catch (e) {
      showToast(String(e), "error");
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
                    <button onClick={() => onCancel(job)} className="btn btn-ghost btn-sm">
                      キャンセル
                    </button>
                  ) : (
                    <>
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
