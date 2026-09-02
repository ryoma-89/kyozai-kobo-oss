import { useEffect, useState } from "react";
import { searchPatterns } from "../api";
import { useApp } from "../store";
import type { PatternSummary } from "../types";
import { Modal, TagChips } from "./ui";

export const PATTERN_TYPE_LABELS: Record<string, string> = {
  strategy: "方針",
  technique: "手法",
  calculation_tip: "計算",
  check: "確認",
};

export function patternTypeLabel(type: string): string {
  return PATTERN_TYPE_LABELS[type] ?? type;
}

export function PatternTypeBadge({ type }: { type: string }) {
  return <span className="badge badge-muted shrink-0">{patternTypeLabel(type)}</span>;
}

export function PatternPicker({
  title = "定石を選択",
  excludeId,
  existingIds = [],
  onPick,
  onClose,
}: {
  title?: string;
  excludeId?: number | null;
  existingIds?: number[];
  onPick: (pattern: PatternSummary) => Promise<void> | void;
  onClose: () => void;
}) {
  const { showToast, bumps } = useApp();
  const [text, setText] = useState("");
  const [type, setType] = useState("");
  const [results, setResults] = useState<PatternSummary[]>([]);
  const [added, setAdded] = useState<number[]>([]);

  useEffect(() => {
    const timer = setTimeout(async () => {
      try {
        setResults(
          await searchPatterns({
            text,
            pattern_type: type || null,
            exclude_id: excludeId ?? null,
            limit: 100,
          }),
        );
      } catch (error) {
        showToast(String(error), "error");
      }
    }, 200);
    return () => clearTimeout(timer);
  }, [text, type, excludeId, bumps.patterns]);

  return (
    <Modal title={title} onClose={onClose} wide>
      <div className="mb-3 flex gap-2">
        <input
          autoFocus
          className="input min-w-0 flex-1"
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="タイトル・状況・候補・タグを検索"
        />
        <select className="select" value={type} onChange={(event) => setType(event.target.value)}>
          <option value="">種類: すべて</option>
          {Object.entries(PATTERN_TYPE_LABELS).map(([value, label]) => (
            <option key={value} value={value}>{label}</option>
          ))}
        </select>
      </div>
      <div className="max-h-[58vh] space-y-1 overflow-y-auto">
        {results.length === 0 ? (
          <p className="py-8 text-center text-sm" style={{ color: "var(--muted)" }}>該当する定石がありません。</p>
        ) : results.map((pattern) => {
          const linked = existingIds.includes(pattern.id) || added.includes(pattern.id);
          return (
            <button
              key={pattern.id}
              className="card card-glow flex w-full items-start gap-2 px-3 py-2 text-left disabled:opacity-60"
              disabled={linked}
              onClick={async () => {
                await onPick(pattern);
                setAdded((current) => [...current, pattern.id]);
              }}
            >
              <PatternTypeBadge type={pattern.pattern_type} />
              <span className="min-w-0 flex-1">
                <span className="block font-medium">{pattern.title}</span>
                {pattern.summary && <span className="mt-0.5 block text-xs" style={{ color: "var(--muted)" }}>{pattern.summary}</span>}
                <span className="mt-1 block"><TagChips tags={pattern.tags} /></span>
              </span>
              {linked && <span className="badge">関連済</span>}
            </button>
          );
        })}
      </div>
    </Modal>
  );
}
