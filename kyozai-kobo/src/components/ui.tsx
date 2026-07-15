import { ReactNode } from "react";
import { useApp } from "../store";
import type { DifficultyRank } from "../types";

export function Modal({
  title,
  onClose,
  children,
  wide,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <div
      className="safe-area-overlay fixed inset-0 z-40 flex items-center justify-center bg-black/60"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`fade-in flex max-h-[90vh] w-full ${wide ? "max-w-4xl" : "max-w-lg"} flex-col rounded-md border shadow-2xl`}
        style={{ background: "var(--panel)", borderColor: "var(--border-strong)" }}
      >
        <div
          className="flex items-center justify-between border-b px-4 py-2.5"
          style={{ borderColor: "var(--border)" }}
        >
          <h2 className="text-sm font-bold" style={{ color: "var(--text)" }}>
            <span className="brand-mark mr-1.5">▸</span>
            {title}
          </h2>
          <button onClick={onClose} className="btn btn-ghost btn-sm" title="閉じる">
            ✕
          </button>
        </div>
        <div className="overflow-y-auto p-4">{children}</div>
      </div>
    </div>
  );
}

export function Toast() {
  const { toast, toastKind } = useApp();
  if (!toast) return null;
  return (
    <div
      className="safe-area-toast fade-in fixed bottom-10 left-1/2 z-50 -translate-x-1/2 rounded border px-4 py-2 text-sm whitespace-pre-wrap shadow-xl"
      style={
        toastKind === "error"
          ? { background: "#2a1418", borderColor: "rgba(241,106,117,0.5)", color: "var(--danger)" }
          : { background: "var(--panel-3)", borderColor: "var(--border-strong)", color: "var(--text)" }
      }
    >
      {toast}
    </div>
  );
}

export function ConfirmDialog() {
  const { confirmState, resolveConfirm } = useApp();
  if (!confirmState) return null;
  return (
    <div className="safe-area-overlay fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div
        className="fade-in w-full max-w-md rounded-md border p-5 shadow-2xl"
        style={{ background: "var(--panel)", borderColor: "rgba(241,106,117,0.35)" }}
      >
        <p className="mb-5 text-sm whitespace-pre-wrap" style={{ color: "var(--text)" }}>
          {confirmState.message}
        </p>
        <div className="flex justify-end gap-2">
          <button onClick={() => resolveConfirm(false)} className="btn btn-ghost">
            キャンセル
          </button>
          <button
            onClick={() => resolveConfirm(true)}
            className="btn"
            style={{ background: "var(--danger)", color: "#1b0c0e", fontWeight: 700 }}
          >
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

export function DifficultyBadge({ value }: { value: string }) {
  const cls = value === "基礎" ? "badge-basic" : value === "発展" ? "badge-advanced" : "badge-standard";
  return <span className={`badge ${cls}`}>{value}</span>;
}

export const DIFFICULTY_RANKS: { rank: DifficultyRank; label: string; description: string }[] = [
  { rank: "A", label: "基礎", description: "必須知識の確認" },
  { rank: "B", label: "標準", description: "授業で身につけたい問題" },
  { rank: "C", label: "応用", description: "やや難しい問題" },
  { rank: "D", label: "発展", description: "上位層・入試寄り" },
];

export function DifficultyRankBadge({
  rank,
  required,
  muted,
}: {
  rank?: DifficultyRank | string | null;
  required?: boolean;
  muted?: boolean;
}) {
  if (!rank && !required) {
    return <span className="badge badge-muted">未設定</span>;
  }
  const cls =
    rank === "A"
      ? "badge-basic"
      : rank === "C" || rank === "D"
        ? "badge-advanced"
        : muted
          ? "badge-muted"
          : "badge-standard";
  return (
    <span className={`badge ${cls}`} title={`${rank ?? "未設定"}${required ? " 最低限" : ""}`}>
      {rank ?? "未設定"}
      {required ? "★" : ""}
    </span>
  );
}

export function TagChips({ tags }: { tags: string[] }) {
  return (
    <span className="flex flex-wrap gap-1">
      {tags.map((t) => (
        <span key={t} className="chip">
          {t}
        </span>
      ))}
    </span>
  );
}
