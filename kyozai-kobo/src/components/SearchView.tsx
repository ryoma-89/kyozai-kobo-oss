import { useEffect, useRef, useState } from "react";
import { addProblemToProject, listAllTags, listProjects, searchProblems } from "../api";
import { useApp } from "../store";
import type { BankNode, DifficultyRank, ProjectSummary, RequiredFilter, SearchResult } from "../types";
import {
  CompletionBadges,
  DIFFICULTY_RANKS,
  DifficultyBadge,
  DifficultyRankBadge,
  Modal,
  TagChips,
} from "./ui";

function bankNodeOptions(nodes: BankNode[], depth = 0): Array<{ node: BankNode; label: string }> {
  return nodes.flatMap((node) => [
    { node, label: `${"　".repeat(depth)}${depth ? "└ " : ""}${node.name}` },
    ...bankNodeOptions(node.children, depth + 1),
  ]);
}

/** 問題検索画面 */
export function SearchView() {
  const { bankTree, refreshTree, openProblemInBank, showToast, setContextName, bumps } = useApp();
  const [text, setText] = useState("");
  const [bankNodeId, setBankNodeId] = useState<number | null>(null);
  const [difficulty, setDifficulty] = useState("");
  const [rankFilters, setRankFilters] = useState<(DifficultyRank | "__unset")[]>([]);
  const [requiredFilter, setRequiredFilter] = useState<RequiredFilter>("all");
  const [tag, setTag] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [results, setResults] = useState<SearchResult[] | null>(null);
  const [addTarget, setAddTarget] = useState<SearchResult | null>(null);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const seenProblemsBumpRef = useRef(bumps.problems);
  const searchRequestRef = useRef(0);

  useEffect(() => {
    setContextName("検索");
    refreshTree();
    listAllTags().then(setTags).catch(() => {});
    inputRef.current?.focus();
    return () => setContextName("");
  }, []);

  useEffect(() => {
    if (seenProblemsBumpRef.current === bumps.problems) return;
    seenProblemsBumpRef.current = bumps.problems;
    listAllTags().then(setTags).catch(() => {});
  }, [bumps.problems]);

  const run = async () => {
    const requestId = ++searchRequestRef.current;
    try {
      const r = await searchProblems({
        text,
        bank_node_id: bankNodeId,
        include_descendants: true,
        difficulty: difficulty || null,
        difficulty_ranks: rankFilters.length ? rankFilters : null,
        required_filter: requiredFilter === "all" ? null : requiredFilter,
        tag: tag || null,
      });
      if (requestId !== searchRequestRef.current) return;
      setResults(r);
    } catch (e) {
      if (requestId === searchRequestRef.current) showToast(String(e), "error");
    }
  };

  // 条件変更で自動検索（入力はデバウンス）
  useEffect(() => {
    const t = setTimeout(run, 250);
    return () => clearTimeout(t);
  }, [text, bankNodeId, difficulty, rankFilters, requiredFilter, tag, bumps.problems, bumps.projects]);

  const toggleRankFilter = (rank: DifficultyRank | "__unset") => {
    setRankFilters((current) =>
      current.includes(rank) ? current.filter((r) => r !== rank) : [...current, rank],
    );
  };

  const openAddModal = async (r: SearchResult) => {
    try {
      setProjects(await listProjects());
      setAddTarget(r);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const addTo = async (projectId: number) => {
    if (!addTarget) return;
    try {
      await addProblemToProject(projectId, addTarget.id);
      setAddTarget(null);
      showToast("教材に追加しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="space-y-2 border-b px-4 py-3" style={{ borderColor: "var(--border)" }}>
        <input
          ref={inputRef}
          data-search-input
          value={text}
          onChange={(e) => setText(e.target.value)}
          className="input w-full"
          placeholder="キーワード検索（タイトル・問題文・タグ・階層名・難易度） Ctrl+F"
        />
        <div className="flex flex-wrap gap-2">
          <select
            value={bankNodeId ?? ""}
            onChange={(e) => setBankNodeId(e.target.value ? Number(e.target.value) : null)}
            className="select"
          >
            <option value="">階層: 全体</option>
            {bankNodeOptions(bankTree).map(({ node, label }) => (
              <option key={node.id} value={node.id}>
                {label}
              </option>
            ))}
          </select>
          <select value={difficulty} onChange={(e) => setDifficulty(e.target.value)} className="select">
            <option value="">難易度: すべて</option>
            <option>基礎</option>
            <option>標準</option>
            <option>発展</option>
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
            className="select"
          >
            <option value="all">★: すべて</option>
            <option value="required">★のみ</option>
            <option value="not_required">★以外</option>
          </select>
          <select value={tag} onChange={(e) => setTag(e.target.value)} className="select">
            <option value="">タグ: すべて</option>
            {tags.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {results == null ? (
          <p className="p-4 text-sm" style={{ color: "var(--muted)" }}>
            検索中...
          </p>
        ) : results.length === 0 ? (
          <p className="p-4 text-sm" style={{ color: "var(--muted)" }}>
            該当する問題がありません。
          </p>
        ) : (
          <table className="w-full text-sm">
            <thead
              className="sticky top-0 text-left text-[11px]"
              style={{ background: "var(--panel)", color: "var(--muted)" }}
            >
              <tr>
                <th className="px-4 py-2 font-normal">タイトル</th>
                <th className="px-2 py-2 font-normal whitespace-nowrap">場所</th>
                <th className="px-2 py-2 font-normal">難易度</th>
                <th className="px-2 py-2 font-normal whitespace-nowrap">完成状態</th>
                <th className="px-2 py-2 font-normal">タグ</th>
                <th className="px-2 py-2 font-normal whitespace-nowrap">更新</th>
                <th className="px-2 py-2 font-normal whitespace-nowrap">使用</th>
                <th className="px-2 py-2 font-normal"></th>
              </tr>
            </thead>
            <tbody>
              {results.map((r) => (
                <tr
                  key={r.id}
                  className="border-b transition-colors hover:bg-[var(--panel-3)]"
                  style={{ borderColor: "var(--border)" }}
                >
                  <td
                    className="cursor-pointer px-4 py-2 font-medium"
                    onClick={() => openProblemInBank(r.bank_node_id, r.id)}
                  >
                    {r.title}
                  </td>
                  <td className="px-2 py-2 text-xs whitespace-nowrap" style={{ color: "var(--muted)" }}>
                    {r.bank_path}
                  </td>
                  <td className="px-2 py-2">
                    <span className="flex flex-wrap gap-1">
                      <DifficultyBadge value={r.difficulty} />
                      <DifficultyRankBadge rank={r.difficulty_rank} required={r.is_required} />
                    </span>
                  </td>
                  <td className="px-2 py-2">
                    <CompletionBadges
                      answerCompleted={r.answer_completed}
                      explanationCompleted={r.explanation_completed}
                    />
                  </td>
                  <td className="px-2 py-2">
                    <TagChips tags={r.tags} />
                  </td>
                  <td className="px-2 py-2 text-xs whitespace-nowrap" style={{ color: "var(--muted)" }}>
                    {r.updated_at}
                  </td>
                  <td className="px-2 py-2 text-center text-xs" style={{ color: "var(--muted)" }}>
                    {r.usage_count}
                  </td>
                  <td className="px-2 py-2 whitespace-nowrap">
                    <button
                      onClick={() => openProblemInBank(r.bank_node_id, r.id)}
                      className="btn btn-ghost btn-sm mr-1"
                    >
                      開く
                    </button>
                    <button onClick={() => openAddModal(r)} className="btn btn-outline btn-sm">
                      教材へ追加
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {addTarget && (
        <Modal title={`「${addTarget.title}」を教材へ追加`} onClose={() => setAddTarget(null)}>
          {projects.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--muted)" }}>
              教材プロジェクトがありません。先に「教材」画面で作成してください。
            </p>
          ) : (
            <ul className="space-y-1">
              {projects.map((p) => (
                <li key={p.id}>
                  <button onClick={() => addTo(p.id)} className="card card-glow w-full px-3 py-2 text-left text-sm">
                    <span className="font-medium">{p.name}</span>
                    <span className="ml-2 text-xs" style={{ color: "var(--muted)" }}>
                      {p.item_count}問 / {p.updated_at}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Modal>
      )}
    </div>
  );
}
