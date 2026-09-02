import { useEffect, useState } from "react";
import type { BankNode } from "../types";

export type PartBrowseScope = number | "all" | "unclassified" | null;

interface PartTreePanelProps {
  tree: BankNode[];
  selectedScope: PartBrowseScope;
  onSelectScope: (scope: Exclude<PartBrowseScope, null>) => void | Promise<void>;
}

function pathToNode(nodes: BankNode[], targetId: number, path: number[] = []): number[] | null {
  for (const node of nodes) {
    const next = [...path, node.id];
    if (node.id === targetId) return next;
    const found = pathToNode(node.children, targetId, next);
    if (found) return found;
  }
  return null;
}

/** 問題と共通のBankNodeを任意深度で表示する、部品用の読み取り専用ツリー。 */
export function PartTreePanel({ tree, selectedScope, onSelectScope }: PartTreePanelProps) {
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  useEffect(() => {
    if (typeof selectedScope !== "number") return;
    const path = pathToNode(tree, selectedScope);
    if (!path) return;
    setExpanded((current) => new Set([...current, ...path.slice(0, -1)]));
  }, [tree, selectedScope]);

  const toggle = (id: number) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const scopeStyle = (active: boolean) => active
    ? {
        background: "var(--accent-dim)",
        color: "var(--accent)",
        border: "1px solid rgba(157,108,242,0.38)",
      }
    : { color: "var(--muted)", border: "1px solid transparent" };

  const renderNode = (node: BankNode, depth: number): React.ReactNode => {
    const isExpanded = expanded.has(node.id);
    const hasChildren = node.children.length > 0;
    return (
      <div key={node.id}>
        <div className="flex items-center rounded" style={scopeStyle(selectedScope === node.id)}>
          <button
            type="button"
            onClick={() => hasChildren && toggle(node.id)}
            className="shrink-0 py-1 text-xs"
            style={{ paddingLeft: `${6 + depth * 14}px`, width: `${22 + depth * 14}px`, color: "var(--muted)" }}
            aria-label={hasChildren ? (isExpanded ? "折りたたむ" : "展開する") : undefined}
          >
            {hasChildren ? (isExpanded ? "▾" : "▸") : "·"}
          </button>
          <button
            type="button"
            onClick={() => void onSelectScope(node.id)}
            className="flex min-w-0 flex-1 items-center gap-1 py-1 pr-2 text-left text-sm"
          >
            <span className="min-w-0 flex-1 truncate">{node.name}</span>
            <span
              className="shrink-0 text-[10px] opacity-70"
              title={`直下 ${node.part_count}件 / 配下合計 ${node.descendant_part_count}件`}
            >
              ({node.part_count}{node.descendant_part_count !== node.part_count ? `/${node.descendant_part_count}` : ""})
            </span>
          </button>
        </div>
        {hasChildren && isExpanded && node.children.map((child) => renderNode(child, depth + 1))}
      </div>
    );
  };

  return (
    <aside className="flex h-full min-h-0 flex-col">
      <div className="border-b px-3 py-2" style={{ borderColor: "var(--border)" }}>
        <span className="section-label">部品ライブラリ</span>
        <p className="mt-1 text-[11px]" style={{ color: "var(--muted)" }}>
          問題バンクと共通の階層
        </p>
      </div>
      <div className="tree-scroll min-h-0 flex-1 overflow-y-auto px-2 py-1">
        <button
          type="button"
          onClick={() => void onSelectScope("all")}
          className="mb-0.5 flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm"
          style={scopeStyle(selectedScope === "all")}
        >
          <span className="w-3 text-xs">▦</span>
          <span className="min-w-0 flex-1 truncate">すべての部品</span>
        </button>
        <button
          type="button"
          onClick={() => void onSelectScope("unclassified")}
          className="mb-1 flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm"
          style={scopeStyle(selectedScope === "unclassified")}
        >
          <span className="w-3 text-xs">◇</span>
          <span className="min-w-0 flex-1 truncate">未分類</span>
        </button>

        {tree.length === 0 ? (
          <p className="px-2 py-4 text-xs" style={{ color: "var(--muted)" }}>
            階層がありません。問題バンクで階層を作成してください。
          </p>
        ) : tree.map((node) => renderNode(node, 0))}
      </div>
      <div className="border-t px-3 py-2 text-[10px]" style={{ borderColor: "var(--border)", color: "var(--muted)" }}>
        階層の追加・名称変更は問題バンクで行えます。
      </div>
    </aside>
  );
}
