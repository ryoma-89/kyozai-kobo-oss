import { useState } from "react";
import { Modal } from "./ui";

export interface ConflictField {
  label: string;
  mine: string;
  server: string;
}

type DiffLine = { kind: "same" | "add" | "del"; text: string };

/** 行単位の簡易LCS差分（LaTeX本文の見比べ用） */
function diffLines(a: string, b: string): DiffLine[] {
  const al = a.split("\n");
  const bl = b.split("\n");
  // サイズ上限（巨大な本文はそのまま並べる）
  if (al.length * bl.length > 400_000) {
    return [
      ...al.map((t) => ({ kind: "del", text: t }) as DiffLine),
      ...bl.map((t) => ({ kind: "add", text: t }) as DiffLine),
    ];
  }
  const n = al.length;
  const m = bl.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = al[i] === bl[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (al[i] === bl[j]) {
      out.push({ kind: "same", text: al[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: "del", text: al[i] });
      i++;
    } else {
      out.push({ kind: "add", text: bl[j] });
      j++;
    }
  }
  while (i < n) out.push({ kind: "del", text: al[i++] });
  while (j < m) out.push({ kind: "add", text: bl[j++] });
  return out;
}

/**
 * 同時編集の競合解決ダイアログ。
 * 「自分の変更 → del(赤) / サーバー版 → add(緑)」の差分を表示し、解決方法を選ばせる。
 */
export function ConflictDialog({
  title,
  fields,
  onResolve,
  onClose,
}: {
  title: string;
  fields: ConflictField[];
  /** "server": サーバー版を採用 / "mine": 自分の変更で上書き / "copy": コピーとして両方残す */
  onResolve: (choice: "server" | "mine" | "copy") => void;
  onClose: () => void;
}) {
  const [showDiff, setShowDiff] = useState(true);
  const changed = fields.filter((f) => f.mine !== f.server);

  return (
    <Modal title={`競合: ${title}`} onClose={onClose} wide>
      <div className="space-y-3 text-xs">
        <p>
          他の端末（またはウィンドウ）で先に保存されています。
          <b>そのまま上書きすると相手の変更が失われます。</b>どう解決するか選んでください。
        </p>

        <div className="flex flex-wrap gap-2">
          <button onClick={() => onResolve("server")} className="btn btn-outline btn-sm">
            サーバー版を採用（自分の変更を破棄）
          </button>
          <button onClick={() => onResolve("mine")} className="btn btn-sm" style={{ background: "var(--warn)", color: "#1c1917", fontWeight: 700 }}>
            自分の変更で上書き
          </button>
          <button onClick={() => onResolve("copy")} className="btn btn-outline btn-sm">
            自分の変更をコピーとして保存（両方残す）
          </button>
          <button onClick={() => setShowDiff(!showDiff)} className="btn btn-ghost btn-sm">
            {showDiff ? "差分を隠す" : "差分を確認"}
          </button>
        </div>

        {showDiff && (
          <div className="max-h-[50vh] space-y-3 overflow-y-auto">
            {changed.length === 0 && (
              <p style={{ color: "var(--muted)" }}>本文の差分はありません（タグ・メタ情報の変更の可能性）。</p>
            )}
            {changed.map((f) => (
              <div key={f.label}>
                <p className="section-label mb-1">
                  {f.label}
                  <span className="ml-2 font-normal" style={{ color: "var(--danger)" }}>
                    − 自分の変更
                  </span>
                  <span className="ml-2 font-normal" style={{ color: "var(--success)" }}>
                    ＋ サーバー版
                  </span>
                </p>
                <pre className="max-h-64 overflow-auto rounded p-2 font-mono text-[11px] leading-relaxed" style={{ background: "var(--panel-2)" }}>
                  {diffLines(f.mine, f.server).map((l, i) => (
                    <div
                      key={i}
                      style={
                        l.kind === "del"
                          ? { background: "var(--danger-dim)", color: "var(--danger)" }
                          : l.kind === "add"
                            ? { background: "var(--success-dim)", color: "var(--success)" }
                            : undefined
                      }
                    >
                      {l.kind === "del" ? "− " : l.kind === "add" ? "＋ " : "　 "}
                      {l.text || " "}
                    </div>
                  ))}
                </pre>
              </div>
            ))}
          </div>
        )}
      </div>
    </Modal>
  );
}
