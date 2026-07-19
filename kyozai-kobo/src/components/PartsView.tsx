import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  addPartAttachment,
  addPartToProject,
  createPart,
  deletePart,
  duplicatePart,
  getPart,
  listAllPartTags,
  listPartCategories,
  listPartVersions,
  listProjects,
  removePartAttachment,
  searchParts,
  updatePart,
  uploadPartAttachment,
} from "../api";
import { useApp } from "../store";
import { ConflictError, isTauri } from "../transport";
import type {
  DifficultyRank,
  PartFull,
  PartOutputTarget,
  PartSummary,
  PartType,
  PartVersionSummary,
  ProjectSummary,
  RequiredFilter,
} from "../types";
import { AiConvertDialog } from "./AiConvertDialog";
import { ConflictDialog } from "./ConflictDialog";
import { LatexEditor } from "./LatexEditor";
import { LatexPreview } from "./LatexPreview";
import { Icon } from "./Icon";
import { DIFFICULTY_RANKS, DifficultyRankBadge, Modal, TagChips } from "./ui";

const PART_TYPES: { value: PartType; label: string }[] = [
  { value: "heading", label: "見出し" },
  { value: "text", label: "本文" },
  { value: "notice", label: "注意" },
  { value: "hint", label: "ヒント" },
  { value: "example", label: "例題" },
  { value: "homework", label: "宿題" },
  { value: "reflection", label: "振り返り" },
  { value: "box", label: "枠" },
  { value: "table", label: "表" },
  { value: "image_block", label: "画像" },
  { value: "latex_snippet", label: "LaTeX断片" },
  { value: "page_break", label: "改ページ" },
  { value: "custom", label: "カスタム" },
];

const OUTPUT_TARGETS: { value: PartOutputTarget; label: string }[] = [
  { value: "problems", label: "問題冊子に表示" },
  { value: "answers", label: "解答冊子に表示" },
  { value: "both", label: "両方に表示" },
  { value: "none", label: "出力しない" },
];

const partDraftKey = (id: number) => `kk-draft-part-${id}`;

function clearPartDraft(id: number) {
  try {
    localStorage.removeItem(partDraftKey(id));
  } catch {
    // 保存本体の成否をlocalStorageの利用可否で変えない。
  }
}

export function PartsView() {
  const { showToast, confirm, setContextName, setDirty: setGlobalDirty, bumps } = useApp();
  const [text, setText] = useState("");
  const [partType, setPartType] = useState("");
  const [category, setCategory] = useState("");
  const [tag, setTag] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [categories, setCategories] = useState<string[]>([]);
  const [rankFilters, setRankFilters] = useState<(DifficultyRank | "__unset")[]>([]);
  const [requiredFilter, setRequiredFilter] = useState<RequiredFilter>("all");
  const [parts, setParts] = useState<PartSummary[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [part, setPart] = useState<PartFull | null>(null);
  const [tagInput, setTagInput] = useState("");
  const [dirty, setDirty] = useState(false);
  const [versions, setVersions] = useState<PartVersionSummary[] | null>(null);
  const [addTarget, setAddTarget] = useState<PartSummary | PartFull | null>(null);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [aiDialogMode, setAiDialogMode] = useState<"convert" | "topic_guide" | null>(null);
  const [conflict, setConflict] = useState<PartFull | null>(null);
  const webFileInputRef = useRef<HTMLInputElement>(null);
  const latestPart = useRef<PartFull | null>(null);
  latestPart.current = part;
  const seenPartsBumpRef = useRef(bumps.parts);
  const pendingPartsRefreshRef = useRef(false);
  const searchRequestRef = useRef(0);
  const partLoadRequestRef = useRef(0);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  const loadFilters = () => {
    listAllPartTags().then(setTags).catch(() => {});
    listPartCategories().then(setCategories).catch(() => {});
  };

  const runSearch = async () => {
    const requestId = ++searchRequestRef.current;
    try {
      const result = await searchParts({
        text,
        part_type: partType || null,
        category: category || null,
        tag: tag || null,
        difficulty_ranks: rankFilters.length ? rankFilters : null,
        required_filter: requiredFilter === "all" ? null : requiredFilter,
      });
      if (requestId !== searchRequestRef.current) return;
      setParts(result);
      if (selectedId == null && result[0]) setSelectedId(result[0].id);
    } catch (e) {
      if (requestId === searchRequestRef.current) showToast(String(e), "error");
    }
  };

  const loadPart = async (id: number, preserveDirty = false) => {
    const requestId = ++partLoadRequestRef.current;
    try {
      const next = await getPart(id);
      if (requestId !== partLoadRequestRef.current) return;
      if (preserveDirty && dirtyRef.current) {
        pendingPartsRefreshRef.current = true;
        return;
      }
      setPart(next);
      setDirty(false);
      try {
        const raw = localStorage.getItem(partDraftKey(next.id));
        if (raw) {
          const draft = JSON.parse(raw) as { savedAt: number; part: PartFull };
          const restore =
            !!draft.part &&
            (await confirm(
              "この端末に未送信の部品編集が残っています。\n復元しますか？\n「キャンセル」で破棄します。",
            ));
          if (requestId !== partLoadRequestRef.current) return;
          if (restore) {
            const version = Number.isFinite(draft.part.version) ? draft.part.version : -1;
            setPart({ ...draft.part, version });
            setDirty(true);
          } else {
            clearPartDraft(next.id);
          }
        }
      } catch {
        clearPartDraft(next.id);
      }
    } catch (e) {
      if (requestId === partLoadRequestRef.current) showToast(String(e), "error");
    }
  };

  useEffect(() => {
    setContextName("部品ライブラリ");
    loadFilters();
    return () => {
      setContextName("");
      setGlobalDirty(false);
    };
  }, []);

  useEffect(() => {
    setGlobalDirty(dirty);
  }, [dirty]);

  useEffect(() => {
    const t = setTimeout(runSearch, 250);
    return () => clearTimeout(t);
  }, [text, partType, category, tag, rankFilters, requiredFilter]);

  useEffect(() => {
    if (selectedId != null) loadPart(selectedId);
  }, [selectedId]);

  const refreshFromRemote = () => {
    loadFilters();
    void runSearch();
    if (selectedId != null) void loadPart(selectedId, true);
  };

  // リモート更新は編集中の部品を潰さず、dirty解消後に一覧と選択中データへ反映する。
  useEffect(() => {
    if (seenPartsBumpRef.current === bumps.parts) return;
    seenPartsBumpRef.current = bumps.parts;
    if (dirty) {
      pendingPartsRefreshRef.current = true;
      return;
    }
    refreshFromRemote();
  }, [bumps.parts]);

  useEffect(() => {
    if (dirty || !pendingPartsRefreshRef.current) return;
    pendingPartsRefreshRef.current = false;
    refreshFromRemote();
  }, [dirty]);

  const patch = (fields: Partial<PartFull>) => {
    setPart((p) => (p ? { ...p, ...fields } : p));
    setDirty(true);
  };

  const toggleRankFilter = (rank: DifficultyRank | "__unset") => {
    setRankFilters((current) =>
      current.includes(rank) ? current.filter((r) => r !== rank) : [...current, rank],
    );
  };

  const onCreate = async () => {
    if (dirty && !(await confirm("未保存の変更があります。保存せずに新しい部品を作成しますか？"))) return;
    try {
      const id = await createPart("新しい部品");
      setSelectedId(id);
      await runSearch();
      loadFilters();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const selectPart = async (id: number) => {
    if (id === selectedId) return;
    if (dirty && !(await confirm("未保存の変更があります。保存せずに別の部品を開きますか？"))) return;
    if (dirty && selectedId != null) clearPartDraft(selectedId);
    setSelectedId(id);
  };

  const onSave = async (forceOverwrite = false) => {
    const p = latestPart.current;
    if (!p) return;
    try {
      await updatePart({
        id: p.id,
        title: p.title,
        part_type: p.part_type,
        category: p.category,
        tags: p.tags,
        latex_source: p.latex_source,
        description: p.description,
        difficulty_rank: p.difficulty_rank,
        is_required: p.is_required,
        output_target: p.output_target,
        expected_version: forceOverwrite ? null : p.version,
      });
      setDirty(false);
      clearPartDraft(p.id);
      await loadPart(p.id);
      await runSearch();
      loadFilters();
      showToast("部品を保存しました");
    } catch (e) {
      if (e instanceof ConflictError) {
        try {
          setConflict(await getPart(p.id));
        } catch {
          showToast("競合を検出しましたが、サーバー版を取得できませんでした", "error");
        }
        return;
      }
      try {
        localStorage.setItem(partDraftKey(p.id), JSON.stringify({ savedAt: Date.now(), part: p }));
      } catch {
        /* localStorage不可なら未保存表示を維持する */
      }
      showToast(`${String(e)}\n（編集内容はこの端末に一時保存されています）`, "error");
    }
  };

  const resolveConflict = async (choice: "server" | "mine" | "copy") => {
    const server = conflict;
    const mine = latestPart.current;
    setConflict(null);
    if (!server || !mine) return;
    if (choice === "server") {
      setPart(server);
      setDirty(false);
      clearPartDraft(mine.id);
      showToast("サーバー版を読み込みました");
    } else if (choice === "mine") {
      await onSave(true);
    } else {
      try {
        const newId = await createPart(`${mine.title} (競合コピー)`);
        await updatePart({
          id: newId,
          title: `${mine.title} (競合コピー)`,
          part_type: mine.part_type,
          category: mine.category,
          tags: mine.tags,
          latex_source: mine.latex_source,
          description: mine.description,
          difficulty_rank: mine.difficulty_rank,
          is_required: mine.is_required,
          output_target: mine.output_target,
        });
        setPart(server);
        setDirty(false);
        clearPartDraft(mine.id);
        await runSearch();
        showToast("自分の変更を「(競合コピー)」として保存しました");
      } catch (e) {
        showToast(String(e), "error");
      }
    }
  };

  const onDuplicate = async () => {
    if (!part) return;
    try {
      const id = await duplicatePart(part.id);
      setSelectedId(id);
      await runSearch();
      showToast("部品を複製しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDelete = async () => {
    if (!part) return;
    if (!(await confirm(`部品「${part.title}」を削除しますか？\n教材に挿入済みのスナップショットは維持されます。`))) {
      return;
    }
    try {
      await deletePart(part.id);
      clearPartDraft(part.id);
      setPart(null);
      setSelectedId(null);
      await runSearch();
      showToast("部品を削除しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const addTag = () => {
    const t = tagInput.trim();
    if (!part || !t) return;
    if (!part.tags.includes(t)) patch({ tags: [...part.tags, t] });
    setTagInput("");
  };

  const onAddAttachment = async () => {
    if (!part) return;
    try {
      if (!isTauri) {
        webFileInputRef.current?.click();
        return;
      }
      const file = await openDialog({
        multiple: false,
        filters: [{ name: "部品アセット", extensions: ["png", "jpg", "jpeg", "pdf", "svg", "tex", "sty"] }],
      });
      if (!file) return;
      await addPartAttachment(part.id, file as string);
      await loadPart(part.id);
      showToast("添付ファイルを追加しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onWebFilePicked = async (files: FileList | null) => {
    const p = latestPart.current;
    if (!files || files.length === 0 || !p) return;
    try {
      for (const f of Array.from(files)) {
        await uploadPartAttachment(p.id, f);
      }
      await loadPart(p.id);
      showToast("添付ファイルを追加しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRemoveAttachment = async (id: number) => {
    if (!(await confirm("この添付ファイルを部品から外しますか？"))) return;
    try {
      await removePartAttachment(id);
      if (part) await loadPart(part.id);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const openAddToProject = async (target: PartSummary | PartFull) => {
    try {
      setProjects(await listProjects());
      setAddTarget(target);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const addToProject = async (projectId: number) => {
    if (!addTarget) return;
    try {
      await addPartToProject(projectId, addTarget.id);
      setAddTarget(null);
      showToast("教材へ部品を追加しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const openVersions = async () => {
    if (!part) return;
    try {
      setVersions(await listPartVersions(part.id));
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  return (
    <div className="parts-split flex h-full min-w-0">
      <aside className="parts-list-pane flex w-[380px] shrink-0 flex-col border-r" style={{ borderColor: "var(--border)" }}>
        <div className="space-y-2 border-b p-3" style={{ borderColor: "var(--border)" }}>
          <div className="flex items-center gap-2">
            <input
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="input min-w-0 flex-1"
              placeholder="部品検索"
            />
            <button onClick={onCreate} className="btn btn-solid btn-sm">
              ＋ 新規
            </button>
            <button
              onClick={() => setAiDialogMode("topic_guide")}
              className="btn btn-outline btn-sm"
              title="高校数学の分野・解法を詳しく説明する新しい部品をAIで生成"
              style={{ borderColor: "rgba(157,108,242,0.52)", color: "var(--purple)", background: "var(--purple-dim)" }}
            >
              <Icon name="sparkle" size={14} /> 解説生成
            </button>
          </div>
          <div className="flex flex-wrap gap-1.5">
            <select value={partType} onChange={(e) => setPartType(e.target.value)} className="select text-xs">
              <option value="">種類: すべて</option>
              {PART_TYPES.map((t) => (
                <option key={t.value} value={t.value}>
                  {t.label}
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
          <div className="flex flex-wrap gap-1">
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
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {parts.length === 0 ? (
            <p className="p-4 text-sm" style={{ color: "var(--muted)" }}>
              該当する部品がありません。
            </p>
          ) : (
            <ul className="space-y-1.5">
              {parts.map((p) => (
                <li key={p.id}>
                  <button
                    onClick={() => void selectPart(p.id)}
                    className="card w-full px-3 py-2 text-left text-sm"
                    style={selectedId === p.id ? { borderColor: "var(--accent)", background: "var(--accent-dim)" } : undefined}
                  >
                    <div className="flex items-center gap-2">
                      <span className="min-w-0 flex-1 truncate font-semibold">{p.title}</span>
                      <DifficultyRankBadge rank={p.difficulty_rank} required={p.is_required} muted />
                    </div>
                    <div className="mt-1 flex items-center gap-2 text-[11px]" style={{ color: "var(--muted)" }}>
                      <span>{PART_TYPES.find((t) => t.value === p.part_type)?.label ?? p.part_type}</span>
                      <span>{p.category || "カテゴリなし"}</span>
                      <span>使用 {p.usage_count}</span>
                      <span>v{p.version}</span>
                    </div>
                    <div className="mt-1 truncate text-xs" style={{ color: "var(--muted)" }}>
                      {p.plain_text_preview || "プレビューなし"}
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </aside>

      <main className="min-w-0 flex-1">
        {!part ? (
          <div className="flex h-full items-center justify-center text-sm" style={{ color: "var(--muted)" }}>
            部品を選択してください。
          </div>
        ) : (
          <div className="editor-split flex h-full min-w-0">
            <section className="flex min-w-0 flex-1 flex-col border-r" style={{ borderColor: "var(--border)" }}>
              <div className="flex items-center gap-2 border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
                <input
                  value={part.title}
                  onChange={(e) => patch({ title: e.target.value })}
                  className="input min-w-0 flex-1 font-semibold"
                  placeholder="部品タイトル"
                />
                <DifficultyRankBadge rank={part.difficulty_rank} required={part.is_required} />
                {dirty && <span className="badge badge-warn">● 未保存</span>}
                <button
                  onClick={() => setAiDialogMode("convert")}
                  className="btn btn-outline btn-sm"
                  title="写真やテキストをAIでLaTeXへ変換して挿入"
                  style={{ borderColor: "rgba(157,108,242,0.52)", color: "var(--purple)", background: "var(--purple-dim)" }}
                >
                  <Icon name="sparkle" size={15} /> AI変換
                </button>
                <button onClick={() => onSave()} className="btn btn-solid">
                  保存
                </button>
              </div>
              <div className="space-y-2 border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
                <div className="grid grid-cols-3 gap-2">
                  <select
                    value={part.part_type}
                    onChange={(e) => patch({ part_type: e.target.value })}
                    className="select"
                  >
                    {PART_TYPES.map((t) => (
                      <option key={t.value} value={t.value}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                  <input
                    value={part.category}
                    onChange={(e) => patch({ category: e.target.value })}
                    className="input"
                    placeholder="カテゴリ"
                  />
                  <select
                    value={part.output_target}
                    onChange={(e) => patch({ output_target: e.target.value as PartOutputTarget })}
                    className="select"
                  >
                    {OUTPUT_TARGETS.map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="flex flex-wrap items-center gap-1.5">
                  {DIFFICULTY_RANKS.map((r) => (
                    <button
                      key={r.rank}
                      onClick={() => patch({ difficulty_rank: r.rank })}
                      className={`btn btn-sm ${part.difficulty_rank === r.rank ? "btn-outline" : "btn-ghost"}`}
                      title={`${r.rank}: ${r.description}`}
                    >
                      {r.rank} {r.label}
                    </button>
                  ))}
                  <button onClick={() => patch({ difficulty_rank: null })} className="btn btn-ghost btn-sm">
                    未設定
                  </button>
                  <label className="ml-1 flex items-center gap-1 text-xs" style={{ color: "var(--muted)" }}>
                    <input
                      type="checkbox"
                      checked={part.is_required}
                      onChange={(e) => patch({ is_required: e.target.checked })}
                    />
                    ★ 最低限
                  </label>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <span className="section-label">タグ</span>
                  <TagChips tags={part.tags} />
                  <input
                    value={tagInput}
                    onChange={(e) => setTagInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        addTag();
                      }
                    }}
                    className="input w-28 px-1.5 py-0.5 text-xs"
                    placeholder="タグ追加"
                  />
                  {part.tags.map((t) => (
                    <button
                      key={`remove-${t}`}
                      onClick={() => patch({ tags: part.tags.filter((x) => x !== t) })}
                      className="btn btn-ghost btn-sm"
                    >
                      {t} ✕
                    </button>
                  ))}
                </div>
                <textarea
                  value={part.description}
                  onChange={(e) => patch({ description: e.target.value })}
                  className="input-area h-14 w-full resize-none"
                  placeholder="説明・メモ"
                />
              </div>
              <div className="min-h-0 flex-1 p-2">
                <LatexEditor
                  key={part.id}
                  value={part.latex_source}
                  onChange={(v) => patch({ latex_source: v })}
                  className="h-full"
                  placeholder="部品のLaTeXソース"
                />
              </div>
              <div className="border-t px-3 py-2" style={{ borderColor: "var(--border)" }}>
                <div className="mb-1 flex items-center gap-2">
                  <span className="section-label">添付ファイル</span>
                  <button onClick={onAddAttachment} className="btn btn-ghost btn-sm">
                    ＋追加
                  </button>
                  <input
                    ref={webFileInputRef}
                    type="file"
                    accept="image/png,image/jpeg,image/webp,application/pdf"
                    multiple
                    className="hidden"
                    onChange={(e) => {
                      onWebFilePicked(e.target.files);
                      e.target.value = "";
                    }}
                  />
                  <button onClick={openVersions} className="btn btn-ghost btn-sm">
                    履歴
                  </button>
                  <button onClick={() => openAddToProject(part)} className="btn btn-outline btn-sm">
                    教材へ追加
                  </button>
                  <button onClick={onDuplicate} className="btn btn-ghost btn-sm ml-auto">
                    複製
                  </button>
                  <button onClick={onDelete} className="btn btn-danger btn-sm">
                    削除
                  </button>
                </div>
                {part.attachments.length > 0 && (
                  <ul className="space-y-1">
                    {part.attachments.map((a) => (
                      <li key={a.id} className="flex items-center gap-2 text-xs" style={{ color: "var(--muted)" }}>
                        <span className="truncate">{a.file_name}</span>
                        <code className="rounded px-1" style={{ background: "var(--panel-3)", color: "var(--accent)" }}>
                          {a.stored_name}
                        </code>
                        <button onClick={() => onRemoveAttachment(a.id)} className="btn btn-danger btn-sm">
                          ✕
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </section>
            <aside className="editor-preview-pane flex w-[36%] min-w-[280px] flex-col" style={{ background: "var(--panel)" }}>
              <div className="border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
                <span className="section-label">LaTeXプレビュー</span>
              </div>
              <div className="min-h-0 flex-1 overflow-auto p-3">
                <div className="paper">
                  <LatexPreview source={part.latex_source || " "} />
                </div>
              </div>
              <div className="border-t px-3 py-1.5 text-[11px]" style={{ borderColor: "var(--border)", color: "var(--muted)" }}>
                更新: {part.updated_at} / 作成: {part.created_at}
              </div>
            </aside>
          </div>
        )}
      </main>

      {versions && (
        <Modal title="部品のバージョン履歴" onClose={() => setVersions(null)}>
          {versions.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--muted)" }}>
              履歴はまだありません。
            </p>
          ) : (
            <ul className="max-h-[60vh] space-y-1 overflow-y-auto">
              {versions.map((v) => (
                <li key={v.id} className="card px-3 py-2 text-sm">
                  <div className="font-semibold">v{v.version} / {v.saved_at}</div>
                  <div className="text-xs" style={{ color: "var(--muted)" }}>
                    {v.title}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </Modal>
      )}

      {aiDialogMode && (
        <AiConvertDialog
          onClose={() => setAiDialogMode(null)}
          preset={
            aiDialogMode === "topic_guide"
              ? {
                  sourceType: "text",
                  mode: "generate_topic_guide",
                  title: "分野・解法の解説部品を生成",
                  solutionLayout: "single_column",
                  solutionDetail: "standard",
                }
              : undefined
          }
          insertTargets={
            aiDialogMode === "convert" && part
              ? [
                  {
                    label: "部品本文",
                    field: "latex_source",
                    entityType: "part",
                    entityId: part.id,
                    insert: (latexText: string) => {
                      const latest = latestPart.current;
                      if (!latest) return;
                      const base = latest.latex_source;
                      patch({ latex_source: base ? `${base}\n${latexText}` : latexText });
                    },
                  },
                ]
              : undefined
          }
        />
      )}

      {conflict && part && (
        <ConflictDialog
          title={`部品「${part.title}」`}
          fields={[
            { label: "LaTeXソース", mine: part.latex_source, server: conflict.latex_source },
            { label: "説明", mine: part.description, server: conflict.description },
            { label: "タイトル", mine: part.title, server: conflict.title },
          ]}
          onResolve={resolveConflict}
          onClose={() => setConflict(null)}
        />
      )}

      {addTarget && (
        <Modal title={`「${addTarget.title}」を教材へ追加`} onClose={() => setAddTarget(null)}>
          {projects.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--muted)" }}>
              教材プロジェクトがありません。
            </p>
          ) : (
            <ul className="space-y-1">
              {projects.map((p) => (
                <li key={p.id}>
                  <button onClick={() => addToProject(p.id)} className="card card-glow w-full px-3 py-2 text-left text-sm">
                    <span className="font-medium">{p.name}</span>
                    <span className="ml-2 text-xs" style={{ color: "var(--muted)" }}>
                      {p.item_count}項目 / {p.updated_at}
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
