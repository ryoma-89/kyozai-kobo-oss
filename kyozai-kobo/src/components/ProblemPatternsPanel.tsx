import { useEffect, useState } from "react";
import {
  linkProblemPattern,
  listPatternsForProblem,
  unlinkProblemPattern,
} from "../api";
import { useApp } from "../store";
import type { ProblemPatternView } from "../types";
import { PatternPicker, PatternTypeBadge } from "./PatternPicker";

export function ProblemPatternsPanel({ problemId }: { problemId: number }) {
  const { showToast, openPattern, bumps, dirty, confirm } = useApp();
  const [patterns, setPatterns] = useState<ProblemPatternView[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [picker, setPicker] = useState(false);
  const [relationType, setRelationType] = useState("applicable");

  const load = async () => {
    try {
      setPatterns(await listPatternsForProblem(problemId));
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  useEffect(() => { void load(); }, [problemId, bumps.patterns]);

  return (
    <section className="card mt-2 p-2">
      <button className="flex w-full items-center gap-2 text-left" onClick={() => setExpanded((current) => !current)}>
        <span className="text-xs">{expanded ? "▾" : "▸"}</span>
        <span className="section-label flex-1">関連定石</span>
        <span className="badge badge-muted">{patterns.length}</span>
      </button>
      {expanded && (
        <div className="mt-2 space-y-1 border-t pt-2" style={{ borderColor: "var(--border)" }}>
          {patterns.length === 0 && <p className="py-2 text-center text-xs" style={{ color: "var(--muted)" }}>関連定石はありません。</p>}
          {patterns.map((pattern) => (
            <div key={pattern.pattern_id} className="flex items-center gap-1.5 rounded px-1.5 py-1" style={{ background: "var(--panel-2)" }}>
              <PatternTypeBadge type={pattern.pattern_type} />
              <button className="min-w-0 flex-1 truncate text-left text-xs font-medium" title={pattern.summary} onClick={async () => {
                if (dirty && !(await confirm("未保存の問題編集があります。保存せずに定石ライブラリへ移動しますか？"))) return;
                openPattern(pattern.pattern_id);
              }}>{pattern.title}</button>
              <select
                className="select px-1 py-0.5 text-[11px]"
                value={pattern.relation_type}
                onChange={async (event) => {
                  try {
                    await linkProblemPattern(problemId, pattern.pattern_id, event.target.value);
                    await load();
                  } catch (error) { showToast(String(error), "error"); }
                }}
              >
                <option value="applicable">候補</option>
                <option value="used">使用</option>
              </select>
              <button className="btn btn-ghost btn-sm" onClick={async () => {
                try {
                  await unlinkProblemPattern(problemId, pattern.pattern_id);
                  await load();
                } catch (error) { showToast(String(error), "error"); }
              }}>解除</button>
            </div>
          ))}
          <div className="flex items-center justify-end gap-1 pt-1">
            <select className="select px-1 py-0.5 text-[11px]" value={relationType} onChange={(event) => setRelationType(event.target.value)}>
              <option value="applicable">候補として追加</option>
              <option value="used">使用として追加</option>
            </select>
            <button className="btn btn-outline btn-sm" onClick={() => setPicker(true)}>＋ 定石を関連付け</button>
          </div>
        </div>
      )}
      {picker && (
        <PatternPicker
          title="問題へ定石を関連付け"
          existingIds={patterns.map((pattern) => pattern.pattern_id)}
          onClose={() => setPicker(false)}
          onPick={async (pattern) => {
            try {
              await linkProblemPattern(problemId, pattern.id, relationType);
              await load();
            } catch (error) { showToast(String(error), "error"); }
          }}
        />
      )}
    </section>
  );
}
