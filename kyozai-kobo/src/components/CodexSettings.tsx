import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  codexLoginCancel,
  codexLoginStart,
  codexLogout,
  codexModels,
  codexSetModel,
  codexSetPath,
  codexStatus,
  codexTest,
} from "../api";
import { useApp } from "../store";
import { isTauri } from "../transport";
import type { CodexModelSettings, CodexStatus } from "../types";
import {
  CODEX_INSTALL_URL,
  CODEX_NPM_INSTALL_COMMAND,
  CODEX_WINDOWS_INSTALL_COMMAND,
  NODE_DOWNLOAD_URL,
} from "../setupRequirements";

/** Codex / ChatGPT接続設定（デスクトップ・Web共通） */
export function CodexSettings() {
  const { showToast, confirm, bumps } = useApp();
  const [status, setStatus] = useState<CodexStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [pathInput, setPathInput] = useState("");
  const [showLog, setShowLog] = useState(false);
  const [modelSettings, setModelSettings] = useState<CodexModelSettings | null>(null);
  const [modelError, setModelError] = useState("");

  const loadModels = async () => {
    try {
      const next = await codexModels();
      setModelSettings(next);
      setModelError("");
    } catch (e) {
      setModelError(String(e));
    }
  };

  const load = async (): Promise<CodexStatus | null> => {
    try {
      const s = await codexStatus();
      setStatus(s);
      setPathInput(s.exePath);
      return s;
    } catch (e) {
      showToast(String(e), "error");
      return null;
    }
  };

  useEffect(() => {
    void (async () => {
      const nextStatus = await load();
      if (!nextStatus?.installed) return;
      await loadModels();
      await load();
    })();
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
  const selectedModel = modelSettings?.selectedModel ?? status.selectedModel ?? "";
  const selectedModelInfo = modelSettings?.models.find((item) => item.model === selectedModel);

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
          <div className="space-y-2" style={{ color: "var(--muted)" }}>
            <p>
              OpenAIのCodex CLIが見つかりません。Windowsでは、PC上のPowerShellで次の公式インストールコマンドを実行できます。
            </p>
            <code className="block select-all overflow-x-auto rounded px-2 py-1.5" style={{ background: "var(--panel-3)" }}>
              {CODEX_WINDOWS_INSTALL_COMMAND}
            </code>
            <p>
              npm版を使う場合は <code>{CODEX_NPM_INSTALL_COMMAND}</code> を実行します。この方法だけNode.jsが必要です。
            </p>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="btn btn-outline btn-sm"
                onClick={() => void openUrl(CODEX_INSTALL_URL).catch((error) => showToast(String(error), "error"))}
              >
                Codex公式の導入案内
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void openUrl(NODE_DOWNLOAD_URL).catch((error) => showToast(String(error), "error"))}
              >
                Node.js公式（npm版用）
              </button>
            </div>
          </div>
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

      <div className="card space-y-2 p-3">
        <div className="flex flex-col items-stretch gap-2 sm:flex-row sm:items-end">
          <div className="min-w-0 flex-1">
            <label className="section-label mb-1 block">AI生成に使用するモデル</label>
            <select
              value={selectedModel}
              onChange={(e) => {
                const model = e.target.value;
                setModelSettings((current) => ({
                  selectedModel: model,
                  models: current?.models ?? [],
                }));
                void withBusy(async () => {
                  await codexSetModel(model);
                  showToast(model ? `AIモデルを ${model} に変更しました` : "AIモデルをCodexの既定に戻しました");
                });
              }}
              disabled={busy}
              className="select w-full"
            >
              <option value="">Codexの既定モデル</option>
              {selectedModel && !modelSettings?.models.some((item) => item.model === selectedModel) && (
                <option value={selectedModel}>{selectedModel}</option>
              )}
              {modelSettings?.models.map((item) => (
                <option key={item.model} value={item.model}>
                  {item.displayName}{item.isDefault ? "（既定）" : ""}
                </option>
              ))}
            </select>
          </div>
          <button
            onClick={() => withBusy(loadModels)}
            disabled={busy || !status.installed}
            className="btn btn-ghost btn-sm w-full shrink-0 sm:w-auto"
          >
            一覧を更新
          </button>
        </div>
        {selectedModelInfo?.description && (
          <p style={{ color: "var(--muted)" }}>{selectedModelInfo.description}</p>
        )}
        {modelError && (
          <p style={{ color: "var(--warn)" }}>
            モデル一覧を取得できませんでした。Codexへ接続してから「一覧を更新」を押してください。
          </p>
        )}
        <p style={{ color: "var(--muted)" }}>
          次に開始する解答・解説・AI変換・グラフ生成から適用されます。未選択の場合はCodex側の既定モデルを使用します。
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
