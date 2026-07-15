import { useEffect, useState } from "react";
import {
  codexLoginCancel,
  codexLoginStart,
  codexLogout,
  codexSetPath,
  codexStatus,
  codexTest,
} from "../api";
import { useApp } from "../store";
import { isTauri } from "../transport";
import type { CodexStatus } from "../types";

/** Codex / ChatGPT接続設定（デスクトップ・Web共通） */
export function CodexSettings() {
  const { showToast, confirm, bumps } = useApp();
  const [status, setStatus] = useState<CodexStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [pathInput, setPathInput] = useState("");
  const [showLog, setShowLog] = useState(false);

  const load = async () => {
    try {
      const s = await codexStatus();
      setStatus(s);
      setPathInput(s.exePath);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  useEffect(() => {
    load();
  }, [bumps.codex]);

  // ログイン待ちの間はポーリング
  useEffect(() => {
    if (status?.login?.status !== "pending") return;
    const timer = setInterval(load, 3000);
    return () => clearInterval(timer);
  }, [status?.login?.status]);

  const withBusy = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      await load();
    } catch (e) {
      showToast(String(e), "error");
      await load().catch(() => {});
    } finally {
      setBusy(false);
    }
  };

  if (!status) {
    return (
      <p className="text-xs" style={{ color: "var(--muted)" }}>
        読み込み中...
      </p>
    );
  }

  const account = status.account?.account;
  const authenticated = !!account;
  const login = status.login;

  return (
    <div className="space-y-3 text-xs">
      <div className="card space-y-2 p-3">
        <div className="flex flex-wrap items-center gap-2">
          {!status.installed ? (
            <span className="badge" style={{ color: "var(--warn)", borderColor: "rgba(251,191,36,0.4)" }}>
              Codex未検出
            </span>
          ) : authenticated ? (
            <span className="badge" style={{ color: "var(--success)", borderColor: "rgba(197,183,223,0.4)", background: "var(--success-dim)" }}>
              ● 接続済み
            </span>
          ) : (
            <span className="badge badge-muted">未ログイン</span>
          )}
          {status.version && <span style={{ color: "var(--muted)" }}>{status.version}</span>}
          {status.running && <span className="badge badge-muted">app-server稼働中</span>}
        </div>

        {!status.installed && (
          <p style={{ color: "var(--muted)" }}>
            OpenAIのCodex CLIが見つかりません。PC上のPowerShellで
            <code className="mx-1 rounded px-1" style={{ background: "var(--panel-3)" }}>
              npm install -g @openai/codex
            </code>
            を実行してインストールしてください（Node.jsが必要）。
          </p>
        )}

        {authenticated && (
          <p>
            アカウント: <b>{account?.email ?? "(メール不明)"}</b>
            {account?.planType && (
              <span className="badge badge-muted ml-1.5">ChatGPT {account.planType}</span>
            )}
            <span className="ml-1.5" style={{ color: "var(--muted)" }}>
              認証方式: {account?.type === "chatgpt" ? "ChatGPTログイン" : account?.type}
            </span>
          </p>
        )}

        {/* デバイスコード認証の進行中表示 */}
        {login?.status === "pending" && (
          <div className="rounded border p-3" style={{ borderColor: "rgba(157,108,242,0.42)", background: "var(--accent-dim)" }}>
            {login.userCode ? (
              <>
                <p className="mb-1 font-semibold">ChatGPTデバイスコード認証</p>
                <p>
                  1.{" "}
                  <a
                    href={login.verificationUrl ?? "#"}
                    target="_blank"
                    rel="noreferrer"
                    className="underline"
                    style={{ color: "var(--accent)" }}
                  >
                    {login.verificationUrl}
                  </a>{" "}
                  を開く
                </p>
                <p className="my-1">
                  2. コード{" "}
                  <code className="rounded px-2 py-0.5 text-lg font-bold tracking-widest" style={{ background: "var(--panel)", color: "var(--accent)" }}>
                    {login.userCode}
                  </code>{" "}
                  を入力してChatGPTでログイン
                </p>
                <p style={{ color: "var(--muted)" }}>完了するとこの画面が自動的に更新されます…</p>
              </>
            ) : (
              <p>
                ブラウザでログインを続行してください:{" "}
                <a href={login.authUrl ?? "#"} target="_blank" rel="noreferrer" className="underline" style={{ color: "var(--accent)" }}>
                  認証ページを開く
                </a>
              </p>
            )}
            <button onClick={() => withBusy(() => codexLoginCancel())} className="btn btn-ghost btn-sm mt-2">
              ログインを中止
            </button>
          </div>
        )}
        {login?.status === "failed" && (
          <p style={{ color: "var(--danger)" }}>ログインに失敗しました: {login.error ?? "不明なエラー"}</p>
        )}

        <div className="flex flex-wrap gap-1.5">
          {!authenticated && login?.status !== "pending" && (
            <>
              <button
                onClick={() => withBusy(() => codexLoginStart("deviceCode"))}
                disabled={busy || !status.installed}
                className="btn btn-solid btn-sm"
                title="iPad等からでも使える推奨方式"
              >
                ChatGPTに接続（デバイスコード）
              </button>
              {isTauri && (
                <button
                  onClick={() => withBusy(() => codexLoginStart("browser"))}
                  disabled={busy || !status.installed}
                  className="btn btn-outline btn-sm"
                  title="このPCのブラウザでログイン"
                >
                  ブラウザでログイン
                </button>
              )}
            </>
          )}
          {authenticated && (
            <button
              onClick={async () => {
                if (await confirm("ChatGPTからログアウトしますか？")) {
                  await withBusy(() => codexLogout());
                }
              }}
              disabled={busy}
              className="btn btn-ghost btn-sm"
            >
              ログアウト
            </button>
          )}
          <button
            onClick={() =>
              withBusy(async () => {
                await codexTest();
                showToast("Codexとの接続に成功しました");
              })
            }
            disabled={busy || !status.installed}
            className="btn btn-ghost btn-sm"
          >
            接続テスト
          </button>
        </div>

        {status.lastError && (
          <p style={{ color: "var(--danger)" }}>最終エラー: {status.lastError}</p>
        )}
        <p style={{ color: "var(--muted)" }}>
          認証情報はCodex CLI（PC側）が管理します。このアプリやブラウザにChatGPTのパスワード・トークンが保存されることはありません。
          APIキー方式は将来の選択肢として、現在はChatGPTログインのみ対応しています。
        </p>
      </div>

      {/* 実行ファイルパス（デスクトップのみ変更可能） */}
      {isTauri && (
        <div className="card space-y-1.5 p-3">
          <label className="section-label block">Codex実行ファイル（空欄で自動検出）</label>
          <div className="flex gap-1.5">
            <input
              value={pathInput}
              onChange={(e) => setPathInput(e.target.value)}
              className="input flex-1 font-mono text-xs"
              placeholder="例: C:\\Users\\...\\codex.exe"
            />
            <button
              onClick={() =>
                withBusy(async () => {
                  await codexSetPath(pathInput.trim());
                  showToast("保存しました");
                })
              }
              className="btn btn-ghost btn-sm"
            >
              保存
            </button>
          </div>
        </div>
      )}

      <div className="card p-3">
        <button onClick={() => setShowLog(!showLog)} className="btn btn-ghost btn-sm">
          {showLog ? "Codexログを隠す" : "Codexログを表示"}
        </button>
        {showLog && (
          <pre className="mt-2 max-h-56 overflow-auto rounded p-2 text-[10px] whitespace-pre-wrap" style={{ background: "var(--panel-2)" }}>
            {status.log.length > 0 ? status.log.join("\n") : "(ログなし)"}
          </pre>
        )}
      </div>
    </div>
  );
}
