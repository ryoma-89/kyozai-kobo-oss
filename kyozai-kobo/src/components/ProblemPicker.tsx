import { useEffect, useMemo, useState } from "react";
import { searchProblems } from "../api";
import { useApp } from "../store";
import type { DifficultyRank, RequiredFilter, SearchResult } from "../types";
import { DIFFICULTY_RANKS, DifficultyBadge, DifficultyRankBadge, Modal } from "./ui";

/** 教材へ追加する問題を問題バンクから選ぶモーダル */
export function ProblemPicker({
  onPick,
  onClose,
}: {
  onPick: (problemId: number) => Promise<void>;
  onClose: () => void;
}) {
  const { tree, refreshTree, showToast } = useApp();
  const [text, setText] = useState("");
  const [subjectId, setSubjectId] = useState<number | null>(null);
  const [fieldId, setFieldId] = useState<number | null>(null);
  const [unitId, setUnitId] = useState<number | null>(null);
  const [rankFilters, setRankFilters] = useState<(DifficultyRank | "__unset")[]>([]);
  const [requiredFilter, setRequiredFilter] = useState<RequiredFilter>("all");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [addedIds, setAddedIds] = useState<number[]>([]);

  useEffect(() => {
    refreshTree();
  }, []);

  const subject = useMemo(() => tree.find((s) => s.id === subjectId) ?? null, [tree, subjectId]);
  const field = useMemo(() => subject?.fields.find((f) => f.id === fieldId) ?? null, [subject, fieldId]);

  useEffect(() => {
    const t = setTimeout(async () => {
      try {
        setResults(
          await searchProblems({
            text,
            subject_id: subjectId,
            field_id: fieldId,
            unit_id: unitId,
            difficulty_ranks: rankFilters.length ? rankFilters : null,
            required_filter: requiredFilter === "all" ? null : requiredFilter,
          }),
        );
      } catch (e) {
        showToast(String(e), "error");
      }
    }, 250);
    return () => clearTimeout(t);
  }, [text, subjectId, fieldId, unitId, rankFilters, requiredFilter]);

  const toggleRankFilter = (rank: DifficultyRank | "__unset") => {
    setRankFilters((current) =>
      current.includes(rank) ? current.filter((r) => r !== rank) : [...current, rank],
    );
  };

  return (
    <Modal title="問題バンクから追加" onClose={onClose} wide>
      <div className="mb-2 flex flex-wrap gap-2">
        <input
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          className="input min-w-40 flex-1"
          placeholder="キーワード検索"
        />
        <select
          value={subjectId ?? ""}
          onChange={(e) => {
            setSubjectId(e.target.value ? Number(e.target.value) : null);
            setFieldId(null);
            setUnitId(null);
          }}
          className="select text-xs"
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
          className="select text-xs"
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
          className="select text-xs"
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
            該当する問題がありません。
          </p>
        ) : (
          <ul className="space-y-1">
            {results.map((r) => (
              <li key={r.id} className="card flex items-center gap-2 px-3 py-1.5 text-sm">
                <span className="min-w-0 flex-1 truncate">
                  <span className="font-medium">{r.title}</span>
                  <span className="ml-2 text-xs" style={{ color: "var(--muted)" }}>
                    {r.subject_name}/{r.field_name}/{r.unit_name}
                  </span>
                </span>
                <DifficultyBadge value={r.difficulty} />
                <DifficultyRankBadge rank={r.difficulty_rank} required={r.is_required} />
                <button
                  onClick={async () => {
                    await onPick(r.id);
                    setAddedIds((ids) => [...ids, r.id]);
                  }}
                  className="btn btn-outline btn-sm"
                >
                  {addedIds.includes(r.id) ? "再追加" : "追加"}
                </button>
                {addedIds.includes(r.id) && (
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
        同じ問題を複数回追加することもできます。追加した時点の内容がスナップショットとして保存されます。
      </p>
    </Modal>
  );
}
