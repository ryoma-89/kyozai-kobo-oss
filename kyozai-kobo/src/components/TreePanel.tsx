import { useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  createBankNode,
  deleteBankNode,
  exportBank,
  getBankNodeDeleteImpact,
  importBank,
  moveBankNode,
  renameBankNode,
  reorderBankNode,
} from "../api";
import { useApp } from "../store";
import { isTauri } from "../transport";
import type { BankNode, BankNodeDeleteImpact, BankNodeDeleteStrategy } from "../types";
import { Modal } from "./ui";

interface EditState {
  id: number | null;
  parentId: number | null;
  name: string;
}

interface DeleteState {
  node: BankNode;
  impact: BankNodeDeleteImpact;
}

function collectNodeIds(node: BankNode, ids = new Set<number>()): Set<number> {
  ids.add(node.id);
  node.children.forEach((child) => collectNodeIds(child, ids));
  return ids;
}

function flattenNodes(nodes: BankNode[], depth = 0): Array<{ node: BankNode; depth: number }> {
  return nodes.flatMap((node) => [
    { node, depth },
    ...flattenNodes(node.children, depth + 1),
  ]);
}

/** 任意深度の問題バンクツリー。すべてのノードを選択でき、子ノードと問題が共存できる。 */
export function TreePanel() {
  const {
    bankTree,
    refreshTree,
    selectedBankNodeId,
    selectBankNode,
    showToast,
    confirm,
    dirty,
    setDirty,
  } = useApp();
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [edit, setEdit] = useState<EditState | null>(null);
  const [moveTarget, setMoveTarget] = useState<BankNode | null>(null);
  const [deleteState, setDeleteState] = useState<DeleteState | null>(null);
  const editSubmittingRef = useRef(false);

  useEffect(() => {
    void refreshTree();
  }, []);

  const onSelectNode = async (id: number) => {
    if (selectedBankNodeId === id) return;
    if (dirty && !(await confirm("未保存の変更があります。保存せずに別の階層へ移動しますか？"))) return;
    setDirty(false);
    selectBankNode(id);
  };

  const toggle = (id: number) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const submitEdit = async () => {
    if (!edit || editSubmittingRef.current) return;
    const current = edit;
    const name = current.name.trim();
    if (!name) {
      setEdit(null);
      return;
    }
    editSubmittingRef.current = true;
    try {
      if (current.id == null) {
        const id = await createBankNode(current.parentId, name);
        if (current.parentId != null) {
          setExpanded((value) => new Set(value).add(current.parentId!));
        }
        setEdit(null);
        await refreshTree();
        selectBankNode(id);
      } else {
        await renameBankNode(current.id, name);
        setEdit(null);
        await refreshTree();
      }
    } catch (error) {
      showToast(String(error), "error");
    } finally {
      editSubmittingRef.current = false;
    }
  };

  const requestDelete = async (node: BankNode) => {
    try {
      setDeleteState({ node, impact: await getBankNodeDeleteImpact(node.id) });
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const executeDelete = async (strategy: BankNodeDeleteStrategy) => {
    if (!deleteState) return;
    if (strategy === "delete_all") {
      const ok = await confirm(
        `「${deleteState.node.name}」配下の問題 ${deleteState.impact.descendant_problem_count}件を完全に削除します。部品 ${deleteState.impact.descendant_part_count}件は未分類へ移動します。\nこの操作を実行しますか？`,
      );
      if (!ok) return;
    }
    try {
      const deletedIds = collectNodeIds(deleteState.node);
      const nextSelection =
        selectedBankNodeId != null && deletedIds.has(selectedBankNodeId)
          ? strategy === "move_to_parent"
            ? deleteState.impact.parent_id
            : null
          : selectedBankNodeId;
      await deleteBankNode(deleteState.node.id, strategy);
      setDeleteState(null);
      await refreshTree();
      if (nextSelection !== selectedBankNodeId) selectBankNode(nextSelection);
      showToast(strategy === "move_to_parent" ? "内容を親へ移して階層を削除しました" : "配下を削除しました");
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const onReorder = async (id: number, delta: number) => {
    try {
      await reorderBankNode(id, delta);
      await refreshTree();
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const onMove = async (node: BankNode, parentId: number | null) => {
    try {
      await moveBankNode(node.id, parentId);
      setMoveTarget(null);
      if (parentId != null) setExpanded((current) => new Set(current).add(parentId));
      await refreshTree();
      showToast("階層を移動しました");
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const onExport = async (node: BankNode | null) => {
    if (!isTauri) {
      showToast("エクスポートはWindowsアプリでのみ利用できます", "error");
      return;
    }
    try {
      const name = node?.name ?? "全体";
      const dest = await saveDialog({
        defaultPath: `問題バンク_${name}.json`,
        filters: [{ name: "教材工房 問題バンク", extensions: ["json"] }],
      });
      if (!dest) return;
      await exportBank(node ? "node" : "all", node?.id ?? null, null, dest);
      showToast(`エクスポートしました:\n${dest}`);
    } catch (error) {
      showToast(String(error), "error");
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
      const result = await importBank(file as string);
      await refreshTree();
      const created = result.nodes_created ?? result.subjects_created + result.fields_created + result.units_created;
      showToast(`インポートしました\n問題 ${result.problems_imported}件 / 階層 ${created}件作成`);
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const editRow = (state: EditState) => (
    <input
      autoFocus
      value={state.name}
      onChange={(event) => setEdit({ ...state, name: event.target.value })}
      onBlur={() => void submitEdit()}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          void submitEdit();
        }
        if (event.key === "Escape") setEdit(null);
      }}
      onClick={(event) => event.stopPropagation()}
      className="input my-0.5 min-w-0 flex-1 px-1.5 py-0.5 text-sm"
      placeholder="名前を入力してEnter"
    />
  );

  const actionButton = "rounded px-1 text-[11px] transition-colors";
  const rowActions = (node: BankNode) => (
    <span className="tree-actions ml-auto hidden shrink-0 gap-0.5 group-hover:flex">
      <button title="子階層を追加" onClick={(event) => { event.stopPropagation(); setExpanded((current) => new Set(current).add(node.id)); setEdit({ id: null, parentId: node.id, name: "" }); }} className={actionButton} style={{ color: "var(--accent)" }}>＋</button>
      <button title="この階層をエクスポート" onClick={(event) => { event.stopPropagation(); void onExport(node); }} className={actionButton} style={{ color: "var(--muted)" }}>⇩</button>
      <button title="名前を変更" onClick={(event) => { event.stopPropagation(); setEdit({ id: node.id, parentId: node.parent_id, name: node.name }); }} className={actionButton} style={{ color: "var(--muted)" }}>✎</button>
      <button title="別の階層へ移動" onClick={(event) => { event.stopPropagation(); setMoveTarget(node); }} className={actionButton} style={{ color: "var(--muted)" }}>↪</button>
      <button title="上へ移動" onClick={(event) => { event.stopPropagation(); void onReorder(node.id, -1); }} className={actionButton} style={{ color: "var(--muted)" }}>↑</button>
      <button title="下へ移動" onClick={(event) => { event.stopPropagation(); void onReorder(node.id, 1); }} className={actionButton} style={{ color: "var(--muted)" }}>↓</button>
      <button title="削除" onClick={(event) => { event.stopPropagation(); void requestDelete(node); }} className={actionButton} style={{ color: "var(--danger)" }}>✕</button>
    </span>
  );

  const renderNode = (node: BankNode, depth: number) => {
    const isExpanded = expanded.has(node.id);
    const isSelected = selectedBankNodeId === node.id;
    const addingChild = edit?.id == null && edit?.parentId === node.id;
    return (
      <div key={node.id}>
        <div
          className="group flex cursor-pointer items-center gap-1 rounded py-0.5 pr-1 text-sm hover:bg-[var(--panel-3)]"
          style={{
            paddingLeft: `${4 + depth * 14}px`,
            ...(isSelected
              ? { background: "var(--accent-dim)", color: "var(--accent)", border: "1px solid rgba(157,108,242,0.38)" }
              : { color: depth === 0 ? "var(--text)" : "var(--muted)", border: "1px solid transparent" }),
            fontWeight: depth === 0 ? 600 : 400,
          }}
          onClick={() => void onSelectNode(node.id)}
        >
          <button
            type="button"
            className="w-4 shrink-0 text-xs"
            style={{ color: "var(--muted)", visibility: node.children.length ? "visible" : "hidden" }}
            onClick={(event) => { event.stopPropagation(); toggle(node.id); }}
            aria-label={isExpanded ? "折りたたむ" : "展開する"}
          >
            {isExpanded ? "▾" : "▸"}
          </button>
          {edit?.id === node.id ? editRow(edit) : (
            <>
              <span className="min-w-0 flex-1 truncate">{node.name}</span>
              <span className="shrink-0 text-[10px] opacity-70" title={`直下 ${node.problem_count}件 / 配下合計 ${node.descendant_problem_count}件`}>
                ({node.problem_count}{node.descendant_problem_count !== node.problem_count ? `/${node.descendant_problem_count}` : ""})
              </span>
              {rowActions(node)}
            </>
          )}
        </div>
        {addingChild && <div style={{ paddingLeft: `${24 + depth * 14}px` }}>{editRow(edit)}</div>}
        {isExpanded && node.children.map((child) => renderNode(child, depth + 1))}
      </div>
    );
  };

  const flatMoveNodes = useMemo(() => flattenNodes(bankTree), [bankTree]);
  const forbiddenMoveIds = useMemo(
    () => moveTarget ? collectNodeIds(moveTarget) : new Set<number>(),
    [moveTarget],
  );

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
        <span className="section-label">問題バンク</span>
        <span className="flex gap-1">
          <button onClick={() => setEdit({ id: null, parentId: null, name: "" })} className="btn btn-ghost btn-sm">＋階層</button>
          <button onClick={() => void onImport()} className="btn btn-ghost btn-sm" title="JSONファイルから問題バンクを取り込み">取込</button>
          <button onClick={() => void onExport(null)} className="btn btn-ghost btn-sm" title="問題バンク全体をJSONへエクスポート">出力</button>
        </span>
      </div>
      <div className="tree-scroll flex-1 overflow-y-auto px-2 py-1">
        {bankTree.length === 0 && (
          <p className="px-2 py-4 text-xs" style={{ color: "var(--muted)" }}>
            階層がありません。「＋階層」から自由に作成できます。
          </p>
        )}
        {bankTree.map((node) => renderNode(node, 0))}
        {edit && edit.id == null && edit.parentId == null && <div className="pl-2">{editRow(edit)}</div>}
      </div>

      {moveTarget && (
        <Modal title={`「${moveTarget.name}」の移動先`} onClose={() => setMoveTarget(null)}>
          <div className="max-h-[60vh] space-y-0.5 overflow-y-auto">
            <button type="button" className="btn btn-ghost btn-sm w-full justify-start" disabled={moveTarget.parent_id == null} onClick={() => void onMove(moveTarget, null)}>
              ルートへ移動
            </button>
            {flatMoveNodes.map(({ node, depth }) => (
              <button
                type="button"
                key={node.id}
                className="btn btn-ghost btn-sm w-full justify-start disabled:opacity-35"
                style={{ paddingLeft: `${10 + depth * 16}px` }}
                disabled={forbiddenMoveIds.has(node.id) || moveTarget.parent_id === node.id}
                onClick={() => void onMove(moveTarget, node.id)}
              >
                {node.name}
              </button>
            ))}
          </div>
          <p className="mt-2 text-xs" style={{ color: "var(--muted)" }}>自分自身とその子孫は移動先にできません。</p>
        </Modal>
      )}

      {deleteState && (
        <Modal title={`「${deleteState.node.name}」を削除`} onClose={() => setDeleteState(null)}>
          <div className="space-y-3 text-sm">
            <p>
              子階層 {deleteState.impact.child_node_count}件、配下の問題 {deleteState.impact.descendant_problem_count}件
              （直下 {deleteState.impact.direct_problem_count}件）、配下の部品 {deleteState.impact.descendant_part_count}件
              （直下 {deleteState.impact.direct_part_count}件）があります。
            </p>
            {deleteState.impact.parent_id != null && (
              <button className="btn btn-outline w-full" onClick={() => void executeDelete("move_to_parent")}>
                子階層・直下の問題・部品を親へ移して削除
              </button>
            )}
            <button className="btn btn-danger w-full" onClick={() => void executeDelete("delete_all")}>
              配下の階層と問題を削除（部品は未分類へ）
            </button>
            <button className="btn btn-ghost w-full" onClick={() => setDeleteState(null)}>キャンセル</button>
          </div>
        </Modal>
      )}
    </div>
  );
}
