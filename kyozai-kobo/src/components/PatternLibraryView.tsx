import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import {
  createPattern,
  deletePattern,
  duplicatePattern,
  exportPatternsFile,
  exportPatternsJson,
  getPattern,
  getPatternDeleteImpact,
  getPatternVersion,
  importPatternsFile,
  importPatternsJson,
  linkPatternRelation,
  linkProblemPattern,
  listPatternFilterValues,
  listPatternVersions,
  restorePatternVersion,
  searchPatterns,
  unlinkPatternRelation,
  unlinkProblemPattern,
  updatePattern,
} from "../api";
import { useApp } from "../store";
import { ConflictError, isTauri } from "../transport";
import type {
  ImportPatternsResult,
  PatternFacets,
  PatternFilterValues,
  PatternFull,
  PatternSnapshot,
  PatternStrategyInput,
  PatternSummary,
  PatternUpdate,
  PatternVersionFull,
  PatternVersionSummary,
} from "../types";
import { LatexPreview } from "./LatexPreview";
import {
  PATTERN_TYPE_LABELS,
  PatternPicker,
  PatternTypeBadge,
  patternTypeLabel,
} from "./PatternPicker";
import { PatternAiEditDialog } from "./PatternAiEditDialog";
import { PatternImageImportDialog } from "./PatternImageImportDialog";
import { ProblemPicker } from "./ProblemPicker";
import { Modal, TagChips } from "./ui";

/** pattern_relations.relation_type の表示名。未知の値は生の文字列を表示する。 */
const PATTERN_RELATION_LABELS: Record<string, string> = {
  related: "関連",
  generalization: "上位（より一般的）",
  specialization: "下位（特殊化）",
  prerequisite: "前提",
  derived: "派生",
  alternative: "代替",
};

const EMPTY_FILTERS: PatternFilterValues = {
  pattern_types: [],
  tags: [],
  domains: [],
  goals: [],
  operations: [],
  structures: [],
  situations: [],
};

const FACET_LABELS: Array<[keyof PatternFacets, string]> = [
  ["domains", "分野"],
  ["goals", "目的"],
  ["operations", "操作"],
  ["structures", "構造"],
  ["situations", "状況分類"],
];

function valuesFromInput(value: string): string[] {
  return value
    .split(/[,、\n]/)
    .map((item) => item.trim())
    .filter(Boolean)
    .filter((item, index, all) => all.indexOf(item) === index);
}

function downloadJson(json: string, fileName: string) {
  const url = URL.createObjectURL(new Blob([json], { type: "application/json" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  URL.revokeObjectURL(url);
}

function resultMessage(result: ImportPatternsResult): string {
  const relations = result.relations_created + result.problem_relations_created;
  return `定石 ${result.created}件を追加、${result.skipped}件を既存として保持${relations ? `、関連 ${relations}件を追加` : ""}しました`;
}

function TextSection({ title, source }: { title: string; source: string }) {
  if (!source.trim()) return null;
  return (
    <section className="space-y-2">
      <h3 className="section-label">■ {title}</h3>
      <div className="rounded border px-4 py-3" style={{ borderColor: "var(--border)", background: "var(--panel-2)" }}>
        <LatexPreview source={source} />
      </div>
    </section>
  );
}

function FacetChips({ pattern }: { pattern: Pick<PatternFull, "facets"> }) {
  return (
    <div className="space-y-1.5">
      {FACET_LABELS.map(([key, label]) => pattern.facets[key].length > 0 && (
        <div key={key} className="flex flex-wrap items-center gap-1 text-xs">
          <span className="w-14 shrink-0" style={{ color: "var(--muted)" }}>{label}</span>
          {pattern.facets[key].map((value) => <span key={value} className="badge badge-muted">{value}</span>)}
        </div>
      ))}
    </div>
  );
}

function PatternSnapshotPreview({ snapshot }: { snapshot: PatternSnapshot }) {
  return (
    <div className="space-y-5">
      <div>
        <div className="mb-2 flex items-center gap-2"><PatternTypeBadge type={snapshot.pattern_type} /><h2 className="text-lg font-semibold">{snapshot.title}</h2></div>
        <p className="text-sm" style={{ color: "var(--muted)" }}>{snapshot.summary}</p>
      </div>

      {snapshot.strategies.length > 0 && (
        <section>
          <h3 className="section-label mb-2">■ 候補</h3>
          <div className="space-y-2">
            {snapshot.strategies.map((strategy, index) => (
              <div key={`${strategy.id ?? "new"}-${index}`} className="card px-4 py-3">
                <strong>{index + 1}. {strategy.title}</strong>
                {strategy.description && <LatexPreview source={strategy.description} />}
              </div>
            ))}
          </div>
        </section>
      )}
      <TextSection title="例" source={snapshot.examples} />
    </div>
  );
}

function PatternEditor({
  pattern,
  onSave,
  onCancel,
}: {
  pattern: PatternFull;
  onSave: (payload: PatternUpdate) => Promise<void>;
  onCancel: () => void;
}) {
  const { setDirty } = useApp();
  const [draft, setDraft] = useState<PatternUpdate>(() => ({
    id: pattern.id,
    expected_version: pattern.version,
    title: pattern.title,
    summary: pattern.summary,
    pattern_type: pattern.pattern_type,
    situation: pattern.situation,
    principle: pattern.principle,
    cautions: pattern.cautions,
    examples: pattern.examples,
    source_note: pattern.source_note,
    tags: pattern.tags,
    facets: pattern.facets,
    strategies: pattern.strategies.map((strategy) => ({ ...strategy })),
  }));
  const [saving, setSaving] = useState(false);

  useEffect(() => () => setDirty(false), []);

  const patch = (fields: Partial<PatternUpdate>) => {
    setDraft((current) => ({ ...current, ...fields }));
    setDirty(true);
  };
  const patchStrategy = (index: number, fields: Partial<PatternStrategyInput>) => {
    patch({
      strategies: draft.strategies.map((strategy, current) => current === index ? { ...strategy, ...fields } : strategy),
    });
  };
  const moveStrategy = (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= draft.strategies.length) return;
    const next = [...draft.strategies];
    [next[index], next[target]] = [next[target], next[index]];
    patch({ strategies: next.map((strategy, sort_order) => ({ ...strategy, sort_order })) });
  };

  return (
    <div className="h-full overflow-y-auto px-5 py-4">
      <div className="mx-auto max-w-5xl space-y-5">
        <div className="flex items-center gap-2 border-b pb-3" style={{ borderColor: "var(--border)" }}>
          <h2 className="mr-auto text-base font-semibold">定石を編集</h2>
          <button className="btn btn-ghost btn-sm" onClick={onCancel}>キャンセル</button>
          <button
            className="btn btn-solid btn-sm"
            disabled={saving || !draft.title.trim()}
            onClick={async () => {
              setSaving(true);
              try {
                await onSave({
                  ...draft,
                  strategies: draft.strategies.map((strategy, sort_order) => ({ ...strategy, sort_order })),
                });
                setDirty(false);
              } finally {
                setSaving(false);
              }
            }}
          >
            {saving ? "保存中..." : "保存"}
          </button>
        </div>

        <div className="grid gap-3 md:grid-cols-[1fr_12rem]">
          <label className="space-y-1 text-xs">タイトル<input className="input w-full text-sm" value={draft.title} onChange={(event) => patch({ title: event.target.value })} /></label>
          <label className="space-y-1 text-xs">種類
            <select className="select w-full" value={draft.pattern_type} onChange={(event) => patch({ pattern_type: event.target.value })}>
              {Object.entries(PATTERN_TYPE_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
              {!PATTERN_TYPE_LABELS[draft.pattern_type] && <option value={draft.pattern_type}>{draft.pattern_type}</option>}
            </select>
          </label>
        </div>

        <section>
          <div className="mb-2 flex items-center"><h3 className="section-label mr-auto">Candidate Strategies</h3><button className="btn btn-outline btn-sm" onClick={() => patch({ strategies: [...draft.strategies, { id: null, parent_strategy_id: null, title: "", description: "", condition: "", reasoning: "", branch_label: "", sort_order: draft.strategies.length }] })}>＋ 候補を追加</button></div>
          <div className="space-y-3">
            {draft.strategies.length === 0 && <p className="card px-4 py-6 text-center text-sm" style={{ color: "var(--muted)" }}>候補となる考え方を追加してください。</p>}
            {draft.strategies.map((strategy, index) => (
              <div key={`${strategy.id ?? "new"}-${index}`} className="card space-y-2 p-3">
                <div className="flex items-center gap-1">
                  <span className="badge">{index + 1}</span>
                  <input className="input min-w-0 flex-1 font-medium" value={strategy.title} onChange={(event) => patchStrategy(index, { title: event.target.value })} placeholder="候補名（例: 係数比較）" />
                  <button className="btn btn-ghost btn-sm" disabled={index === 0} onClick={() => moveStrategy(index, -1)}>↑</button>
                  <button className="btn btn-ghost btn-sm" disabled={index === draft.strategies.length - 1} onClick={() => moveStrategy(index, 1)}>↓</button>
                  <button className="btn btn-danger btn-sm" onClick={() => patch({ strategies: draft.strategies.filter((_, current) => current !== index) })}>削除</button>
                </div>
                <textarea className="textarea min-h-16 w-full text-sm" value={strategy.description} onChange={(event) => patchStrategy(index, { description: event.target.value })} placeholder="候補の説明" />
                <div className="grid gap-2 lg:grid-cols-2">
                </div>
              </div>
            ))}
          </div>
        </section>

        <label className="block space-y-1 text-xs">例<textarea className="textarea min-h-28 w-full font-mono text-sm" value={draft.examples} onChange={(event) => patch({ examples: event.target.value })} /></label>

        {/* 旧版で作られた定石だけが持つ項目。カードにも詳細表示にも出さないが、
            中身が残っている場合に確認・削除できるよう編集画面にだけ置く。 */}
        {(draft.summary.trim() || draft.situation.trim() || draft.principle.trim() || draft.cautions.trim()) && (
          <details className="card p-3">
            <summary className="cursor-pointer text-xs" style={{ color: "var(--muted)" }}>
              旧項目（カードには出ません）
            </summary>
            <div className="mt-2 space-y-2">
              <label className="block space-y-1 text-xs">概要<textarea className="textarea min-h-20 w-full text-sm" value={draft.summary} onChange={(event) => patch({ summary: event.target.value })} /></label>
              <label className="block space-y-1 text-xs">どんな状況で思い出すか<textarea className="textarea min-h-24 w-full font-mono text-sm" value={draft.situation} onChange={(event) => patch({ situation: event.target.value })} /></label>
              <label className="block space-y-1 text-xs">基本原則<textarea className="textarea min-h-24 w-full font-mono text-sm" value={draft.principle} onChange={(event) => patch({ principle: event.target.value })} /></label>
              <label className="block space-y-1 text-xs">注意事項<textarea className="textarea min-h-24 w-full font-mono text-sm" value={draft.cautions} onChange={(event) => patch({ cautions: event.target.value })} /></label>
            </div>
          </details>
        )}

        <section className="card space-y-3 p-4">
          <h3 className="section-label">分類metadata</h3>
          <label className="grid items-center gap-2 text-xs sm:grid-cols-[6rem_1fr]">タグ<input className="input" value={draft.tags.join("、")} onChange={(event) => patch({ tags: valuesFromInput(event.target.value) })} placeholder="カンマまたは読点区切り" /></label>
          {FACET_LABELS.map(([key, label]) => (
            <label key={key} className="grid items-center gap-2 text-xs sm:grid-cols-[6rem_1fr]">{label}<input className="input" value={draft.facets[key].join("、")} onChange={(event) => patch({ facets: { ...draft.facets, [key]: valuesFromInput(event.target.value) } })} placeholder="複数指定可" /></label>
          ))}
        </section>
        <label className="block space-y-1 text-xs">出典・由来メモ<textarea className="textarea min-h-20 w-full text-sm" value={draft.source_note} onChange={(event) => patch({ source_note: event.target.value })} placeholder="自分で作成、Problemから一般化、書籍・授業など" /></label>
      </div>
    </div>
  );
}

function PatternDetail({
  pattern,
  onEdit,
  onReload,
  onDuplicate,
  onDelete,
  onHistory,
  onExport,
}: {
  pattern: PatternFull;
  onEdit: () => void;
  onReload: () => Promise<void>;
  onDuplicate: () => Promise<void>;
  onDelete: () => Promise<void>;
  onHistory: () => Promise<void>;
  onExport: () => Promise<void>;
}) {
  const { openPattern, openProblemInBank, showToast } = useApp();
  const [patternPicker, setPatternPicker] = useState(false);
  const [problemPicker, setProblemPicker] = useState(false);
  const [problemRelation, setProblemRelation] = useState("applicable");
  const [patternRelation, setPatternRelation] = useState("related");
  const [aiEdit, setAiEdit] = useState(false);

  return (
    <div className="h-full overflow-y-auto px-5 py-4">
      <article className="mx-auto max-w-5xl space-y-6">
        <header className="border-b pb-4" style={{ borderColor: "var(--border)" }}>
          <div className="mb-2 flex flex-wrap items-center gap-2">
            <PatternTypeBadge type={pattern.pattern_type} />
            <h1 className="min-w-0 flex-1 text-xl font-semibold">{pattern.title}</h1>
            <button className="btn btn-outline btn-sm" onClick={onEdit}>編集</button>
            <button className="btn btn-outline btn-sm" onClick={() => setAiEdit(true)}>AIで編集</button>
            <button className="btn btn-ghost btn-sm" onClick={onHistory}>履歴</button>
            <button className="btn btn-ghost btn-sm" onClick={onDuplicate}>複製</button>
            <button className="btn btn-ghost btn-sm" onClick={onExport}>JSON</button>
            <button className="btn btn-danger btn-sm" onClick={onDelete}>削除</button>
          </div>
          {pattern.summary && <p className="text-sm leading-6" style={{ color: "var(--muted)" }}>{pattern.summary}</p>}
          <div className="mt-3 flex flex-wrap gap-1"><TagChips tags={pattern.tags} /></div>
          <div className="mt-3"><FacetChips pattern={pattern} /></div>
        </header>



        <section>
          <h3 className="section-label mb-2">■ 候補となる考え方</h3>
          {pattern.strategies.length === 0 ? <p className="text-sm" style={{ color: "var(--muted)" }}>候補はまだ登録されていません。</p> : (
            <div className="space-y-3">
              {pattern.strategies.map((strategy, index) => (
                <div key={strategy.id} className="card px-4 py-3">
                  <h4 className="font-semibold">{index + 1}. {strategy.title}</h4>
                  {strategy.description && <div className="mt-2"><LatexPreview source={strategy.description} /></div>}
                </div>
              ))}
            </div>
          )}
        </section>

        <TextSection title="例" source={pattern.examples} />

        <section className="card p-4">
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <h3 className="section-label mr-auto">関連定石</h3>
            <select className="select text-xs" value={patternRelation} onChange={(event) => setPatternRelation(event.target.value)}>
              {Object.entries(PATTERN_RELATION_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
            <button className="btn btn-outline btn-sm" onClick={() => setPatternPicker(true)}>＋ 関連付け</button>
          </div>
          {pattern.related_patterns.length === 0 ? <p className="text-sm" style={{ color: "var(--muted)" }}>関連定石はありません。</p> : (
            <ul className="space-y-1">
              {pattern.related_patterns.map((relation) => (
                <li key={`${relation.from_pattern_id}-${relation.to_pattern_id}-${relation.relation_type}`} className="flex items-center gap-2 rounded px-2 py-1.5" style={{ background: "var(--panel-2)" }}>
                  <PatternTypeBadge type={relation.pattern_type} />
                  <button className="min-w-0 flex-1 truncate text-left text-sm font-medium" onClick={() => openPattern(relation.pattern_id)}>{relation.title}</button>
                  <span className="text-[11px]" style={{ color: "var(--muted)" }}>{PATTERN_RELATION_LABELS[relation.relation_type] ?? relation.relation_type}</span>
                  <button className="btn btn-ghost btn-sm" onClick={async () => { try { await unlinkPatternRelation(relation.from_pattern_id, relation.to_pattern_id, relation.relation_type); await onReload(); } catch (error) { showToast(String(error), "error"); } }}>解除</button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="card p-4">
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <h3 className="section-label mr-auto">関連問題</h3>
            <select className="select text-xs" value={problemRelation} onChange={(event) => setProblemRelation(event.target.value)}><option value="applicable">候補</option><option value="used">使用</option></select>
            <button className="btn btn-outline btn-sm" onClick={() => setProblemPicker(true)}>＋ 問題を関連付け</button>
          </div>
          {pattern.related_problems.length === 0 ? <p className="text-sm" style={{ color: "var(--muted)" }}>関連問題はありません。</p> : (
            <ul className="space-y-1">
              {pattern.related_problems.map((problem) => (
                <li key={problem.problem_id} className="flex items-center gap-2 rounded px-2 py-1.5" style={{ background: "var(--panel-2)" }}>
                  <select className="select text-xs" value={problem.relation_type} onChange={async (event) => { try { await linkProblemPattern(problem.problem_id, pattern.id, event.target.value); await onReload(); } catch (error) { showToast(String(error), "error"); } }}><option value="applicable">候補</option><option value="used">使用</option></select>
                  <button className="min-w-0 flex-1 text-left" onClick={() => openProblemInBank(problem.bank_node_id, problem.problem_id)}><span className="block truncate text-sm font-medium">{problem.title}</span><span className="block truncate text-[11px]" style={{ color: "var(--muted)" }}>{problem.bank_path}</span></button>
                  <button className="btn btn-ghost btn-sm" onClick={async () => { try { await unlinkProblemPattern(problem.problem_id, pattern.id); await onReload(); } catch (error) { showToast(String(error), "error"); } }}>解除</button>
                </li>
              ))}
            </ul>
          )}
        </section>

        {pattern.source_note && <TextSection title="出典・由来" source={pattern.source_note} />}
        <footer className="border-t pt-3 text-[11px]" style={{ borderColor: "var(--border)", color: "var(--muted)" }}>v{pattern.version} / 更新: {pattern.updated_at} / 作成: {pattern.created_at}</footer>
      </article>

      {aiEdit && (
        <PatternAiEditDialog
          pattern={pattern}
          onClose={() => setAiEdit(false)}
          onApplied={onReload}
        />
      )}

      {patternPicker && <PatternPicker title="関連定石を追加" excludeId={pattern.id} existingIds={pattern.related_patterns.map((item) => item.pattern_id)} onClose={() => setPatternPicker(false)} onPick={async (target) => { try { await linkPatternRelation(pattern.id, target.id, patternRelation); await onReload(); } catch (error) { showToast(String(error), "error"); } }} />}
      {problemPicker && <ProblemPicker existingProblemIds={pattern.related_problems.map((item) => item.problem_id)} onClose={() => setProblemPicker(false)} onPick={async (problemId) => { try { await linkProblemPattern(problemId, pattern.id, problemRelation); await onReload(); } catch (error) { showToast(String(error), "error"); } }} />}
    </div>
  );
}

export function PatternLibraryView() {
  const {
    selectedPatternId,
    selectPattern,
    showToast,
    confirm,
    setContextName,
    setDirty,
    dirty,
    bumps,
  } = useApp();
  const [text, setText] = useState("");
  const [type, setType] = useState("");
  const [tag, setTag] = useState("");
  const [domain, setDomain] = useState("");
  const [goal, setGoal] = useState("");
  const [operation, setOperation] = useState("");
  const [structure, setStructure] = useState("");
  const [filters, setFilters] = useState<PatternFilterValues>(EMPTY_FILTERS);
  const [patterns, setPatterns] = useState<PatternSummary[]>([]);
  const [imageImport, setImageImport] = useState(false);
  const [pattern, setPattern] = useState<PatternFull | null>(null);
  const [editing, setEditing] = useState(false);
  const [loading, setLoading] = useState(false);
  const [versions, setVersions] = useState<PatternVersionSummary[] | null>(null);
  const [versionView, setVersionView] = useState<PatternVersionFull | null>(null);
  const webImportRef = useRef<HTMLInputElement>(null);
  const requestRef = useRef(0);

  const loadList = async () => {
    const request = ++requestRef.current;
    try {
      const result = await searchPatterns({
        text,
        pattern_type: type || null,
        tag: tag || null,
        domain: domain || null,
        goal: goal || null,
        operation: operation || null,
        structure: structure || null,
        limit: 100,
      });
      if (request === requestRef.current) setPatterns(result);
    } catch (error) {
      if (request === requestRef.current) showToast(String(error), "error");
    }
  };

  const loadPattern = async (id = selectedPatternId) => {
    if (id == null) { setPattern(null); return; }
    setLoading(true);
    try {
      setPattern(await getPattern(id));
    } catch (error) {
      setPattern(null);
      showToast(String(error), "error");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    setContextName("定石ライブラリ");
    listPatternFilterValues().then(setFilters).catch(() => {});
    return () => { setContextName(""); setDirty(false); };
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => void loadList(), 220);
    return () => clearTimeout(timer);
  }, [text, type, tag, domain, goal, operation, structure, bumps.patterns]);

  useEffect(() => { if (!editing) void loadPattern(); }, [selectedPatternId, bumps.patterns]);
  useEffect(() => { listPatternFilterValues().then(setFilters).catch(() => {}); }, [bumps.patterns]);

  const choosePattern = async (id: number) => {
    if (id === selectedPatternId) return;
    if (dirty && !(await confirm("未保存の定石編集があります。保存せずに別の定石を開きますか？"))) return;
    setDirty(false);
    setEditing(false);
    selectPattern(id);
  };

  const createNew = async () => {
    if (dirty && !(await confirm("未保存の定石編集があります。保存せずに新規作成しますか？"))) return;
    try {
      const id = await createPattern("新しい定石", "strategy");
      selectPattern(id);
      setPattern(await getPattern(id));
      setEditing(true);
      await loadList();
    } catch (error) { showToast(String(error), "error"); }
  };

  const save = async (payload: PatternUpdate) => {
    try {
      await updatePattern(payload);
    } catch (error) {
      if (error instanceof ConflictError) {
        if (!(await confirm("他の端末でこの定石が更新されています。自分の内容で上書きしますか？"))) {
          await loadPattern(payload.id);
          setEditing(false);
          setDirty(false);
          return;
        }
        await updatePattern({ ...payload, expected_version: null });
      } else {
        showToast(String(error), "error");
        throw error;
      }
    }
    await Promise.all([loadPattern(payload.id), loadList(), listPatternFilterValues().then(setFilters)]);
    setEditing(false);
    setDirty(false);
    showToast("定石を保存しました");
  };

  const duplicateCurrent = async () => {
    if (!pattern) return;
    try {
      const id = await duplicatePattern(pattern.id);
      selectPattern(id);
      setPattern(await getPattern(id));
      setEditing(true);
      await loadList();
      showToast("定石を複製しました");
    } catch (error) { showToast(String(error), "error"); }
  };

  const deleteCurrent = async () => {
    if (!pattern) return;
    try {
      const impact = await getPatternDeleteImpact(pattern.id);
      const warning = `定石「${pattern.title}」を削除しますか？\n関連問題 ${impact.problem_count}件、関連定石 ${impact.related_pattern_count}件との関連は解除されます。\nProblem本体は削除されません。`;
      if (!(await confirm(warning))) return;
      await deletePattern(pattern.id);
      selectPattern(null);
      setPattern(null);
      await loadList();
      showToast("定石を削除しました");
    } catch (error) { showToast(String(error), "error"); }
  };

  const openHistory = async () => {
    if (!pattern) return;
    try { setVersions(await listPatternVersions(pattern.id)); }
    catch (error) { showToast(String(error), "error"); }
  };

  const exportJson = async (selectedOnly: boolean) => {
    try {
      const ids = selectedOnly && pattern ? [pattern.id] : null;
      if (isTauri) {
        const dest = await saveDialog({ defaultPath: selectedOnly && pattern ? `${pattern.title}.patterns.json` : "pattern-library.json", filters: [{ name: "定石ライブラリJSON", extensions: ["json"] }] });
        if (!dest) return;
        await exportPatternsFile(ids, dest);
      } else {
        downloadJson(await exportPatternsJson(ids), selectedOnly && pattern ? `${pattern.title}.patterns.json` : "pattern-library.json");
      }
      showToast("定石をJSONへ書き出しました");
    } catch (error) { showToast(String(error), "error"); }
  };

  const importJson = async () => {
    try {
      if (isTauri) {
        const file = await openDialog({ multiple: false, filters: [{ name: "定石ライブラリJSON", extensions: ["json"] }] });
        if (!file) return;
        const result = await importPatternsFile(file as string);
        showToast(resultMessage(result));
        await loadList();
      } else {
        webImportRef.current?.click();
      }
    } catch (error) { showToast(String(error), "error"); }
  };

  return (
    <div className="flex h-full min-w-0">
      <aside className="flex w-[340px] shrink-0 flex-col border-r" style={{ borderColor: "var(--border)", background: "var(--panel)" }}>
        <div className="space-y-2 border-b p-3" style={{ borderColor: "var(--border)" }}>
          <div className="space-y-1">
            <div className="flex gap-1"><button className="btn btn-solid btn-sm flex-1" onClick={createNew}>＋ 新規定石</button><button className="btn btn-ghost btn-sm" onClick={() => void importJson()}>取込</button><button className="btn btn-ghost btn-sm" onClick={() => void exportJson(false)}>全出力</button></div>
            <button className="btn btn-outline btn-sm w-full" onClick={() => setImageImport(true)}>画像・写真から取り込む</button>
          </div>
          <input className="input w-full" value={text} onChange={(event) => setText(event.target.value)} placeholder="定石・状況・候補・タグを検索" />
          <div className="grid grid-cols-2 gap-1">
            <select className="select min-w-0 text-xs" value={type} onChange={(event) => setType(event.target.value)}><option value="">種類: すべて</option>{[...new Set([...Object.keys(PATTERN_TYPE_LABELS), ...filters.pattern_types])].map((value) => <option key={value} value={value}>{patternTypeLabel(value)}</option>)}</select>
            <select className="select min-w-0 text-xs" value={tag} onChange={(event) => setTag(event.target.value)}><option value="">タグ: すべて</option>{filters.tags.map((value) => <option key={value}>{value}</option>)}</select>
            <select className="select min-w-0 text-xs" value={domain} onChange={(event) => setDomain(event.target.value)}><option value="">分野: すべて</option>{filters.domains.map((value) => <option key={value}>{value}</option>)}</select>
            <select className="select min-w-0 text-xs" value={goal} onChange={(event) => setGoal(event.target.value)}><option value="">目的: すべて</option>{filters.goals.map((value) => <option key={value}>{value}</option>)}</select>
            <select className="select min-w-0 text-xs" value={operation} onChange={(event) => setOperation(event.target.value)}><option value="">操作: すべて</option>{filters.operations.map((value) => <option key={value}>{value}</option>)}</select>
            <select className="select min-w-0 text-xs" value={structure} onChange={(event) => setStructure(event.target.value)}><option value="">構造: すべて</option>{filters.structures.map((value) => <option key={value}>{value}</option>)}</select>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {patterns.length === 0 ? <p className="p-4 text-center text-sm" style={{ color: "var(--muted)" }}>該当する定石がありません。</p> : patterns.map((item) => (
            <button key={item.id} className={`mb-1 w-full rounded border px-3 py-2 text-left transition-colors ${selectedPatternId === item.id ? "border-[var(--accent)] bg-[var(--accent-dim)]" : "border-transparent hover:bg-[var(--panel-3)]"}`} onClick={() => void choosePattern(item.id)}>
              <span className="flex items-start gap-2"><PatternTypeBadge type={item.pattern_type} /><span className="min-w-0 flex-1 font-medium">{item.title}</span></span>
              {item.summary && <span className="mt-1 line-clamp-2 block text-xs" style={{ color: "var(--muted)" }}>{item.summary}</span>}
              <span className="mt-1.5 flex flex-wrap items-center gap-1"><TagChips tags={item.tags} /><span className="ml-auto text-[10px]" style={{ color: "var(--muted)" }}>{item.strategy_count}候補 / {item.problem_count}問</span></span>
            </button>
          ))}
        </div>
        <div className="border-t px-3 py-1.5 text-[10px]" style={{ borderColor: "var(--border)", color: "var(--muted)" }}>最大100件をbackend検索</div>
      </aside>

      <main className="min-w-0 flex-1">
        {loading ? <div className="flex h-full items-center justify-center text-sm" style={{ color: "var(--muted)" }}>読み込み中...</div> : pattern ? (
          editing ? <PatternEditor key={`${pattern.id}-${pattern.version}`} pattern={pattern} onSave={save} onCancel={async () => { if (!dirty || await confirm("編集内容を破棄しますか？")) { setDirty(false); setEditing(false); await loadPattern(pattern.id); } }} /> :
          <PatternDetail pattern={pattern} onEdit={() => setEditing(true)} onReload={() => loadPattern(pattern.id)} onDuplicate={duplicateCurrent} onDelete={deleteCurrent} onHistory={openHistory} onExport={() => exportJson(true)} />
        ) : <div className="flex h-full flex-col items-center justify-center gap-2 text-sm" style={{ color: "var(--muted)" }}><span className="text-3xl">◇</span><p>左の一覧から定石を選択するか、新規作成してください。</p></div>}
      </main>

      <input ref={webImportRef} type="file" accept="application/json,.json" className="hidden" onChange={async (event) => { const file = event.target.files?.[0]; event.target.value = ""; if (!file) return; try { const result = await importPatternsJson(await file.text()); showToast(resultMessage(result)); await loadList(); } catch (error) { showToast(String(error), "error"); } }} />

      {imageImport && (
        <PatternImageImportDialog
          onClose={() => setImageImport(false)}
          onApplied={() => void loadList()}
        />
      )}

      {versions && (
        <Modal title="定石の変更履歴" onClose={() => { setVersions(null); setVersionView(null); }} wide={versionView != null}>
          {versionView ? (
            <div className="grid max-h-[70vh] gap-4 overflow-y-auto lg:grid-cols-[14rem_1fr]">
              <div><button className="btn btn-ghost btn-sm mb-2" onClick={() => setVersionView(null)}>← 一覧へ</button><p className="text-xs" style={{ color: "var(--muted)" }}>v{versionView.version}<br />{versionView.saved_at}</p><button className="btn btn-solid btn-sm mt-4" onClick={async () => { if (!pattern || !(await confirm(`v${versionView.version}へ復元しますか？\n現在の内容も履歴として保存されます。`))) return; try { await restorePatternVersion(versionView.id, pattern.version); setVersions(null); setVersionView(null); await loadPattern(pattern.id); await loadList(); showToast("定石を履歴から復元しました"); } catch (error) { showToast(String(error), "error"); } }}>この版へ復元</button></div>
              <PatternSnapshotPreview snapshot={versionView.snapshot} />
            </div>
          ) : versions.length === 0 ? <p className="py-6 text-center text-sm" style={{ color: "var(--muted)" }}>履歴はまだありません。</p> : (
            <ul className="max-h-[60vh] space-y-1 overflow-y-auto">{versions.map((version) => <li key={version.id}><button className="card card-glow flex w-full items-center gap-3 px-3 py-2 text-left" onClick={async () => { try { setVersionView(await getPatternVersion(version.id)); } catch (error) { showToast(String(error), "error"); } }}><strong className="badge">v{version.version}</strong><span className="min-w-0 flex-1 truncate">{version.title}</span><span className="text-xs" style={{ color: "var(--muted)" }}>{version.saved_at}</span></button></li>)}</ul>
          )}
        </Modal>
      )}
    </div>
  );
}
