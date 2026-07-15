import { useEffect, useMemo, useRef, useState } from "react";
import { addProblemToProject, listAllTags, listProjects, searchProblems } from "../api";
import { useApp } from "../store";
import type { DifficultyRank, ProjectSummary, RequiredFilter, SearchResult } from "../types";
import { DIFFICULTY_RANKS, DifficultyBadge, DifficultyRankBadge, Modal, TagChips } from "./ui";

/** 問題検索画面 */
export function SearchView() {
  const { tree, refreshTree, openProblemInBank, showToast, setContextName, bumps } = useApp();
  const [text, setText] = useState("");
  const [subjectId, setSubjectId] = useState<number | null>(null);
  const [fieldId, setFieldId] = useState<number | null>(null);
  const [unitId, setUnitId] = useState<number | null>(null);
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

  const subject = useMemo(() => tree.find((s) => s.id === subjectId) ?? null, [tree, subjectId]);
  const field = useMemo(
    () => subject?.fields.find((f) => f.id === fieldId) ?? null,
    [subject, fieldId],
  );

  const run = async () => {
    const requestId = ++searchRequestRef.current;
    try {
      const r = await searchProblems({
        text,
        subject_id: subjectId,
        field_id: fieldId,
        unit_id: unitId,
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
  }, [text, subjectId, fieldId, unitId, difficulty, rankFilters, requiredFilter, tag, bumps.problems, bumps.projects]);

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
          placeholder="キーワード検索（タイトル・問題文・タグ・単元名・難易度） Ctrl+F"
        />
        <div className="flex flex-wrap gap-2">
          <select
            value={subjectId ?? ""}
            onChange={(e) => {
              setSubjectId(e.target.value ? Number(e.target.value) : null);
              setFieldId(null);
              setUnitId(null);
            }}
            className="select"
          >
            <option value="">科目: すべて</option>
            {tree.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
          <select
            value={fieldId ?? ""}
            onChange={(e) => {
              setFieldId(e.target.value ? Number(e.target.value) : null);
              setUnitId(null);
            }}
            className="select"
            disabled={!subject}
            title={subject ? undefined : "先に科目を選択すると分野で絞り込めます"}
          >
            <option value="">{subject ? "分野: すべて" : "分野: 先に科目を選択"}</option>
            {subject?.fields.map((f) => (
              <option key={f.id} value={f.id}>
                {f.name}
              </option>
            ))}
          </select>
          <select
            value={unitId ?? ""}
            onChange={(e) => setUnitId(e.target.value ? Number(e.target.value) : null)}
            className="select"
            disabled={!field}
            title={field ? undefined : "先に分野を選択すると単元で絞り込めます"}
          >
            <option value="">{field ? "単元: すべて" : "単元: 先に分野を選択"}</option>
            {field?.units.map((u) => (
              <option key={u.id} value={u.id}>
                {u.name}
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
                    onClick={() => openProblemInBank(r.unit_id, r.id)}
                  >
                    {r.title}
                  </td>
                  <td className="px-2 py-2 text-xs whitespace-nowrap" style={{ color: "var(--muted)" }}>
                    {r.subject_name} / {r.field_name} / {r.unit_name}
                  </td>
                  <td className="px-2 py-2">
                    <span className="flex flex-wrap gap-1">
                      <DifficultyBadge value={r.difficulty} />
                      <DifficultyRankBadge rank={r.difficulty_rank} required={r.is_required} />
                    </span>
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
                      onClick={() => openProblemInBank(r.unit_id, r.id)}
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
