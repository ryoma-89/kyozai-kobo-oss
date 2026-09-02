import { useEffect, useMemo, useState } from "react";
import { listAllPartTags, listPartCategories, searchParts } from "../api";
import { useApp } from "../store";
import type { BankNode, DifficultyRank, PartSummary, RequiredFilter } from "../types";
import { DIFFICULTY_RANKS, DifficultyRankBadge, Modal, TagChips } from "./ui";

const PART_TYPE_LABEL: Record<string, string> = {
  heading: "見出し",
  text: "本文",
  notice: "注意",
  hint: "ヒント",
  example: "例題",
  homework: "宿題",
  reflection: "振り返り",
  box: "枠",
  table: "表",
  image_block: "画像",
  latex_snippet: "LaTeX",
  page_break: "改ページ",
  custom: "カスタム",
};

function flattenBankNodes(nodes: BankNode[], depth = 0): { node: BankNode; depth: number }[] {
  return nodes.flatMap((node) => [
    { node, depth },
    ...flattenBankNodes(node.children, depth + 1),
  ]);
}

export function PartPicker({
  onPick,
  onClose,
}: {
  onPick: (partId: number) => Promise<void>;
  onClose: () => void;
}) {
  const { showToast, bankTree, refreshTree } = useApp();
  const [text, setText] = useState("");
  const [bankNodeId, setBankNodeId] = useState<number | null>(null);
  const [partType, setPartType] = useState("");
  const [category, setCategory] = useState("");
  const [tag, setTag] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [categories, setCategories] = useState<string[]>([]);
  const [rankFilters, setRankFilters] = useState<(DifficultyRank | "__unset")[]>([]);
  const [requiredFilter, setRequiredFilter] = useState<RequiredFilter>("all");
  const [results, setResults] = useState<PartSummary[]>([]);
  const [addedIds, setAddedIds] = useState<number[]>([]);
  const bankOptions = useMemo(() => flattenBankNodes(bankTree), [bankTree]);

  useEffect(() => {
    void refreshTree();
    listAllPartTags().then(setTags).catch(() => {});
    listPartCategories().then(setCategories).catch(() => {});
  }, []);

  useEffect(() => {
    const t = setTimeout(async () => {
      try {
        setResults(
          await searchParts({
            text,
            bank_node_id: bankNodeId,
            include_descendants: true,
            part_type: partType || null,
            category: category || null,
            tag: tag || null,
            difficulty_ranks: rankFilters.length ? rankFilters : null,
            required_filter: requiredFilter === "all" ? null : requiredFilter,
          }),
        );
      } catch (e) {
        showToast(String(e), "error");
      }
    }, 250);
    return () => clearTimeout(t);
  }, [text, bankNodeId, partType, category, tag, rankFilters, requiredFilter]);

  const toggleRankFilter = (rank: DifficultyRank | "__unset") => {
    setRankFilters((current) =>
      current.includes(rank) ? current.filter((r) => r !== rank) : [...current, rank],
    );
  };

  return (
    <Modal title="部品ライブラリから追加" onClose={onClose} wide>
      <div className="mb-2 flex flex-wrap gap-2">
        <input
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          className="input min-w-44 flex-1"
          placeholder="タイトル・本文・タグ検索"
        />
        <select value={partType} onChange={(e) => setPartType(e.target.value)} className="select text-xs">
          <option value="">種類: すべて</option>
          {Object.entries(PART_TYPE_LABEL).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
        <select
          value={bankNodeId ?? ""}
          onChange={(e) => setBankNodeId(e.target.value ? Number(e.target.value) : null)}
          className="select text-xs"
        >
          <option value="">階層: すべて</option>
          {bankOptions.map(({ node, depth }) => (
            <option key={node.id} value={node.id}>
              {`${"　".repeat(depth)}${depth > 0 ? "└ " : ""}${node.name}`}
            </option>
          ))}
        </select>
        <select value={category} onChange={(e) => setCategory(e.target.value)} className="select text-xs">
          <option value="">カテゴリ: すべて</option>
          {categories.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
        <select value={tag} onChange={(e) => setTag(e.target.value)} className="select text-xs">
          <option value="">タグ: すべて</option>
          {tags.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
        <span className="flex items-center gap-1">
          {DIFFICULTY_RANKS.map((r) => (
            <button
              key={r.rank}
              onClick={() => toggleRankFilter(r.rank)}
              className={`btn btn-sm ${rankFilters.includes(r.rank) ? "btn-outline" : "btn-ghost"}`}
              title={`${r.rank}: ${r.description}`}
            >
              {r.rank}
            </button>
          ))}
          <button
            onClick={() => toggleRankFilter("__unset")}
            className={`btn btn-sm ${rankFilters.includes("__unset") ? "btn-outline" : "btn-ghost"}`}
          >
            未設定
          </button>
        </span>
        <select
          value={requiredFilter}
          onChange={(e) => setRequiredFilter(e.target.value as RequiredFilter)}
          className="select text-xs"
        >
          <option value="all">★すべて</option>
          <option value="required">★のみ</option>
          <option value="not_required">★以外</option>
        </select>
      </div>

      <div className="max-h-[55vh] overflow-y-auto">
        {results.length === 0 ? (
          <p className="py-6 text-center text-sm" style={{ color: "var(--muted)" }}>
            該当する部品がありません。
          </p>
        ) : (
          <ul className="space-y-1">
            {results.map((part) => (
              <li key={part.id} className="card flex items-center gap-2 px-3 py-2 text-sm">
                <span className="min-w-0 flex-1">
                  <span className="font-medium">{part.title}</span>
                  <span className="ml-2 text-xs" style={{ color: "var(--muted)" }}>
                    {PART_TYPE_LABEL[part.part_type] ?? part.part_type} / {part.category || "カテゴリなし"} / v
                    {part.version}
                  </span>
                  <span className="mt-1 block truncate text-xs" style={{ color: "var(--muted)" }}>
                    {part.bank_path || "階層: 未分類"}
                  </span>
                  <span className="mt-1 block truncate text-xs" style={{ color: "var(--muted)" }}>
                    {part.plain_text_preview || "プレビューなし"}
                  </span>
                </span>
                <TagChips tags={part.tags.slice(0, 3)} />
                <DifficultyRankBadge rank={part.difficulty_rank} required={part.is_required} muted />
                <button
                  onClick={async () => {
                    await onPick(part.id);
                    setAddedIds((ids) => [...ids, part.id]);
                  }}
                  className="btn btn-outline btn-sm"
                >
                  {addedIds.includes(part.id) ? "再追加" : "追加"}
                </button>
                {addedIds.includes(part.id) && (
                  <span className="text-xs" style={{ color: "var(--success)" }}>
                    ✓済
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      <p className="mt-2 text-xs" style={{ color: "var(--muted)" }}>
        追加した時点の部品内容・添付ファイル情報が教材内にスナップショット保存されます。
      </p>
    </Modal>
  );
}
