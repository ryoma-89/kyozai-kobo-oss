import { useState } from "react";
import { applyPatternProposal } from "../api";
import { useApp } from "../store";
import type { ApplyPatternProposalPayload, PatternProposal } from "../types";
import {
  ProposalEditor,
  ProposalPreview,
  RECOMMENDATION_LABELS,
  TYPE_LABELS,
  existingAction,
  existingButtonLabel,
  withProposalDefaults,
} from "./PatternProposalReview";
import { Modal } from "./ui";

type ProposalStatus =
  | { kind: "pending" }
  | { kind: "rejected" }
  | { kind: "applied"; patternId: number; message: string };

/**
 * Problemに紐づかない定石候補（画像取り込み・AI Chat）のReview一覧。
 * 保存は共通の apply_pattern_proposal を通し、承認するまで定石ライブラリを変更しない。
 */
export function PatternProposalReviewList({
  proposals: initialProposals,
  sourceReference,
  onApplied,
}: {
  proposals: PatternProposal[];
  sourceReference?: string;
  onApplied?: () => void;
}) {
  const { confirm, openPattern, showToast } = useApp();
  const [proposals, setProposals] = useState<PatternProposal[]>(() =>
    initialProposals.map(withProposalDefaults),
  );
  const [status, setStatus] = useState<Record<string, ProposalStatus>>(() =>
    Object.fromEntries(
      initialProposals.map((proposal) => [proposal.proposalId, { kind: "pending" as const }]),
    ),
  );
  const [editing, setEditing] = useState<Record<string, boolean>>({});
  const [applying, setApplying] = useState<string | null>(null);

  const updateProposal = (proposalId: string, proposal: PatternProposal) => {
    setProposals((current) =>
      current.map((item) => (item.proposalId === proposalId ? proposal : item)),
    );
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
        `既存定石「${proposal.matchedPatternTitle ?? ""}」へこの内容を追記します。変更履歴を保存して反映しますか？`,
      );
      if (!accepted) return;
    }
    setApplying(proposal.proposalId);
    try {
      const result = await applyPatternProposal({
        // Problemに紐づかない経路なので problemId は送らない。
        proposal,
        action,
        targetPatternId:
          action === "create_new" || action === "create_child_pattern"
            ? null
            : proposal.matchedPatternId,
        parentPatternId: action === "create_child_pattern" ? proposal.matchedPatternId : null,
        sourceReference,
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

  const pendingCount = proposals.filter(
    (proposal) => (status[proposal.proposalId]?.kind ?? "pending") === "pending",
  ).length;

  return (
    <div className="space-y-4">
      <div
        className="rounded border px-3 py-2 text-xs"
        style={{ borderColor: "var(--border)", background: "var(--panel-2)", color: "var(--muted)" }}
      >
        {proposals.length}件の候補です。まだ保存されていません。内容を確認し、必要なものだけ保存してください。
      </div>

      {proposals.map((proposal, index) => {
        const proposalStatus = status[proposal.proposalId] ?? { kind: "pending" };
        const isEditing = editing[proposal.proposalId] === true;
        return (
          <article key={proposal.proposalId} className="card overflow-hidden">
            <header
              className="flex flex-wrap items-start gap-2 border-b px-4 py-3"
              style={{ borderColor: "var(--border)", background: "var(--panel-2)" }}
            >
              <div>
                <div className="text-[11px]" style={{ color: "var(--muted)" }}>
                  候補 {index + 1}
                </div>
                <h3 className="text-base font-bold">{proposal.title}</h3>
              </div>
              <span className="badge badge-standard ml-auto">{TYPE_LABELS[proposal.patternType]}</span>
              {proposalStatus.kind === "applied" && <span className="badge badge-basic">保存済み</span>}
              {proposalStatus.kind === "rejected" && <span className="badge badge-muted">却下</span>}
            </header>

            <div className="space-y-4 p-4">
              {proposal.matchedPatternId && (
                <div
                  className="rounded border px-3 py-2 text-sm"
                  style={{ borderColor: "rgba(157,108,242,0.45)", background: "var(--purple-dim)" }}
                >
                  <div>
                    <strong>類似する既存定石：</strong>
                    {proposal.matchedPatternTitle}
                  </div>
                  {proposal.similarityReason && (
                    <div className="mt-1 text-xs">{proposal.similarityReason}</div>
                  )}
                  <div className="mt-1 text-xs">
                    <strong>AI推奨：</strong>
                    {RECOMMENDATION_LABELS[proposal.actionRecommendation]}
                  </div>
                </div>
              )}

              {isEditing ? (
                <ProposalEditor
                  proposal={proposal}
                  onChange={(next) => updateProposal(proposal.proposalId, next)}
                />
              ) : (
                <ProposalPreview proposal={proposal} />
              )}

              {proposalStatus.kind === "pending" && (
                <div
                  className="flex flex-wrap justify-end gap-2 border-t pt-3"
                  style={{ borderColor: "var(--border)" }}
                >
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() =>
                      setEditing((current) => ({ ...current, [proposal.proposalId]: !isEditing }))
                    }
                  >
                    {isEditing ? "編集を閉じる" : "編集"}
                  </button>
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() =>
                      setStatus((current) => ({
                        ...current,
                        [proposal.proposalId]: { kind: "rejected" },
                      }))
                    }
                  >
                    却下
                  </button>
                  {proposal.matchedPatternId && (
                    <button
                      className="btn btn-outline btn-sm"
                      disabled={applying === proposal.proposalId}
                      onClick={() => void apply(proposal, existingAction(proposal))}
                    >
                      {existingButtonLabel(proposal)}
                    </button>
                  )}
                  <button
                    className="btn btn-solid btn-sm"
                    disabled={applying === proposal.proposalId}
                    onClick={() => void apply(proposal, "create_new")}
                  >
                    {applying === proposal.proposalId ? "保存中…" : "新規定石として保存"}
                  </button>
                </div>
              )}

              {proposalStatus.kind === "applied" && (
                <div
                  className="flex items-center justify-between rounded border px-3 py-2 text-sm"
                  style={{ borderColor: "var(--border)" }}
                >
                  <span>{proposalStatus.message}</span>
                  <button
                    className="btn btn-outline btn-sm"
                    onClick={() => openPattern(proposalStatus.patternId)}
                  >
                    定石を開く
                  </button>
                </div>
              )}
            </div>
          </article>
        );
      })}

      <div className="text-xs" style={{ color: "var(--muted)" }}>
        未処理 {pendingCount}件
      </div>
    </div>
  );
}

export function PatternProposalReviewDialog({
  proposals,
  sourceReference,
  onClose,
  onApplied,
}: {
  proposals: PatternProposal[];
  sourceReference?: string;
  onClose: () => void;
  onApplied?: () => void;
}) {
  return (
    <Modal title="定石候補を確認して保存" onClose={onClose} wide>
      <PatternProposalReviewList
        proposals={proposals}
        sourceReference={sourceReference}
        onApplied={onApplied}
      />
      <div className="mt-4 flex justify-end border-t pt-3" style={{ borderColor: "var(--border)" }}>
        <button className="btn btn-ghost" onClick={onClose}>
          閉じる
        </button>
      </div>
    </Modal>
  );
}
