import { ReactNode, useEffect, useRef, useState } from "react";
import { useApp } from "../store";
import type { DifficultyRank } from "../types";

/** 閉じる操作を退場アニメーション付きにする（アニメーション後に本来のクローズを呼ぶ） */
function useDismiss(onClose: () => void, durationMs = 160) {
  const [closing, setClosing] = useState(false);
  const timer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );
  const dismiss = () => {
    if (closing) return;
    setClosing(true);
    timer.current = window.setTimeout(onClose, durationMs);
  };
  return { closing, dismiss };
}

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
  const { closing, dismiss } = useDismiss(onClose);
  return (
    <div
      className={`safe-area-overlay modal-scrim ${closing ? "modal-scrim-out" : ""} fixed inset-0 z-40 flex items-center justify-center`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) dismiss();
      }}
    >
      <div
        className={`modal-panel ${closing ? "modal-panel-out" : ""} flex max-h-[90vh] w-full ${wide ? "max-w-4xl" : "max-w-lg"} flex-col rounded-md border shadow-2xl`}
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
          <button onClick={dismiss} className="btn btn-ghost btn-sm" title="閉じる">
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
      className="safe-area-toast toast-in fixed bottom-10 left-1/2 z-50 -translate-x-1/2 rounded border px-4 py-2 text-sm whitespace-pre-wrap shadow-xl"
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
  const [closing, setClosing] = useState(false);
  const timer = useRef<number | null>(null);
  // 常時マウントされるため、新しい確認が来たら閉じ状態をリセットする
  useEffect(() => {
    if (confirmState) setClosing(false);
  }, [confirmState]);
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );
  if (!confirmState) return null;
  const finish = (value: boolean) => {
    if (closing) return;
    setClosing(true);
    timer.current = window.setTimeout(() => resolveConfirm(value), 140);
  };
  return (
    <div className={`safe-area-overlay modal-scrim ${closing ? "modal-scrim-out" : ""} fixed inset-0 z-50 flex items-center justify-center`}>
      <div
        className={`modal-panel ${closing ? "modal-panel-out" : ""} w-full max-w-md rounded-md border p-5 shadow-2xl`}
        style={{ background: "var(--panel)", borderColor: "rgba(241,106,117,0.35)" }}
      >
        <p className="mb-5 text-sm whitespace-pre-wrap" style={{ color: "var(--text)" }}>
          {confirmState.message}
        </p>
        <div className="flex justify-end gap-2">
          <button onClick={() => finish(false)} className="btn btn-ghost">
            キャンセル
          </button>
          <button
            onClick={() => finish(true)}
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
