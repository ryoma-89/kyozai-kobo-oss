import { useEffect, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  addTreeNode,
  deleteTreeNode,
  exportBank,
  importBank,
  moveTreeNode,
  renameTreeNode,
  type BankScope,
} from "../api";
import { useApp } from "../store";
import { isTauri } from "../transport";
import type { NodeKind } from "../types";

interface EditState {
  kind: NodeKind;
  id: number | null; // null = 新規追加
  parentId: number | null;
  name: string;
}

/** 科目 → 分野 → 単元 の階層ツリー */
export function TreePanel() {
  const { tree, refreshTree, selectedUnitId, selectUnit, showToast, confirm, dirty, setDirty } = useApp();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [edit, setEdit] = useState<EditState | null>(null);

  /** 問題編集中に単元を切り替えると編集画面が破棄されるため、未保存なら確認する */
  const onSelectUnit = async (id: number) => {
    if (dirty && !(await confirm("未保存の変更があります。保存せずに別の単元へ移動しますか？"))) return;
    setDirty(false);
    selectUnit(id);
  };

  useEffect(() => {
    refreshTree();
  }, []);

  const toggle = (key: string) => {
    const next = new Set(expanded);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    setExpanded(next);
  };

  const submitEdit = async () => {
    if (!edit) return;
    try {
      if (edit.name.trim() === "") {
        setEdit(null);
        return;
      }
      if (edit.id === null) {
        await addTreeNode(edit.kind, edit.parentId, edit.name);
      } else {
        await renameTreeNode(edit.kind, edit.id, edit.name);
      }
      setEdit(null);
      await refreshTree();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDelete = async (kind: NodeKind, id: number, name: string) => {
    const label = kind === "subject" ? "科目" : kind === "field" ? "分野" : "単元";
    const warn =
      kind === "unit"
        ? "この単元に属する問題もすべて削除されます。"
        : "配下の階層と問題もすべて削除されます。";
    if (!(await confirm(`${label}「${name}」を削除しますか？\n${warn}`))) return;
    try {
      await deleteTreeNode(kind, id);
      if (kind === "unit" && selectedUnitId === id) selectUnit(null);
      await refreshTree();
      showToast("削除しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onMove = async (kind: NodeKind, id: number, delta: number) => {
    try {
      await moveTreeNode(kind, id, delta);
      await refreshTree();
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  /** 科目・分野・単元・全体をJSONへエクスポート */
  const onExport = async (scope: BankScope, id: number | null, name: string) => {
    if (!isTauri) {
      showToast("エクスポートはWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const dest = await saveDialog({
        defaultPath: `問題バンク_${name}.json`,
        filters: [{ name: "教材工房 問題バンク", extensions: ["json"] }],
      });
      if (!dest) return;
      await exportBank(scope, id, null, dest);
      showToast(`エクスポートしました:\n${dest}`);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onImport = async () => {
    if (!isTauri) {
      showToast("インポートはWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const file = await openDialog({
        multiple: false,
        filters: [{ name: "教材工房 問題バンク", extensions: ["json"] }],
      });
      if (!file) return;
      const r = await importBank(file as string);
      await refreshTree();
      showToast(
        `インポートしました\n問題 ${r.problems_imported}件（科目+${r.subjects_created} / 分野+${r.fields_created} / 単元+${r.units_created}、同名の階層にはマージ）`,
      );
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const editRow = (state: EditState) => (
    <input
      autoFocus
      value={state.name}
      onChange={(e) => setEdit({ ...state, name: e.target.value })}
      onBlur={submitEdit}
      onKeyDown={(e) => {
        if (e.key === "Enter") submitEdit();
        if (e.key === "Escape") setEdit(null);
      }}
      className="input my-0.5 w-full px-1.5 py-0.5 text-sm"
      placeholder="名前を入力してEnter"
    />
  );

  const actBtn = "rounded px-1 text-[11px] transition-colors";
  const rowActions = (kind: NodeKind, id: number, name: string, addChild?: { kind: NodeKind; label: string }) => (
    <span className="ml-auto hidden shrink-0 gap-0.5 group-hover:flex">
      {addChild && (
        <button
          title={`${addChild.label}を追加`}
          onClick={(e) => {
            e.stopPropagation();
            setEdit({ kind: addChild.kind, id: null, parentId: id, name: "" });
          }}
          className={actBtn}
          style={{ color: "var(--accent)" }}
        >
          ＋
        </button>
      )}
      <button
        title="この階層をエクスポート"
        onClick={(e) => {
          e.stopPropagation();
          onExport(kind, id, name);
        }}
        className={actBtn}
        style={{ color: "var(--muted)" }}
      >
        ⇩
      </button>
      <button
        title="名前を変更"
        onClick={(e) => {
          e.stopPropagation();
          setEdit({ kind, id, parentId: null, name });
        }}
        className={actBtn}
        style={{ color: "var(--muted)" }}
      >
        ✎
      </button>
      <button
        title="上へ移動"
        onClick={(e) => {
          e.stopPropagation();
          onMove(kind, id, -1);
        }}
        className={actBtn}
        style={{ color: "var(--muted)" }}
      >
        ↑
      </button>
      <button
        title="下へ移動"
        onClick={(e) => {
          e.stopPropagation();
          onMove(kind, id, 1);
        }}
        className={actBtn}
        style={{ color: "var(--muted)" }}
      >
        ↓
      </button>
      <button
        title="削除"
        onClick={(e) => {
          e.stopPropagation();
          onDelete(kind, id, name);
        }}
        className={actBtn}
        style={{ color: "var(--danger)" }}
      >
        ✕
      </button>
    </span>
  );

  return (
    <div className="flex h-full flex-col">
      <div
        className="flex items-center justify-between border-b px-3 py-2"
        style={{ borderColor: "var(--border)" }}
      >
        <span className="section-label">問題バンク</span>
        <span className="flex gap-1">
          <button
            onClick={() => setEdit({ kind: "subject", id: null, parentId: null, name: "" })}
            className="btn btn-ghost btn-sm"
          >
            ＋科目
          </button>
          <button onClick={onImport} className="btn btn-ghost btn-sm" title="JSONファイルから問題バンクを取り込み">
            取込
          </button>
          <button
            onClick={() => onExport("all", null, "全体")}
            className="btn btn-ghost btn-sm"
            title="問題バンク全体をJSONへエクスポート（各階層は行の⇩から）"
          >
            出力
          </button>
        </span>
      </div>
      <div className="flex-1 overflow-y-auto px-2 py-1">
        {tree.length === 0 && (
          <p className="px-2 py-4 text-xs" style={{ color: "var(--muted)" }}>
            科目がありません。「＋科目」から作成するか、設定画面からサンプルデータを追加できます。
          </p>
        )}
        {tree.map((s) => (
          <div key={s.id}>
            <div
              className="group flex cursor-pointer items-center gap-1 rounded px-1 py-1 text-sm font-semibold hover:bg-[var(--panel-3)]"
              onClick={() => toggle(`s${s.id}`)}
            >
              <span className="w-3 text-xs" style={{ color: "var(--muted)" }}>
                {expanded.has(`s${s.id}`) ? "▾" : "▸"}
              </span>
              {edit && edit.kind === "subject" && edit.id === s.id ? (
                editRow(edit)
              ) : (
                <>
                  <span className="truncate">{s.name}</span>
                  {rowActions("subject", s.id, s.name, { kind: "field", label: "分野" })}
                </>
              )}
            </div>
            {edit && edit.kind === "field" && edit.id === null && edit.parentId === s.id && (
              <div className="pl-6">{editRow(edit)}</div>
            )}
            {expanded.has(`s${s.id}`) &&
              s.fields.map((f) => (
                <div key={f.id} className="pl-4">
                  <div
                    className="group flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-sm hover:bg-[var(--panel-3)]"
                    onClick={() => toggle(`f${f.id}`)}
                  >
                    <span className="w-3 text-xs" style={{ color: "var(--muted)" }}>
                      {expanded.has(`f${f.id}`) ? "▾" : "▸"}
                    </span>
                    {edit && edit.kind === "field" && edit.id === f.id ? (
                      editRow(edit)
                    ) : (
                      <>
                        <span className="truncate">{f.name}</span>
                        {rowActions("field", f.id, f.name, { kind: "unit", label: "単元" })}
                      </>
                    )}
                  </div>
                  {edit && edit.kind === "unit" && edit.id === null && edit.parentId === f.id && (
                    <div className="pl-6">{editRow(edit)}</div>
                  )}
                  {expanded.has(`f${f.id}`) &&
                    f.units.map((u) => (
                      <div
                        key={u.id}
                        className="group flex cursor-pointer items-center gap-1 rounded py-0.5 pr-1 pl-5 text-sm"
                        style={
                          selectedUnitId === u.id
                            ? {
                                background: "var(--accent-dim)",
                                color: "var(--accent)",
                                border: "1px solid rgba(157,108,242,0.38)",
                              }
                            : { color: "var(--muted)", border: "1px solid transparent" }
                        }
                        onClick={() => void onSelectUnit(u.id)}
                      >
                        {edit && edit.kind === "unit" && edit.id === u.id ? (
                          editRow(edit)
                        ) : (
                          <>
                            <span className="truncate">{u.name}</span>
                            <span className="ml-1 shrink-0 text-[10px] opacity-70">({u.problem_count})</span>
                            {rowActions("unit", u.id, u.name)}
                          </>
                        )}
                      </div>
                    ))}
                </div>
              ))}
          </div>
        ))}
        {edit && edit.kind === "subject" && edit.id === null && <div className="pl-2">{editRow(edit)}</div>}
      </div>
    </div>
  );
}
