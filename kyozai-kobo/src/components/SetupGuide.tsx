import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { codexStatus, detectTex } from "../api";
import {
  CODEX_INSTALL_URL,
  CODEX_NPM_INSTALL_COMMAND,
  CODEX_WINDOWS_INSTALL_COMMAND,
  NODE_DOWNLOAD_URL,
  TEX_LIVE_WINDOWS_URL,
} from "../setupRequirements";
import { useApp } from "../store";
import type { CodexStatus, TexDetection } from "../types";

interface SetupGuideProps {
  onDone?: () => void;
  onOpenSettings?: () => void;
}

export function SetupGuide({ onDone, onOpenSettings }: SetupGuideProps) {
  const { showToast } = useApp();
  const [tex, setTex] = useState<TexDetection | null>(null);
  const [codex, setCodex] = useState<CodexStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState("");

  const runCheck = useCallback(async () => {
    setChecking(true);
    setCheckError("");
    const [texResult, codexResult] = await Promise.allSettled([detectTex(), codexStatus()]);
    if (texResult.status === "fulfilled") setTex(texResult.value);
    if (codexResult.status === "fulfilled") setCodex(codexResult.value);
    if (texResult.status === "rejected" || codexResult.status === "rejected") {
      setCheckError("一部の環境を確認できませんでした。設定画面から個別に再確認できます。");
    }
    setChecking(false);
  }, []);

  useEffect(() => {
    void runCheck();
  }, [runCheck]);

  const openOfficialPage = async (url: string) => {
    try {
      await openUrl(url);
    } catch (error) {
      showToast(`公式ページを開けませんでした: ${String(error)}`, "error");
    }
  };

  const copyCommand = async (command: string) => {
    try {
      await navigator.clipboard.writeText(command);
      showToast("インストールコマンドをコピーしました");
    } catch {
      showToast("コピーできませんでした。表示されているコマンドを選択してコピーしてください。", "error");
    }
  };

  const texReady = !!tex?.uplatex_path && !!tex?.dvipdfmx_path;
  const texPartlyReady = !!tex?.uplatex_path || !!tex?.dvipdfmx_path;
  const codexReady = !!codex?.installed;
  const codexConnected = !!codex?.account?.account;

  return (
    <div className="space-y-4 text-sm">
      <div
        className="rounded border p-3 text-xs leading-relaxed"
        style={{ borderColor: "var(--border)", background: "var(--panel-2)", color: "var(--muted)" }}
      >
        問題バンク、教材編集、LaTeXソースの出力は追加ソフトなしで使えます。
        PDF生成にはTeX Live、AI変換・解答・解説生成にはOpenAI Codex CLIとChatGPTへの接続が必要です。
        教材工房が外部ソフトを無断でインストールすることはありません。
      </div>

      <section className="card space-y-2 p-3">
        <div className="flex flex-wrap items-center gap-2">
          <b>1. PDF生成 — TeX Live</b>
          {checking && tex === null ? (
            <span className="badge badge-muted">確認中</span>
          ) : texReady ? (
            <span className="badge badge-basic">✓ 使用可能</span>
          ) : texPartlyReady ? (
            <span className="badge" style={{ color: "var(--warn)" }}>一部不足</span>
          ) : (
            <span className="badge" style={{ color: "var(--warn)" }}>未検出</span>
          )}
        </div>
        <p className="text-xs" style={{ color: "var(--muted)" }}>
          PDFの作成には <code>uplatex</code> と <code>dvipdfmx</code> を使用します。
          未導入の場合はTeX LiveのWindows用インストーラーで導入し、完了後に「再確認」を押してください。
        </p>
        {(tex?.uplatex_path || tex?.dvipdfmx_path) && (
          <div className="space-y-1 rounded p-2 font-mono text-[11px]" style={{ background: "var(--panel-3)" }}>
            <div className="break-all">uplatex: {tex.uplatex_path ?? "未検出"}</div>
            <div className="break-all">dvipdfmx: {tex.dvipdfmx_path ?? "未検出"}</div>
          </div>
        )}
        <button type="button" className="btn btn-outline btn-sm" onClick={() => void openOfficialPage(TEX_LIVE_WINDOWS_URL)}>
          TeX Live公式の導入案内
        </button>
      </section>

      <section className="card space-y-2 p-3">
        <div className="flex flex-wrap items-center gap-2">
          <b>2. AI機能 — OpenAI Codex CLI</b>
          {checking && codex === null ? (
            <span className="badge badge-muted">確認中</span>
          ) : codexConnected ? (
            <span className="badge badge-basic">✓ 導入・接続済み</span>
          ) : codexReady ? (
            <span className="badge badge-muted">導入済み・ログインを確認</span>
          ) : (
            <span className="badge" style={{ color: "var(--warn)" }}>未検出</span>
          )}
          {codex?.version && <span className="text-xs" style={{ color: "var(--muted)" }}>{codex.version}</span>}
        </div>
        <p className="text-xs" style={{ color: "var(--muted)" }}>
          Windowsでは、次の公式インストールコマンドをPowerShellで実行する方法を案内します。
          コマンドは確認のため表示するだけで、教材工房から自動実行しません。
        </p>
        <div className="flex min-w-0 gap-2">
          <code
            className="min-w-0 flex-1 select-all overflow-x-auto rounded px-2 py-1.5 text-[11px]"
            style={{ background: "var(--panel-3)" }}
          >
            {CODEX_WINDOWS_INSTALL_COMMAND}
          </code>
          <button type="button" className="btn btn-ghost btn-sm shrink-0" onClick={() => void copyCommand(CODEX_WINDOWS_INSTALL_COMMAND)}>
            コピー
          </button>
        </div>
        <p className="text-xs" style={{ color: "var(--muted)" }}>
          npm版を使う場合だけNode.jsが必要です。
        </p>
        <div className="flex min-w-0 gap-2">
          <code
            className="min-w-0 flex-1 select-all overflow-x-auto rounded px-2 py-1.5 text-[11px]"
            style={{ background: "var(--panel-3)" }}
          >
            {CODEX_NPM_INSTALL_COMMAND}
          </code>
          <button type="button" className="btn btn-ghost btn-sm shrink-0" onClick={() => void copyCommand(CODEX_NPM_INSTALL_COMMAND)}>
            コピー
          </button>
        </div>
        {codex?.exePath && (
          <div className="break-all rounded p-2 font-mono text-[11px]" style={{ background: "var(--panel-3)" }}>
            Codex: {codex.exePath}
          </div>
        )}
        <div className="flex flex-wrap gap-2">
          <button type="button" className="btn btn-outline btn-sm" onClick={() => void openOfficialPage(CODEX_INSTALL_URL)}>
            Codex公式の導入案内
          </button>
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => void openOfficialPage(NODE_DOWNLOAD_URL)}>
            Node.js公式（npm版用）
          </button>
        </div>
      </section>

      <section className="card space-y-2 p-3">
        <b>3. 導入後の設定</b>
        <ol className="list-decimal space-y-1 pl-5 text-xs" style={{ color: "var(--muted)" }}>
          <li>下の「再確認」でTeX LiveとCodexが検出されることを確認します。</li>
          <li>設定の「Codex / ChatGPT接続」でログインし、「接続テスト」を実行します。</li>
          <li>必要ならAIモデル、PDF保存先、iPhone用の教材サーバーを設定します。</li>
        </ol>
      </section>

      {checkError && <p className="text-xs" style={{ color: "var(--warn)" }}>{checkError}</p>}

      <div className="flex flex-wrap items-center justify-end gap-2">
        <button type="button" className="btn btn-outline" disabled={checking} onClick={() => void runCheck()}>
          {checking ? "確認中..." : "環境を再確認"}
        </button>
        {onDone && (
          <button type="button" className="btn btn-ghost" onClick={onDone}>
            あとで
          </button>
        )}
        {onOpenSettings && (
          <button type="button" className="btn btn-solid" onClick={onOpenSettings}>
            設定を開く
          </button>
        )}
      </div>
    </div>
  );
}
