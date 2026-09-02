import { useState } from "react";
import type {
  ApplyPatternProposalPayload,
  PatternProposal,
  PatternProposalStrategy,
  PatternType,
} from "../types";
import { LatexPreview } from "./LatexPreview";

export const TYPE_LABELS: Record<PatternType, string> = {
  strategy: "方針",
  technique: "手法",
  calculation_tip: "計算",
  check: "確認",
};

export const SOURCE_LABELS: Record<string, string> = {
  solution_used: "既存解答で使用",
  explanation_used: "既存解説で使用",
  ai_inferred: "AIによる推定",
  image_import: "画像から取り込み",
  ai_chat: "AI Chatで作成",
  manual: "手動で作成",
};

export const RECOMMENDATION_LABELS: Record<string, string> = {
  create_new: "新規定石を作成",
  create_child_pattern: "既存定石の特殊化として作成",
  merge_into_existing: "既存定石へ統合",
  add_candidate_to_existing: "既存定石へ候補を追加",
  add_caution_to_existing: "既存定石へ注意を追加",
  add_example_to_existing: "既存定石へ具体例を追加",
  duplicate: "既存定石と重複",
  ignore: "採用非推奨",
};

export const SPECIFICITY_LABELS: Record<number, string> = {
  1: "抽象度: 原理",
  2: "抽象度: 方針",
  3: "抽象度: 手法",
  4: "抽象度: 特化",
};

/** バックエンドの PATTERN_GENERALIZATION_MAX_PASSES と一致させる。 */
export const MAX_GENERALIZATION_PASSES = 2;

export const DECISION_LABELS: Record<string, string> = {
  generalize: "もう一段一般化できる",
  keep_as_is: "この粒度が定石として適切",
  split_general_and_specific: "上位定石と特殊化に分けるとよい",
};

export function reusabilityLabel(score: number): { label: string; tone: string } {
  if (score >= 0.75) return { label: "再利用性: 高い", tone: "badge-basic" };
  if (score >= 0.45) return { label: "再利用性: ふつう", tone: "badge-standard" };
  return { label: "再利用性: 低い", tone: "badge-warn" };
}

/**
 * 抽象化に関する項目を持たない旧ジョブの結果からも復帰できるように、
 * 表示・保存の前に既定値を補う。バックエンドでも同じ既定値へ寄せている。
 */
export function withProposalDefaults(proposal: PatternProposal): PatternProposal {
  const level = proposal.specificityLevel;
  return {
    ...proposal,
    rawTechnique: proposal.rawTechnique ?? "",
    generalizationReason: proposal.generalizationReason ?? "",
    specificityLevel:
      level === 1 || level === 2 || level === 3 || level === 4
        ? level
        : proposal.patternType === "strategy" || proposal.patternType === "check"
          ? 2
          : 3,
    reusabilityScore:
      typeof proposal.reusabilityScore === "number" && Number.isFinite(proposal.reusabilityScore)
        ? Math.min(1, Math.max(0, proposal.reusabilityScore))
        : 0,
    searchConcepts: proposal.searchConcepts ?? [],
    isOverlySpecific: proposal.isOverlySpecific === true,
    isOverlyGeneral: proposal.isOverlyGeneral === true,
    specificityReason: proposal.specificityReason ?? "",
    possibleParentPattern: proposal.possibleParentPattern ?? null,
    // 方針を持たない旧Proposalは、粒度を勝手に動かさない側の既定にする。
    generalizationDecision: proposal.generalizationDecision ?? "keep_as_is",
    recommendedStorage: proposal.recommendedStorage ?? "new_pattern",
    generalizationPassCount: Math.max(0, proposal.generalizationPassCount ?? 0),
  };
}

export function splitValues(value: string): string[] {
  return value
    .split(/[、,\n]/)
    .map((item) => item.trim())
    .filter((item, index, all) => item && all.indexOf(item) === index);
}

export function joinValues(values: string[]): string {
  return values.join("、");
}

export function Field({
  label,
  value,
  onChange,
  rows = 2,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  rows?: number;
}) {
  return (
    <label className="block">
      <span className="section-label mb-1 block">{label}</span>
      <textarea
        value={value}
        rows={rows}
        onChange={(event) => onChange(event.target.value)}
        className="input w-full resize-y text-sm"
      />
    </label>
  );
}

export function ProposalEditor({
  proposal,
  onChange,
}: {
  proposal: PatternProposal;
  onChange: (proposal: PatternProposal) => void;
}) {
  const patch = (fields: Partial<PatternProposal>) => onChange({ ...proposal, ...fields });
  const patchStrategy = (index: number, fields: Partial<PatternProposalStrategy>) => {
    const strategies = proposal.strategies.map((strategy, strategyIndex) =>
      strategyIndex === index ? { ...strategy, ...fields } : strategy,
    );
    patch({ strategies });
  };
  const moveStrategy = (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= proposal.strategies.length) return;
    const strategies = [...proposal.strategies];
    [strategies[index], strategies[target]] = [strategies[target], strategies[index]];
    patch({ strategies: strategies.map((strategy, sortOrder) => ({ ...strategy, sortOrder: sortOrder + 1 })) });
  };

  return (
    <div className="space-y-3">
      <div className="grid gap-3 sm:grid-cols-[1fr_170px_180px]">
        <label className="block">
          <span className="section-label mb-1 block">タイトル</span>
          <input
            value={proposal.title}
            onChange={(event) => patch({ title: event.target.value })}
            className="input w-full text-sm"
          />
        </label>
        <label className="block">
          <span className="section-label mb-1 block">種類</span>
          <select
            value={proposal.patternType}
            onChange={(event) => patch({ patternType: event.target.value as PatternType })}
            className="input w-full text-sm"
          >
            {Object.entries(TYPE_LABELS).map(([value, label]) => (
              <option key={value} value={value}>{label}</option>
            ))}
          </select>
        </label>
        <label className="block">
          <span className="section-label mb-1 block">抽出根拠</span>
          <select
            value={proposal.sourceType}
            onChange={(event) => patch({ sourceType: event.target.value as PatternProposal["sourceType"] })}
            className="input w-full text-sm"
          >
            {Object.entries(SOURCE_LABELS).map(([value, label]) => (
              <option key={value} value={value}>{label}</option>
            ))}
          </select>
        </label>
      </div>
      <Field
        label="元問題で使われた手法（一般化の材料。保存時は具体例として使われます）"
        value={proposal.rawTechnique}
        onChange={(rawTechnique) => patch({ rawTechnique })}
        rows={3}
      />

      <div>
        <div className="mb-2 flex items-center justify-between">
          <span className="section-label">候補手法</span>
          <button
            type="button"
            className="btn btn-outline btn-sm"
            onClick={() => patch({
              strategies: [
                ...proposal.strategies,
                { title: "", description: "", condition: "", reasoning: "", sortOrder: proposal.strategies.length + 1 },
              ],
            })}
          >
            ＋ 候補を追加
          </button>
        </div>
        <div className="space-y-2">
          {proposal.strategies.map((strategy, index) => (
            <div key={`${proposal.proposalId}-strategy-${index}`} className="card space-y-2 p-3">
              <div className="flex items-center gap-2">
                <strong className="text-xs">候補 {index + 1}</strong>
                <button type="button" className="btn btn-ghost btn-sm ml-auto" disabled={index === 0} onClick={() => moveStrategy(index, -1)}>↑</button>
                <button type="button" className="btn btn-ghost btn-sm" disabled={index === proposal.strategies.length - 1} onClick={() => moveStrategy(index, 1)}>↓</button>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  disabled={proposal.strategies.length === 1}
                  onClick={() => patch({ strategies: proposal.strategies.filter((_, itemIndex) => itemIndex !== index) })}
                >
                  削除
                </button>
              </div>
              <input
                value={strategy.title}
                placeholder="候補名"
                onChange={(event) => patchStrategy(index, { title: event.target.value })}
                className="input w-full text-sm"
              />
              <Field label="説明" value={strategy.description} onChange={(description) => patchStrategy(index, { description })} />
            </div>
          ))}
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Domain" value={joinValues(proposal.domains)} onChange={(value) => patch({ domains: splitValues(value) })} />
        <Field label="Goal" value={joinValues(proposal.goals)} onChange={(value) => patch({ goals: splitValues(value) })} />
        <Field label="Operation" value={joinValues(proposal.operations)} onChange={(value) => patch({ operations: splitValues(value) })} />
        <Field label="Structure" value={joinValues(proposal.structures)} onChange={(value) => patch({ structures: splitValues(value) })} />
        <Field label="Situation分類" value={joinValues(proposal.situations)} onChange={(value) => patch({ situations: splitValues(value) })} />
        <Field label="タグ" value={joinValues(proposal.tags)} onChange={(value) => patch({ tags: splitValues(value) })} />
      </div>
    </div>
  );
}

export function ProposalPreview({ proposal }: { proposal: PatternProposal }) {
  const [showAllMetadata, setShowAllMetadata] = useState(false);
  const metadata = [
    ...proposal.domains.map((value) => `分野: ${value}`),
    ...proposal.goals.map((value) => `目的: ${value}`),
    ...proposal.operations.map((value) => `操作: ${value}`),
    ...proposal.structures.map((value) => `構造: ${value}`),
    ...proposal.situations.map((value) => `状況: ${value}`),
    ...proposal.tags,
  ];
  // 分類値が多いと定石の中身がチップに埋もれる。既定は畳んでおく。
  const METADATA_PREVIEW = 4;
  const visibleMetadata = showAllMetadata ? metadata : metadata.slice(0, METADATA_PREVIEW);
  const hiddenMetadataCount = metadata.length - visibleMetadata.length;
  const reusability = reusabilityLabel(proposal.reusabilityScore);
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="badge badge-standard">{TYPE_LABELS[proposal.patternType]}</span>
        <span className={`badge ${proposal.sourceType === "ai_inferred" ? "badge-warn" : "badge-basic"}`}>
          {SOURCE_LABELS[proposal.sourceType]}
        </span>
        <span className="badge badge-muted">{SPECIFICITY_LABELS[proposal.specificityLevel]}</span>
        <span className={`badge ${reusability.tone}`}>{reusability.label}</span>
        <span className={`badge ${proposal.generalizationDecision === "generalize" ? "badge-warn" : "badge-muted"}`}>
          {DECISION_LABELS[proposal.generalizationDecision] ?? proposal.generalizationDecision}
        </span>
        {proposal.generalizationPassCount > 0 && (
          <span className="badge badge-muted">一般化 {proposal.generalizationPassCount}回</span>
        )}
        {visibleMetadata.map((value) => <span key={value} className="chip">{value}</span>)}
        {hiddenMetadataCount > 0 && (
          <button type="button" className="chip" onClick={() => setShowAllMetadata(true)}>
            ＋{hiddenMetadataCount}件の分類
          </button>
        )}
        {showAllMetadata && metadata.length > METADATA_PREVIEW && (
          <button type="button" className="chip" onClick={() => setShowAllMetadata(false)}>
            分類を畳む
          </button>
        )}
      </div>

      {proposal.rawTechnique && (
        <section className="rounded border p-3" style={{ borderColor: "var(--border)", background: "var(--panel-2)" }}>
          <div className="section-label mb-1">元問題で使われた手法</div>
          <LatexPreview source={proposal.rawTechnique} />
          <div className="my-2 text-center text-sm" style={{ color: "var(--muted)" }}>↓ 一般化</div>
          <div className="section-label mb-1">一般化された定石</div>
          <div className="text-sm font-bold">{proposal.title}</div>
          {proposal.generalizationReason && (
            <div className="mt-1 text-xs" style={{ color: "var(--muted)" }}>{proposal.generalizationReason}</div>
          )}
        </section>
      )}

      {!proposal.rawTechnique && proposal.generalizationReason && (
        <div className="rounded border px-3 py-2 text-xs" style={{ borderColor: "var(--border)", color: "var(--muted)" }}>
          <strong>粒度の判断：</strong>{proposal.generalizationReason}
        </div>
      )}

      {(proposal.isOverlySpecific || proposal.isOverlyGeneral) && (
        <div className="rounded border px-3 py-2 text-xs" style={{ borderColor: "rgba(241,106,117,0.5)", color: "var(--danger)" }}>
          <strong>
            {proposal.isOverlySpecific ? "具体的すぎる可能性があります" : "一般的すぎる可能性があります"}
          </strong>
          {proposal.specificityReason && <div className="mt-1">{proposal.specificityReason}</div>}
        </div>
      )}

      {proposal.possibleParentPattern && (
        <div className="rounded border px-3 py-2 text-xs" style={{ borderColor: "var(--border)" }}>
          <strong>より一般的な定石の候補：</strong>{proposal.possibleParentPattern.title}
          {proposal.possibleParentPattern.reason && (
            <div className="mt-1" style={{ color: "var(--muted)" }}>{proposal.possibleParentPattern.reason}</div>
          )}
        </div>
      )}

      <section>
        <div className="section-label mb-2">候補手法</div>
        <div className="space-y-2">
          {proposal.strategies.map((strategy, index) => (
            <div key={`${proposal.proposalId}-preview-${index}`} className="card p-3">
              <strong>{index + 1}. {strategy.title}</strong>
              {strategy.description && <div className="mt-2"><LatexPreview source={strategy.description} /></div>}
            </div>
          ))}
        </div>
      </section>
      {proposal.searchConcepts.length > 0 && (
        <section>
          <div className="section-label mb-1">既存定石の検索に使った概念</div>
          <div className="flex flex-wrap gap-1">
            {proposal.searchConcepts.map((concept) => <span key={concept} className="chip">{concept}</span>)}
          </div>
        </section>
      )}
    </div>
  );
}

export function existingAction(proposal: PatternProposal): ApplyPatternProposalPayload["action"] {
  switch (proposal.actionRecommendation) {
    case "duplicate": return "link_existing";
    case "ignore": return "link_existing";
    case "add_candidate_to_existing": return "add_candidate_to_existing";
    case "add_caution_to_existing": return "add_caution_to_existing";
    case "add_example_to_existing": return "add_example_to_existing";
    case "create_child_pattern": return "create_child_pattern";
    default: return "merge_into_existing";
  }
}

export function existingButtonLabel(proposal: PatternProposal): string {
  switch (existingAction(proposal)) {
    case "link_existing": return "既存定石を関連付け";
    case "add_candidate_to_existing": return "既存へ候補追加";
    case "add_caution_to_existing": return "既存へ注意追加";
    case "add_example_to_existing": return "既存へ具体例追加";
    case "create_child_pattern": return "特殊化として作成";
    default: return "既存へ統合";
  }
}
