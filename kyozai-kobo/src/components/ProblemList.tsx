import { useEffect, useMemo, useRef, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { createProblem, deleteProblems, exportBank, listProblems, moveProblems } from "../api";
import { useApp } from "../store";
import { isTauri } from "../transport";
import type { BankNode, DifficultyRank, ProblemSummary, RequiredFilter } from "../types";
import { AiConvertDialog } from "./AiConvertDialog";
import { Icon } from "./Icon";
import {
  CompletionBadges,
  DIFFICULTY_RANKS,
  DifficultyBadge,
  DifficultyRankBadge,
  Modal,
  TagChips,
} from "./ui";

type ProblemSortOrder =
  | "created_asc"
  | "created_desc"
  | "updated_desc"
  | "updated_asc"
  | "title_asc"
  | "usage_desc";

const PROBLEM_SORT_STORAGE_KEY = "kyozai-kobo.problem-bank-sort";
const PROBLEM_SORT_OPTIONS: { value: ProblemSortOrder; label: string }[] = [
  { value: "created_asc", label: "追加が古い順" },
  { value: "created_desc", label: "追加が新しい順" },
  { value: "updated_desc", label: "更新が新しい順" },
  { value: "updated_asc", label: "更新が古い順" },
  { value: "title_asc", label: "タイトル順" },
  { value: "usage_desc", label: "使用回数が多い順" },
];

function initialProblemSortOrder(): ProblemSortOrder {
  const stored = localStorage.getItem(PROBLEM_SORT_STORAGE_KEY);
  return PROBLEM_SORT_OPTIONS.some((option) => option.value === stored)
    ? stored as ProblemSortOrder
    : "created_asc";
}

/** 選択中のBankNode直下の問題一覧（複数選択で一括移動・削除・エクスポート） */
export function ProblemList() {
  const { selectedBankNodeId, selectProblem, refreshTree, showToast, confirm, bumps } = useApp();
  const [problems, setProblems] = useState<ProblemSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [showMove, setShowMove] = useState(false);
  const [showAiImport, setShowAiImport] = useState(false);
  const [rankFilters, setRankFilters] = useState<(DifficultyRank | "__unset")[]>([]);
  const [requiredFilter, setRequiredFilter] = useState<RequiredFilter>("all");
  const [sortOrder, setSortOrder] = useState<ProblemSortOrder>(initialProblemSortOrder);
  const seenProblemsBumpRef = useRef(bumps.problems);
  const loadRequestRef = useRef(0);

  const load = async (preserveSelection = false) => {
    if (selectedBankNodeId == null) return;
    const requestId = ++loadRequestRef.current;
    setLoading(true);
    try {
      const nextProblems = await listProblems(selectedBankNodeId);
      if (requestId !== loadRequestRef.current) return;
      const availableIds = new Set(nextProblems.map((problem) => problem.id));
      setProblems(nextProblems);
      setSelected((current) =>
        preserveSelection
          ? new Set([...current].filter((id) => availableIds.has(id)))
          : new Set(),
      );
    } catch (e) {
      if (requestId === loadRequestRef.current) showToast(String(e), "error");
    } finally {
      if (requestId === loadRequestRef.current) setLoading(false);
    }
  };

  useEffect(() => {
    seenProblemsBumpRef.current = bumps.problems;
    void load();
  }, [selectedBankNodeId]);

  useEffect(() => {
    if (seenProblemsBumpRef.current === bumps.problems) return;
    seenProblemsBumpRef.current = bumps.problems;
    void load(true);
  }, [bumps.problems]);

  const onCreate = async () => {
    if (selectedBankNodeId == null) return;
    try {
      const id = await createProblem(selectedBankNodeId, "");
      await refreshTree();
      selectProblem(id);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  // Ctrl+N で新規問題
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        onCreate();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [selectedBankNodeId]);

  const toggleSelect = (id: number) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  const filteredProblems = useMemo(() => {
    const filtered = problems.filter((p) => {
        const rankOk =
          rankFilters.length === 0 ||
          (p.difficulty_rank ? rankFilters.includes(p.difficulty_rank) : rankFilters.includes("__unset"));
        const requiredOk =
          requiredFilter === "all" ||
          (requiredFilter === "required" ? p.is_required : !p.is_required);
        return rankOk && requiredOk;
      });
    return filtered.sort((a, b) => {
      switch (sortOrder) {
        case "created_desc":
          return b.created_at.localeCompare(a.created_at) || b.id - a.id;
        case "updated_desc":
          return b.updated_at.localeCompare(a.updated_at) || b.id - a.id;
        case "updated_asc":
          return a.updated_at.localeCompare(b.updated_at) || a.id - b.id;
        case "title_asc":
          return a.title.localeCompare(b.title, "ja", { numeric: true, sensitivity: "base" }) || a.id - b.id;
        case "usage_desc":
          return b.usage_count - a.usage_count || b.updated_at.localeCompare(a.updated_at) || b.id - a.id;
        case "created_asc":
        default:
          return a.created_at.localeCompare(b.created_at) || a.id - b.id;
      }
    });
  }, [problems, rankFilters, requiredFilter, sortOrder]);

  const changeSortOrder = (value: ProblemSortOrder) => {
    setSortOrder(value);
    localStorage.setItem(PROBLEM_SORT_STORAGE_KEY, value);
  };

  const toggleRankFilter = (rank: DifficultyRank | "__unset") => {
    setRankFilters((current) =>
      current.includes(rank) ? current.filter((r) => r !== rank) : [...current, rank],
    );
  };

  const allChecked = filteredProblems.length > 0 && filteredProblems.every((p) => selected.has(p.id));
  const toggleAll = () => {
    setSelected(allChecked ? new Set() : new Set(filteredProblems.map((p) => p.id)));
  };

  const ids = useMemo(() => [...selected], [selected]);

  const onBulkDelete = async () => {
    if (!(await confirm(`選択した ${ids.length} 件の問題を削除しますか？`))) return;
    try {
      await deleteProblems(ids);
      await refreshTree();
      await load();
      showToast("削除しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onBulkExport = async () => {
    if (!isTauri) {
      showToast("エクスポートはWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const dest = await saveDialog({
        defaultPath: `問題バンク_選択${ids.length}件.json`,
        filters: [{ name: "教材工房 問題バンク", extensions: ["json"] }],
      });
      if (!dest) return;
      await exportBank("problems", null, ids, dest);
      showToast(`エクスポートしました:\n${dest}`);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onBulkMove = async (bankNodeId: number) => {
    try {
      await moveProblems(ids, bankNodeId);
      setShowMove(false);
      await refreshTree();
      await load();
      showToast(`${ids.length}件を移動しました`);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  if (selectedBankNodeId == null) {
    return (
      <div className="flex h-full items-center justify-center text-sm" style={{ color: "var(--muted)" }}>
        左のツリーから階層を選択してください
      </div>
    );
  }

  return (
    <div className="problem-list flex h-full min-w-0 flex-col">
      <div
        className="problem-list-toolbar flex items-center justify-between gap-2 border-b px-4 py-2"
        style={{ borderColor: "var(--border)" }}
      >
        <span className="section-label">問題一覧</span>
        <span className="problem-list-filters flex min-w-0 items-center gap-1">
          <label className="problem-list-mobile-select hidden items-center gap-1 text-xs" style={{ color: "var(--muted)" }}>
            <input type="checkbox" checked={allChecked} onChange={toggleAll} />
            全選択
          </label>
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
          <select
            value={requiredFilter}
            onChange={(e) => setRequiredFilter(e.target.value as RequiredFilter)}
            className="select px-1.5 py-0.5 text-xs"
          >
            <option value="all">★すべて</option>
            <option value="required">★のみ</option>
            <option value="not_required">★以外</option>
          </select>
          <label className="flex items-center gap-1 whitespace-nowrap text-xs" style={{ color: "var(--muted)" }}>
            並び順
            <select
              value={sortOrder}
              onChange={(e) => changeSortOrder(e.target.value as ProblemSortOrder)}
              className="select px-1.5 py-0.5 text-xs"
              aria-label="問題の並び順"
            >
              {PROBLEM_SORT_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </label>
        </span>
        <span className="flex shrink-0 items-center gap-1">
          <button
            onClick={() => setShowAiImport(true)}
            className="btn btn-outline btn-sm"
            title="1枚または複数枚の写真から、複数の問題文を分離して取り込む"
          >
            <Icon name="sparkle" size={15} /> AIで写真取込
          </button>
          <button onClick={onCreate} className="problem-list-new btn btn-solid btn-sm">
            ＋ 新規問題 <span className="problem-list-shortcut">(Ctrl+N)</span>
          </button>
        </span>
      </div>

      {/* 一括操作バー */}
      {selected.size > 0 && (
        <div
          className="fade-in flex items-center gap-2 border-b px-4 py-1.5"
          style={{ borderColor: "rgba(157,108,242,0.42)", background: "var(--accent-dim)" }}
        >
          <span className="text-xs font-bold" style={{ color: "var(--accent)" }}>
            {selected.size}件選択中
          </span>
          <button onClick={() => setShowMove(true)} className="btn btn-outline btn-sm">
            別の階層へ移動
          </button>
          <button onClick={onBulkExport} className="btn btn-ghost btn-sm">
            エクスポート
          </button>
          <button onClick={onBulkDelete} className="btn btn-danger btn-sm">
            削除
          </button>
          <button onClick={() => setSelected(new Set())} className="btn btn-ghost btn-sm ml-auto">
            選択解除
          </button>
        </div>
      )}

      <div className="problem-list-scroll flex-1 overflow-y-auto">
        {loading ? (
          <p className="p-4 text-sm" style={{ color: "var(--muted)" }}>
            読み込み中...
          </p>
        ) : filteredProblems.length === 0 ? (
          <p className="p-4 text-sm" style={{ color: "var(--muted)" }}>
            該当する問題がありません。
          </p>
        ) : (
          <>
          <table className="problem-list-desktop w-full text-sm">
            <thead
              className="sticky top-0 text-left text-[11px]"
              style={{ background: "var(--panel)", color: "var(--muted)" }}
            >
              <tr>
                <th className="w-8 px-2 py-2">
                  <input type="checkbox" checked={allChecked} onChange={toggleAll} title="すべて選択" />
                </th>
                <th className="px-2 py-2 font-normal">タイトル</th>
                <th className="px-2 py-2 font-normal whitespace-nowrap">完成状態</th>
                <th className="px-2 py-2 font-normal">難易度</th>
                <th className="px-2 py-2 font-normal">タグ</th>
                <th className="px-2 py-2 font-normal whitespace-nowrap">最終更新</th>
                <th className="px-2 py-2 font-normal whitespace-nowrap">使用回数</th>
              </tr>
            </thead>
            <tbody>
              {filteredProblems.map((p) => (
                <tr
                  key={p.id}
                  className="cursor-pointer border-b transition-colors hover:bg-[var(--panel-3)]"
                  style={{ borderColor: "var(--border)" }}
                >
                  <td className="px-2 py-2" onClick={(e) => e.stopPropagation()}>
                    <input type="checkbox" checked={selected.has(p.id)} onChange={() => toggleSelect(p.id)} />
                  </td>
                  <td className="px-2 py-2 font-medium" onClick={() => selectProblem(p.id)}>
                    <span className="mr-2">{p.title}</span>
                    <DifficultyRankBadge rank={p.difficulty_rank} required={p.is_required} />
                  </td>
                  <td className="px-2 py-2" onClick={() => selectProblem(p.id)}>
                    <CompletionBadges
                      answerCompleted={p.answer_completed}
                      explanationCompleted={p.explanation_completed}
                    />
                  </td>
                  <td className="px-2 py-2" onClick={() => selectProblem(p.id)}>
                    <DifficultyBadge value={p.difficulty} />
                  </td>
                  <td className="px-2 py-2" onClick={() => selectProblem(p.id)}>
                    <TagChips tags={p.tags} />
                  </td>
                  <td
                    className="px-2 py-2 text-xs whitespace-nowrap"
                    style={{ color: "var(--muted)" }}
                    onClick={() => selectProblem(p.id)}
                  >
                    {p.updated_at}
                  </td>
                  <td
                    className="px-2 py-2 text-center text-xs"
                    style={{ color: "var(--muted)" }}
                    onClick={() => selectProblem(p.id)}
                  >
                    {p.usage_count}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="problem-list-mobile space-y-2 p-2">
            {filteredProblems.map((p) => (
              <div key={p.id} className="card flex min-w-0 items-start gap-2 p-3">
                <input
                  type="checkbox"
                  checked={selected.has(p.id)}
                  onChange={() => toggleSelect(p.id)}
                  className="mt-1 shrink-0"
                  aria-label={`${p.title}を選択`}
                />
                <button onClick={() => selectProblem(p.id)} className="min-w-0 flex-1 text-left">
                  <span className="flex min-w-0 flex-wrap items-center gap-1.5">
                    <span className="min-w-0 flex-1 break-words font-semibold">{p.title}</span>
                    <DifficultyRankBadge rank={p.difficulty_rank} required={p.is_required} />
                  </span>
                  <span className="mt-2 flex flex-wrap items-center gap-1.5">
                    <DifficultyBadge value={p.difficulty} />
                    <CompletionBadges
                      answerCompleted={p.answer_completed}
                      explanationCompleted={p.explanation_completed}
                    />
                    <TagChips tags={p.tags} />
                  </span>
                  <span className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px]" style={{ color: "var(--muted)" }}>
                    <span>更新: {p.updated_at}</span>
                    <span>使用: {p.usage_count}回</span>
                  </span>
                </button>
              </div>
            ))}
          </div>
          </>
        )}
      </div>

      {showMove && (
        <BankNodePicker
          title={`${selected.size}件の問題を移動`}
          excludeBankNodeId={selectedBankNodeId}
          onPick={onBulkMove}
          onClose={() => setShowMove(false)}
        />
      )}
      {showAiImport && (
        <AiConvertDialog
          preset={{
            sourceType: "image",
            mode: "problem_bank_import",
            title: "AIで写真から問題バンクへ取り込む",
          }}
          onClose={() => {
            setShowAiImport(false);
            void load(true);
          }}
        />
      )}
    </div>
  );
}

function flattenBankNodes(nodes: BankNode[], depth = 0): Array<{ node: BankNode; depth: number }> {
  return nodes.flatMap((node) => [
    { node, depth },
    ...flattenBankNodes(node.children, depth + 1),
  ]);
}

/** 問題の保存先・移動先となる任意深度のBankNodeを選ぶモーダル。 */
export function BankNodePicker({
  title,
  excludeBankNodeId,
  onPick,
  onClose,
}: {
  title: string;
  excludeBankNodeId?: number | null;
  onPick: (bankNodeId: number) => void;
  onClose: () => void;
}) {
  const { bankTree } = useApp();
  const nodes = flattenBankNodes(bankTree);
  return (
    <Modal title={title} onClose={onClose}>
      <div className="max-h-[60vh] space-y-0.5 overflow-y-auto">
        {nodes.map(({ node, depth }) => (
          <button
            type="button"
            key={node.id}
            onClick={() => onPick(node.id)}
            disabled={node.id === excludeBankNodeId}
            className="btn btn-ghost btn-sm w-full justify-start disabled:opacity-40"
            style={{ paddingLeft: `${10 + depth * 16}px` }}
            title={node.id === excludeBankNodeId ? "現在の階層" : `${node.name}へ移動`}
          >
            <span className="min-w-0 flex-1 truncate text-left">{node.name}</span>
            <span style={{ color: "var(--muted)" }}>({node.problem_count})</span>
          </button>
        ))}
      </div>
      <p className="mt-2 text-xs" style={{ color: "var(--muted)" }}>
        移動先の階層をクリックしてください。子階層がある場所にも問題を置けます。
      </p>
    </Modal>
  );
}
