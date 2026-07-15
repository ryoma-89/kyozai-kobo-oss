import { useEffect, useRef, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  addTemplateAsset,
  analyzeTexFile,
  createTemplate,
  deleteTemplate,
  duplicateTemplate,
  exportTemplate,
  getTemplate,
  importTemplateFile,
  importTemplateFromTex,
  listTemplates,
  listTemplateVersions,
  removeTemplateAsset,
  restoreTemplateVersion,
  testCompileTemplate,
  updateTemplate,
} from "../api";
import { useApp } from "../store";
import { ConflictError, isTauri, openCompiledFile } from "../transport";
import type { CompileResult, ImportAnalysis, TemplateFull, TemplateSummary, TemplateVersionSummary } from "../types";
import { AiConvertDialog } from "./AiConvertDialog";
import { LatexEditor, type LatexEditorHandle } from "./LatexEditor";
import { Modal } from "./ui";
import { Icon } from "./Icon";

type TplTab = "problem" | "answer" | "base";

const templateDraftKey = (id: number) => `kk-draft-template-${id}`;

function clearTemplateDraft(id: number) {
  try {
    localStorage.removeItem(templateDraftKey(id));
  } catch {
    // 保存本体の成否をlocalStorageの利用可否で変えない。
  }
}

const TAB_LABELS: Record<TplTab, string> = {
  problem: "問題冊子用",
  answer: "解答冊子用",
  base: "共通（本文用）",
};

const PLACEHOLDERS = [
  "{{TITLE}}",
  "{{SUBTITLE}}",
  "{{TARGET}}",
  "{{DATE}}",
  "{{NAME_FIELD}}",
  "{{HEADER_LEFT}}",
  "{{HEADER_RIGHT}}",
  "{{BODY}}",
  "{{ANSWER_BODY}}",
  "{{EXPLANATION_BODY}}",
];

/** LaTeXテンプレート管理画面 */
export function TemplatesView() {
  const { showToast, confirm, setContextName, setDirty, setLastCompile, setLogOpen, bumps } = useApp();
  const [templates, setTemplates] = useState<TemplateSummary[]>([]);
  const [tpl, setTpl] = useState<TemplateFull | null>(null);
  const [tab, setTab] = useState<TplTab>("problem");
  const [localDirty, setLocalDirty] = useState(false);
  const [compiling, setCompiling] = useState<string | null>(null);
  const [compileResult, setCompileResult] = useState<CompileResult | null>(null);
  const [versions, setVersions] = useState<TemplateVersionSummary[] | null>(null);
  const [showAi, setShowAi] = useState(false);
  const [importWizard, setImportWizard] = useState<{ path: string; analysis: ImportAnalysis } | null>(null);
  const textareaRef = useRef<LatexEditorHandle>(null);
  const tplRef = useRef<TemplateFull | null>(null);
  tplRef.current = tpl;
  const localDirtyRef = useRef(false);
  localDirtyRef.current = localDirty;
  const seenTemplatesBumpRef = useRef(bumps.templates);
  const pendingTemplatesRefreshRef = useRef(false);
  const templateListRequestRef = useRef(0);
  const templateLoadRequestRef = useRef(0);

  const loadList = async () => {
    const requestId = ++templateListRequestRef.current;
    try {
      const next = await listTemplates();
      if (requestId !== templateListRequestRef.current) return;
      setTemplates(next);
    } catch (e) {
      if (requestId === templateListRequestRef.current) showToast(String(e), "error");
    }
  };

  /** 未保存の変更があれば自動保存する（履歴が残るため安全） */
  const autoSaveIfDirty = async (): Promise<boolean> => {
    const t = tplRef.current;
    if (!t || !localDirtyRef.current) return true;
    try {
      await updateTemplate({
        id: t.id,
        expected_version: t.version,
        name: t.name,
        description: t.description,
        base_template: t.base_template,
        problem_template: t.problem_template,
        answer_template: t.answer_template,
        compile_method: t.compile_method,
        packages_memo: t.packages_memo,
      });
      setTpl((current) =>
        current && current.id === t.id ? { ...current, version: t.version + 1 } : current,
      );
      setLocalDirty(false);
      setDirty(false);
      clearTemplateDraft(t.id);
      showToast("編集中のテンプレートを自動保存しました");
      return true;
    } catch (e) {
      if (e instanceof ConflictError) {
        const overwrite = await confirm(
          "他の端末でテンプレートが更新されています。\n「OK」: 自分の変更で上書き\n「キャンセル」: サーバー版を読み込む",
        );
        if (overwrite) {
          try {
            const warnings = await updateTemplate({
              id: t.id,
              expected_version: null,
              name: t.name,
              description: t.description,
              base_template: t.base_template,
              problem_template: t.problem_template,
              answer_template: t.answer_template,
              compile_method: t.compile_method,
              packages_memo: t.packages_memo,
            });
            setTpl((current) =>
              current ? { ...current, warnings, version: e.serverVersion + 1 } : current,
            );
            setLocalDirty(false);
            setDirty(false);
            clearTemplateDraft(t.id);
            await loadList();
            showToast("自分の変更で更新しました");
          } catch (overwriteError) {
            showToast(String(overwriteError), "error");
            return false;
          }
          return true;
        }
        try {
          setTpl(await getTemplate(t.id));
          setLocalDirty(false);
          setDirty(false);
          clearTemplateDraft(t.id);
          showToast("サーバー版を読み込みました");
        } catch (reloadError) {
          showToast(String(reloadError), "error");
          return false;
        }
        return true;
      }
      try {
        localStorage.setItem(
          templateDraftKey(t.id),
          JSON.stringify({ savedAt: Date.now(), template: t }),
        );
      } catch {
        /* localStorage不可なら未保存表示を維持する */
      }
      showToast(`${String(e)}\n（編集内容はこの端末に一時保存されています）`, "error");
      return false;
    }
  };

  const openTpl = async (id: number) => {
    const requestId = ++templateLoadRequestRef.current;
    try {
      if (tplRef.current && tplRef.current.id !== id) {
        if (!(await autoSaveIfDirty())) return;
      }
      const next = await getTemplate(id);
      if (requestId !== templateLoadRequestRef.current) return;
      setTpl(next);
      setTab("problem");
      setLocalDirty(false);
      setDirty(false);
      try {
        const raw = localStorage.getItem(templateDraftKey(next.id));
        if (raw) {
          const draft = JSON.parse(raw) as { savedAt: number; template: TemplateFull };
          const restore =
            !!draft.template &&
            (await confirm(
              "この端末に未送信のテンプレート編集が残っています。\n復元しますか？\n「キャンセル」で破棄します。",
            ));
          if (requestId !== templateLoadRequestRef.current) return;
          if (restore) {
            const version = Number.isFinite(draft.template.version) ? draft.template.version : -1;
            setTpl({ ...draft.template, version });
            setLocalDirty(true);
            setDirty(true);
          } else {
            clearTemplateDraft(next.id);
          }
        }
      } catch {
        clearTemplateDraft(next.id);
      }
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  useEffect(() => {
    setContextName("LaTeXテンプレート");
    loadList();
    return () => {
      setContextName("");
      setDirty(false);
    };
  }, []);

  const refreshFromRemote = async () => {
    const requestId = ++templateLoadRequestRef.current;
    await loadList();
    const current = tplRef.current;
    if (!current) return;
    try {
      const next = await getTemplate(current.id);
      if (requestId !== templateLoadRequestRef.current) return;
      if (localDirtyRef.current) {
        pendingTemplatesRefreshRef.current = true;
        return;
      }
      setTpl(next);
    } catch (e) {
      setTpl(null);
      showToast(String(e), "error");
    }
  };

  // 自動保存前の編集内容は上書きせず、dirty解消後にリモート版を反映する。
  useEffect(() => {
    if (seenTemplatesBumpRef.current === bumps.templates) return;
    seenTemplatesBumpRef.current = bumps.templates;
    if (localDirty) {
      pendingTemplatesRefreshRef.current = true;
      return;
    }
    void refreshFromRemote();
  }, [bumps.templates]);

  useEffect(() => {
    if (localDirty || !pendingTemplatesRefreshRef.current) return;
    pendingTemplatesRefreshRef.current = false;
    void refreshFromRemote();
  }, [localDirty]);

  const patch = (fields: Partial<TemplateFull>) => {
    setTpl((t) => (t ? { ...t, ...fields } : t));
    setLocalDirty(true);
    setDirty(true);
  };

  const saveTpl = async () => {
    const t = tplRef.current;
    if (!t) return;
    try {
      const warnings = await updateTemplate({
        id: t.id,
        expected_version: t.version,
        name: t.name,
        description: t.description,
        base_template: t.base_template,
        problem_template: t.problem_template,
        answer_template: t.answer_template,
        compile_method: t.compile_method,
        packages_memo: t.packages_memo,
      });
      setTpl((prev) => (prev ? { ...prev, warnings, version: t.version + 1 } : prev));
      setLocalDirty(false);
      setDirty(false);
      clearTemplateDraft(t.id);
      await loadList();
      const used = templates.find((x) => x.id === t.id)?.usage_count ?? 0;
      const usedNote =
        used > 0
          ? `\nこのテンプレートを使用中の教材 ${used}件へは、各教材の「最新版に更新」で反映されます。`
          : "";
      if (warnings.length > 0) {
        showToast(`保存しました（警告 ${warnings.length}件あり）${usedNote}`);
      } else {
        showToast(`保存しました${usedNote}`);
      }
    } catch (e) {
      if (e instanceof ConflictError) {
        const overwrite = await confirm(
          "他の端末でテンプレートが更新されています。\n「OK」: 自分の変更で上書き\n「キャンセル」: サーバー版を読み込む",
        );
        if (overwrite) {
          try {
            const warnings = await updateTemplate({
              id: t.id,
              expected_version: null,
              name: t.name,
              description: t.description,
              base_template: t.base_template,
              problem_template: t.problem_template,
              answer_template: t.answer_template,
              compile_method: t.compile_method,
              packages_memo: t.packages_memo,
            });
            setTpl((current) =>
              current ? { ...current, warnings, version: e.serverVersion + 1 } : current,
            );
            setLocalDirty(false);
            setDirty(false);
            clearTemplateDraft(t.id);
            await loadList();
            showToast("自分の変更で更新しました");
          } catch (overwriteError) {
            showToast(String(overwriteError), "error");
          }
          return;
        }
        try {
          setTpl(await getTemplate(t.id));
          setLocalDirty(false);
          setDirty(false);
          clearTemplateDraft(t.id);
          showToast("サーバー版を読み込みました");
        } catch (reloadError) {
          showToast(String(reloadError), "error");
        }
        return;
      }
      try {
        localStorage.setItem(
          templateDraftKey(t.id),
          JSON.stringify({ savedAt: Date.now(), template: t }),
        );
      } catch {
        /* localStorage不可なら未保存表示を維持する */
      }
      showToast(`${String(e)}\n（編集内容はこの端末に一時保存されています）`, "error");
    }
  };

  // Ctrl+S で保存
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        saveTpl();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const onCreate = async () => {
    try {
      const id = await createTemplate("");
      await loadList();
      await openTpl(id);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDuplicate = async () => {
    if (!tpl) return;
    try {
      const id = await duplicateTemplate(tpl.id);
      await loadList();
      await openTpl(id);
      showToast("複製しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDelete = async () => {
    if (!tpl) return;
    const used = templates.find((t) => t.id === tpl.id)?.usage_count ?? 0;
    const warn =
      used > 0
        ? `\nこのテンプレートは ${used} 件の教材で使用されていますが、各教材はスナップショットを保持しているため出力は影響を受けません。`
        : "";
    if (!(await confirm(`テンプレート「${tpl.name}」を削除しますか？${warn}`))) return;
    try {
      await deleteTemplate(tpl.id);
      clearTemplateDraft(tpl.id);
      setTpl(null);
      await loadList();
      showToast("削除しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onExport = async () => {
    if (!tpl) return;
    if (!isTauri) {
      showToast("この操作はWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const dest = await saveDialog({
        defaultPath: `${tpl.name}.kyozai-tpl.json`,
        filters: [{ name: "教材工房テンプレート", extensions: ["json"] }],
      });
      if (!dest) return;
      await exportTemplate(tpl.id, dest);
      showToast(`エクスポートしました:\n${dest}`);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onImportJson = async () => {
    if (!isTauri) {
      showToast("この操作はWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const file = await openDialog({
        multiple: false,
        filters: [{ name: "教材工房テンプレート", extensions: ["json"] }],
      });
      if (!file) return;
      const id = await importTemplateFile(file as string);
      await loadList();
      await openTpl(id);
      showToast("インポートしました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onImportTex = async () => {
    if (!isTauri) {
      showToast("この操作はWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const file = await openDialog({
        multiple: false,
        filters: [{ name: "LaTeXファイル", extensions: ["tex"] }],
      });
      if (!file) return;
      const analysis = await analyzeTexFile(file as string);
      setImportWizard({ path: file as string, analysis });
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onTestCompile = async (kind: "problems" | "answers") => {
    if (!tpl) return;
    if (localDirty) await saveTpl();
    setCompiling(kind);
    try {
      const result = await testCompileTemplate(tpl.id, kind);
      setCompileResult(result);
      setLastCompile({
        ...result,
        label: `テンプレート「${tpl.name}」（${kind === "answers" ? "解答冊子" : "問題冊子"}・サンプルデータ）`,
      });
      if (!result.success) setLogOpen(true);
      if (result.success && result.pdf_path) {
        await openCompiledFile(result.pdf_path).catch(() => {});
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setCompiling(null);
    }
  };

  const onAddAsset = async () => {
    if (!tpl) return;
    if (!isTauri) {
      showToast("この操作はWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const file = await openDialog({ multiple: false });
      if (!file) return;
      await addTemplateAsset(tpl.id, file as string);
      await openTpl(tpl.id);
      showToast("アセットを追加しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const openHistory = async () => {
    if (!tpl) return;
    try {
      setVersions(await listTemplateVersions(tpl.id));
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onRestoreVersion = async (versionId: number) => {
    if (!tpl) return;
    if (!(await confirm("この履歴の内容に戻しますか？\n（現在の内容も履歴として保存されます）"))) return;
    try {
      await restoreTemplateVersion(versionId);
      setVersions(null);
      await openTpl(tpl.id);
      showToast("履歴から復元しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const fieldKey: Record<TplTab, "problem_template" | "answer_template" | "base_template"> = {
    problem: "problem_template",
    answer: "answer_template",
    base: "base_template",
  };
  const currentText = tpl ? tpl[fieldKey[tab]] : "";

  const insertPlaceholder = (text: string) => {
    const ta = textareaRef.current;
    if (!ta || !tpl) return;
    const start = ta.selectionStart;
    const end = ta.selectionEnd;
    const newValue = currentText.slice(0, start) + text + currentText.slice(end);
    patch({ [fieldKey[tab]]: newValue } as Partial<TemplateFull>);
    requestAnimationFrame(() => {
      ta.focus();
      ta.setSelectionRange(start + text.length, start + text.length);
    });
  };

  /** ログのエラー行（.tex:NN:）クリックでエディタの該当行へ移動 */
  const jumpToLine = (line: number) => {
    const ta = textareaRef.current;
    if (!ta || !tpl) return;
    const lines = currentText.split("\n");
    const target = Math.min(line, lines.length) - 1;
    let offset = 0;
    for (let i = 0; i < target; i++) offset += lines[i].length + 1;
    setCompileResult(null);
    requestAnimationFrame(() => {
      ta.focus();
      ta.setSelectionRange(offset, offset + (lines[target]?.length ?? 0));
      const lineHeight = 21.5;
      ta.scrollTop = Math.max(0, target * lineHeight - ta.clientHeight / 2);
    });
  };

  return (
    <div className="template-split flex h-full min-w-0">
      {/* 左: テンプレート一覧 */}
      <div
        className="template-list-pane flex w-72 shrink-0 flex-col border-r"
        style={{ background: "var(--panel)", borderColor: "var(--border)" }}
      >
        <div
          className="flex items-center justify-between border-b px-3 py-2"
          style={{ borderColor: "var(--border)" }}
        >
          <span className="section-label">LaTeXテンプレート</span>
          <button onClick={onCreate} className="btn btn-outline btn-sm">
            ＋新規
          </button>
        </div>
        <div className="flex gap-1.5 border-b px-3 py-1.5" style={{ borderColor: "var(--border)" }}>
          <button onClick={onImportTex} className="btn btn-ghost btn-sm flex-1" title="既存の.texファイルから取り込み">
            .tex取り込み
          </button>
          <button onClick={onImportJson} className="btn btn-ghost btn-sm flex-1" title="エクスポートしたテンプレートを読み込み">
            インポート
          </button>
        </div>
        <div className="flex-1 space-y-1.5 overflow-y-auto p-2">
          {templates.map((t) => (
            <button
              key={t.id}
              onClick={() => openTpl(t.id)}
              className="card card-glow w-full px-3 py-2 text-left"
              style={tpl?.id === t.id ? { borderColor: "var(--accent)", background: "var(--accent-dim)" } : undefined}
            >
              <div className="truncate text-sm font-semibold">{t.name}</div>
              <div className="truncate text-[11px]" style={{ color: "var(--muted)" }}>
                {t.description || t.compile_method}
              </div>
              <div className="mt-0.5 flex gap-2 text-[10px]" style={{ color: "var(--muted)" }}>
                <span>{t.usage_count}教材で使用</span>
                <span>更新 {t.updated_at}</span>
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* 右: テンプレート編集 */}
      {tpl == null ? (
        <div className="flex flex-1 items-center justify-center text-sm" style={{ color: "var(--muted)" }}>
          左の一覧からテンプレートを選択するか、新規作成・取り込みを行ってください
        </div>
      ) : (
        <div className="template-editor-pane flex min-w-0 flex-1 flex-col">
          {/* ヘッダー */}
          <div className="template-editor-header flex items-center gap-2 border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
            <input
              value={tpl.name}
              onChange={(e) => patch({ name: e.target.value })}
              className="input min-w-0 flex-1 font-semibold"
              placeholder="テンプレート名"
            />
            <select
              value={tpl.compile_method}
              onChange={(e) => patch({ compile_method: e.target.value })}
              className="select text-xs"
              title="コンパイル方式"
            >
              <option value="uplatex+dvipdfmx">uplatex + dvipdfmx</option>
            </select>
            {localDirty && <span className="badge badge-warn">● 未保存</span>}
            <button onClick={saveTpl} className="btn btn-solid">
              保存 (Ctrl+S)
            </button>
          </div>

          {/* 説明・操作行 */}
          <div
            className="flex flex-wrap items-center gap-1.5 border-b px-3 py-1.5"
            style={{ borderColor: "var(--border)" }}
          >
            <input
              value={tpl.description}
              onChange={(e) => patch({ description: e.target.value })}
              className="input min-w-40 flex-1 text-xs"
              placeholder="説明"
            />
            <input
              value={tpl.packages_memo}
              onChange={(e) => patch({ packages_memo: e.target.value })}
              className="input min-w-40 flex-1 text-xs"
              placeholder="必要パッケージのメモ（記録用・出力には影響しません。\usepackage はテンプレート本文に書きます）"
              title="この欄はメモです。パッケージを追加するには、テンプレート本文のプリアンブルに \usepackage{...} を書いてください"
            />
            <button onClick={openHistory} className="btn btn-ghost btn-sm">
              履歴
            </button>
            <button onClick={() => setShowAi(true)} className="btn btn-outline btn-sm">
              <Icon name="sparkle" size={15} /> AI変換
            </button>
            <button onClick={onDuplicate} className="btn btn-ghost btn-sm">
              複製
            </button>
            <button onClick={onExport} className="btn btn-ghost btn-sm">
              エクスポート
            </button>
            <button onClick={onDelete} className="btn btn-danger btn-sm">
              削除
            </button>
            <span className="mx-1 h-4 w-px" style={{ background: "var(--border)" }} />
            <button
              onClick={() => onTestCompile("problems")}
              disabled={compiling != null}
              className="btn btn-outline btn-sm"
              title="サンプル教材データでコンパイルして確認"
            >
              {compiling === "problems" ? "生成中..." : <><Icon name="play" size={15} /> プレビュー(問題)</>}
            </button>
            <button
              onClick={() => onTestCompile("answers")}
              disabled={compiling != null}
              className="btn btn-outline btn-sm"
            >
              {compiling === "answers" ? "生成中..." : <><Icon name="play" size={15} /> プレビュー(解答)</>}
            </button>
          </div>

          {/* 警告 */}
          {tpl.warnings.length > 0 && (
            <div
              className="space-y-0.5 border-b px-3 py-1.5 text-xs"
              style={{ borderColor: "rgba(251,191,36,0.3)", background: "var(--warn-dim)", color: "var(--warn)" }}
            >
              {tpl.warnings.map((w, i) => (
                <div key={i}><Icon name="warning" size={14} /> {w}</div>
              ))}
            </div>
          )}

          {/* タブ */}
          <div className="template-tabs flex items-center border-b" style={{ borderColor: "var(--border)" }}>
            {(Object.keys(TAB_LABELS) as TplTab[]).map((t) => (
              <button key={t} onClick={() => setTab(t)} className={`tab ${tab === t ? "tab-active" : ""}`}>
                {TAB_LABELS[t]}
              </button>
            ))}
            <span className="template-tab-help ml-auto pr-3 text-[10px]" style={{ color: "var(--muted)" }}>
              共通（本文用）は、問題冊子用・解答冊子用が空のときに使われます
            </span>
          </div>

          {/* プレースホルダ挿入 */}
          <div className="flex flex-wrap gap-1 border-b px-2 py-1" style={{ borderColor: "var(--border)" }}>
            {PLACEHOLDERS.map((p) => (
              <button
                key={p}
                onClick={() => insertPlaceholder(p)}
                className="rounded border px-1.5 py-0.5 font-mono text-[10.5px]"
                style={{
                  borderColor: "rgba(197,183,223,0.34)",
                  color: "var(--success)",
                  background: "var(--success-dim)",
                }}
                title="カーソル位置に挿入"
              >
                {p}
              </button>
            ))}
            <button
              onClick={() => insertPlaceholder("% APP_BODY_START\n% APP_BODY_END\n")}
              className="rounded border px-1.5 py-0.5 font-mono text-[10.5px]"
              style={{
                borderColor: "rgba(157,108,242,0.34)",
                color: "var(--purple)",
                background: "var(--purple-dim)",
              }}
              title="この2つのコメントの間に本文が挿入されます"
            >
              % APP_BODY マーカー
            </button>
          </div>

          {/* エディタ */}
          <div className="min-h-0 flex-1 p-2">
            <LatexEditor
              key={`${tpl.id}-${tab}`}
              ref={textareaRef}
              value={currentText}
              onChange={(v) => patch({ [fieldKey[tab]]: v } as Partial<TemplateFull>)}
              className="h-full"
              placeholder={
                tab === "base"
                  ? "（任意）問題冊子用・解答冊子用が空の場合に使われる共通テンプレート"
                  : "LaTeXテンプレートを入力。{{BODY}} などのプレースホルダが置換されます"
              }
            />
          </div>

          {/* アセット */}
          <div className="border-t px-3 py-1.5" style={{ borderColor: "var(--border)" }}>
            <div className="flex items-center gap-2">
              <span className="section-label">テンプレート用アセット（画像・.sty等、コンパイル時にコピーされます）</span>
              <button onClick={onAddAsset} className="btn btn-ghost btn-sm">
                ＋追加
              </button>
            </div>
            {tpl.assets.length > 0 && (
              <ul className="mt-1 flex flex-wrap gap-2">
                {tpl.assets.map((a) => (
                  <li key={a.id} className="flex items-center gap-1 text-xs" style={{ color: "var(--muted)" }}>
                    <code className="rounded px-1" style={{ background: "var(--panel-3)", color: "var(--accent)" }}>
                      {a.file_name}
                    </code>
                    <button
                      onClick={async () => {
                        await removeTemplateAsset(a.id);
                        await openTpl(tpl.id);
                      }}
                      className="opacity-60 hover:opacity-100"
                      style={{ color: "var(--danger)" }}
                    >
                      ✕
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          {showAi && (
            <AiConvertDialog
              onClose={() => setShowAi(false)}
              insertTargets={[
                {
                  label: `${TAB_LABELS[tab]}へ挿入`,
                  entityType: "template",
                  entityId: tpl.id,
                  field: fieldKey[tab],
                  insert: insertPlaceholder,
                },
              ]}
            />
          )}
        </div>
      )}

      {/* テストコンパイル結果 */}
      {compileResult && (
        <Modal
          title={compileResult.success ? "テストコンパイル 成功" : "テストコンパイル 失敗"}
          onClose={() => setCompileResult(null)}
          wide
        >
          <p
            className="mb-3 text-sm whitespace-pre-wrap"
            style={{ color: compileResult.success ? "var(--success)" : "var(--danger)" }}
          >
            {compileResult.message}
          </p>
          <p className="mb-2 text-xs" style={{ color: "var(--muted)" }}>
            エラー行（.tex:行番号）をクリックするとエディタの該当行へ移動します（本文挿入により行番号がずれる場合があります）
          </p>
          <pre className="log-pre max-h-[50vh] overflow-auto rounded p-3" style={{ background: "#080b11" }}>
            {(compileResult.log || "(ログなし)").split("\n").map((line, i) => {
              const m = line.match(/\.tex:(\d+):/) || line.match(/^l\.(\d+)/);
              const isErr = line.startsWith("!") || m != null;
              return (
                <div
                  key={i}
                  className={`${isErr ? "log-line-error" : ""} ${m ? "log-line-click" : ""}`}
                  onClick={m ? () => jumpToLine(Number(m[1])) : undefined}
                >
                  {line || " "}
                </div>
              );
            })}
          </pre>
          <div className="mt-3 flex justify-end gap-2">
            {compileResult.success && compileResult.pdf_path && (
              <button onClick={() => openCompiledFile(compileResult.pdf_path!)} className="btn btn-solid">
                PDFを開く
              </button>
            )}
            <button onClick={() => setCompileResult(null)} className="btn btn-ghost">
              閉じる
            </button>
          </div>
        </Modal>
      )}

      {/* 履歴モーダル */}
      {versions && (
        <Modal title="テンプレートの変更履歴" onClose={() => setVersions(null)}>
          {versions.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--muted)" }}>
              履歴はまだありません（保存すると記録されます）。
            </p>
          ) : (
            <ul className="max-h-[60vh] space-y-1 overflow-y-auto">
              {versions.map((v) => (
                <li key={v.id} className="card flex items-center gap-2 px-3 py-1.5 text-xs">
                  <span className="font-semibold">{v.saved_at}</span>
                  <span className="min-w-0 flex-1 truncate" style={{ color: "var(--muted)" }}>
                    {v.name}
                  </span>
                  <button onClick={() => onRestoreVersion(v.id)} className="btn btn-outline btn-sm">
                    復元
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Modal>
      )}

      {/* .tex取り込みウィザード */}
      {importWizard && (
        <ImportWizard
          path={importWizard.path}
          analysis={importWizard.analysis}
          onClose={() => setImportWizard(null)}
          onImported={async (id) => {
            setImportWizard(null);
            await loadList();
            await openTpl(id);
            showToast("テンプレートを取り込みました。「プレビュー」でコンパイルテストできます。");
          }}
        />
      )}
    </div>
  );
}

/** 既存.tex取り込みウィザード */
function ImportWizard({
  path,
  analysis,
  onClose,
  onImported,
}: {
  path: string;
  analysis: ImportAnalysis;
  onClose: () => void;
  onImported: (id: number) => Promise<void>;
}) {
  const { showToast } = useApp();
  const fileName = path.split(/[\\/]/).pop() ?? path;
  const [name, setName] = useState(fileName.replace(/\.tex$/i, ""));
  const canAsIs = analysis.has_body_placeholder || analysis.has_markers;
  const [mode, setMode] = useState<string>(canAsIs ? "as_is" : "replace_body");

  const run = async () => {
    try {
      const id = await importTemplateFromTex(path, name, mode);
      await onImported(id);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  return (
    <Modal title={`.texテンプレートの取り込み — ${fileName}`} onClose={onClose} wide>
      <div className="space-y-3 text-sm">
        <div className="grid grid-cols-2 gap-2">
          <div className="card px-3 py-2">
            <div className="section-label mb-1">文書クラス</div>
            <code style={{ color: "var(--accent)" }}>{analysis.doc_class || "（検出できず）"}</code>
          </div>
          <div className="card px-3 py-2">
            <div className="section-label mb-1">本文挿入位置の検出</div>
            <div className="text-xs">
              {analysis.has_body_placeholder && <div style={{ color: "var(--success)" }}>✓ {"{{BODY}}"} プレースホルダあり</div>}
              {analysis.has_markers && <div style={{ color: "var(--success)" }}>✓ % APP_BODY マーカーあり</div>}
              {!canAsIs && <div style={{ color: "var(--warn)" }}><Icon name="warning" size={14} /> 挿入位置が未指定（下で選択してください）</div>}
            </div>
          </div>
        </div>
        <div className="card px-3 py-2">
          <div className="section-label mb-1">検出されたパッケージ</div>
          <div className="flex flex-wrap gap-1">
            {analysis.packages.length === 0 ? (
              <span className="text-xs" style={{ color: "var(--muted)" }}>
                なし
              </span>
            ) : (
              analysis.packages.map((p) => (
                <code key={p} className="rounded px-1.5 text-xs" style={{ background: "var(--panel-3)" }}>
                  {p}
                </code>
              ))
            )}
          </div>
        </div>
        {analysis.referenced_files.length > 0 && (
          <div className="card px-3 py-2">
            <div className="section-label mb-1">参照ファイル（アプリ管理領域へコピーされます）</div>
            <div className="flex flex-wrap gap-1">
              {analysis.referenced_files.map((f) => (
                <code key={f} className="rounded px-1.5 text-xs" style={{ background: "var(--panel-3)", color: "var(--accent)" }}>
                  {f}
                </code>
              ))}
            </div>
          </div>
        )}

        <div>
          <div className="section-label mb-1">本文の挿入方法</div>
          <label className="flex items-start gap-2 py-1">
            <input type="radio" checked={mode === "as_is"} onChange={() => setMode("as_is")} disabled={!canAsIs} />
            <span className={canAsIs ? "" : "opacity-40"}>
              そのまま使う
              <span className="block text-xs" style={{ color: "var(--muted)" }}>
                既にある {"{{BODY}}"} プレースホルダまたは % APP_BODY_START / END コメントの位置に問題一覧を挿入します
              </span>
            </span>
          </label>
          <label className="flex items-start gap-2 py-1">
            <input
              type="radio"
              checked={mode === "replace_body"}
              onChange={() => setMode("replace_body")}
              disabled={!analysis.has_document_env}
            />
            <span className={analysis.has_document_env ? "" : "opacity-40"}>
              本文全体を置き換える
              <span className="block text-xs" style={{ color: "var(--muted)" }}>
                \begin{"{document}"} 〜 \end{"{document}"} の中身を {"{{BODY}}"} に置き換えます（プリアンブル・独自コマンドは維持）
              </span>
            </span>
          </label>
        </div>

        <div>
          <label className="section-label mb-0.5 block">テンプレート名</label>
          <input value={name} onChange={(e) => setName(e.target.value)} className="input w-full" />
        </div>

        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="btn btn-ghost">
            キャンセル
          </button>
          <button onClick={run} className="btn btn-solid">
            取り込む
          </button>
        </div>
      </div>
    </Modal>
  );
}
