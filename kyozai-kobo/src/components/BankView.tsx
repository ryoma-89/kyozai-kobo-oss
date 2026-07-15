import { useEffect, useState } from "react";
import { useApp } from "../store";
import { ProblemEditor } from "./ProblemEditor";
import { ProblemList } from "./ProblemList";
import { TreePanel } from "./TreePanel";

/** 画面幅がしきい値未満かどうか（iPad縦・スマートフォン向けの1ペイン表示判定） */
export function useNarrowLayout(threshold = 900): boolean {
  const [narrow, setNarrow] = useState(() => window.innerWidth < threshold);
  useEffect(() => {
    const onResize = () => setNarrow(window.innerWidth < threshold);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [threshold]);
  return narrow;
}

/** 問題バンク画面: 左ツリー + 中央一覧/編集 + 右プレビュー
 *  狭い画面では「単元選択 → 問題一覧・編集」の1ペイン切り替え式になる */
export function BankView() {
  const { selectedProblemId, selectedUnitId, selectUnit, setContextName } = useApp();
  const narrow = useNarrowLayout();

  useEffect(() => {
    setContextName("問題バンク");
    return () => setContextName("");
  }, []);

  if (narrow) {
    // 1ペイン: 単元未選択ならツリー、選択済みなら一覧/編集（戻るボタン付き）
    return (
      <div className="flex h-full min-w-0 flex-col">
        {selectedUnitId == null ? (
          <div className="min-h-0 flex-1" style={{ background: "var(--panel)" }}>
            <TreePanel />
          </div>
        ) : (
          <>
            {selectedProblemId == null && (
              <div className="border-b px-2 py-1" style={{ borderColor: "var(--border)" }}>
                <button onClick={() => selectUnit(null)} className="btn btn-ghost btn-sm">
                  ← 単元一覧
                </button>
              </div>
            )}
            <div className="min-h-0 min-w-0 flex-1">
              {selectedProblemId != null ? <ProblemEditor /> : <ProblemList />}
            </div>
          </>
        )}
      </div>
    );
  }

  return (
    <div className="flex h-full min-w-0">
      <div
        className="w-64 shrink-0 border-r"
        style={{ background: "var(--panel)", borderColor: "var(--border)" }}
      >
        <TreePanel />
      </div>
      <div className="min-w-0 flex-1">
        {selectedProblemId != null ? <ProblemEditor /> : <ProblemList />}
      </div>
    </div>
  );
}
