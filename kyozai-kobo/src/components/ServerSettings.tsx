import { useEffect, useState } from "react";
import QRCode from "qrcode";
import {
  autostartGet,
  autostartSet,
  backupNow,
  listBackups,
  restoreBackup,
  revokeWebDevice,
  serverRegenPairing,
  serverSettingsGet,
  serverSettingsSet,
  serverStart,
  serverStatus,
  serverStop,
  tailscaleStatus,
} from "../api";
import { useApp } from "../store";
import type { ServerStatus, TailscaleStatus } from "../types";
import { Icon } from "./Icon";

/** 教材サーバー管理（デスクトップ専用セクション） */
export function ServerSettings() {
  const { showToast, confirm, bumps } = useApp();
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [settings, setSettingsState] = useState<{ port: number; lanMode: boolean; serverAutostart: boolean } | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [ts, setTs] = useState<TailscaleStatus | null>(null);
  const [qrTarget, setQrTarget] = useState<"tailscale" | "local">("tailscale");
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showLog, setShowLog] = useState(false);
  const [backups, setBackups] = useState<{ fileName: string; sizeBytes: number; modified: string }[] | null>(null);

  const load = async () => {
    try {
      const [st, se, au] = await Promise.all([serverStatus(), serverSettingsGet(), autostartGet().catch(() => false)]);
      setStatus(st);
      setSettingsState(se);
      setAutostart(au);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  useEffect(() => {
    load();
    tailscaleStatus().then(setTs).catch(() => {});
  }, [bumps.server]);

  // QRコード生成
  useEffect(() => {
    const url =
      qrTarget === "tailscale" && ts?.httpsUrl ? ts.httpsUrl : status ? status.localUrl : null;
    if (!url) {
      setQrDataUrl(null);
      return;
    }
    QRCode.toDataURL(url, { width: 220, margin: 1, color: { dark: "#0a0e15", light: "#ffffff" } })
      .then(setQrDataUrl)
      .catch(() => setQrDataUrl(null));
  }, [qrTarget, ts?.httpsUrl, status?.localUrl, status?.running]);

  const withBusy = async (fn: () => Promise<unknown>, doneMsg?: string) => {
    setBusy(true);
    try {
      await fn();
      if (doneMsg) showToast(doneMsg);
      await load();
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      showToast("コピーしました");
    } catch {
      showToast("コピーに失敗しました", "error");
    }
  };

  const saveSettings = async () => {
    if (!settings) return;
    if (settings.lanMode) {
      const ok = await confirm(
        "LAN直接アクセスモードを有効にすると、同じネットワーク上の全端末がこのサーバーへ到達できるようになります。\n" +
          "ペアリング認証は必要ですが、Tailscale Serve（推奨）より露出が増えます。\n有効にしますか？",
      );
      if (!ok) {
        setSettingsState({ ...settings, lanMode: false });
        return;
      }
    }
    await withBusy(
      () => serverSettingsSet(settings),
      "サーバー設定を保存しました（ポート等は再起動後に反映されます）",
    );
  };

  const onRestore = async (fileName: string) => {
    if (
      !(await confirm(
        `バックアップ「${fileName}」からデータベースを復元しますか？\n\n現在のデータは pre-restore として退避されますが、` +
          `復元後は全画面の内容が置き換わります。`,
      ))
    )
      return;
    await withBusy(async () => {
      await restoreBackup(fileName);
      setBackups(await listBackups());
    }, "復元しました");
  };

  if (!status || !settings) {
    return (
      <p className="text-xs" style={{ color: "var(--muted)" }}>
        読み込み中...
      </p>
    );
  }

  const tsUrl = ts?.httpsUrl ?? "";

  return (
    <div className="space-y-4">
      {/* 状態と操作 */}
      <div className="card space-y-3 p-3">
        <div className="flex flex-wrap items-center gap-2">
          <span
            className="badge"
            style={
              status.running
                ? { color: "var(--success)", borderColor: "rgba(197,183,223,0.4)", background: "var(--success-dim)" }
                : undefined
            }
          >
            {status.running ? "● 稼働中" : "○ 停止中"}
          </span>
          <span className="font-mono text-xs">ポート {status.port}</span>
          <span className="text-xs" style={{ color: "var(--muted)" }}>
            接続中セッション: {status.activeSessions}
          </span>
          <span className="ml-auto flex gap-1.5">
            {status.running ? (
              <>
                <button onClick={() => withBusy(() => serverStop(), "停止しました")} disabled={busy} className="btn btn-ghost btn-sm">
                  停止
                </button>
                <button
                  onClick={() => withBusy(async () => { await serverStop(); await serverStart(); }, "再起動しました")}
                  disabled={busy}
                  className="btn btn-outline btn-sm"
                >
                  再起動
                </button>
              </>
            ) : (
              <button onClick={() => withBusy(() => serverStart(), "起動しました")} disabled={busy} className="btn btn-solid btn-sm">
                <Icon name="play" size={15} /> サーバーを起動
              </button>
            )}
          </span>
        </div>

        {status.running && (
          <div className="grid gap-3 md:grid-cols-[1fr_auto]">
            <div className="space-y-2 text-xs">
              <div className="flex items-center gap-1.5">
                <span className="section-label w-24 shrink-0">ローカルURL</span>
                <code className="truncate">{status.localUrl}</code>
                <button onClick={() => copy(status.localUrl)} className="btn btn-ghost btn-sm">コピー</button>
              </div>
              <div className="flex items-center gap-1.5">
                <span className="section-label w-24 shrink-0">Tailscale URL</span>
                {tsUrl ? (
                  <>
                    <code className="truncate">{tsUrl}</code>
                    <button onClick={() => copy(tsUrl)} className="btn btn-ghost btn-sm">コピー</button>
                  </>
                ) : (
                  <span style={{ color: "var(--muted)" }}>（Tailscale未接続）</span>
                )}
              </div>
              <div className="flex items-center gap-1.5">
                <span className="section-label w-24 shrink-0">ペアリングコード</span>
                <code className="text-base font-bold tracking-widest" style={{ color: "var(--accent)" }}>
                  {status.pairingCode ?? "--------"}
                </code>
                <button onClick={() => withBusy(() => serverRegenPairing())} className="btn btn-ghost btn-sm" title="コードを再発行">
                  ⟳
                </button>
              </div>
              <p style={{ color: "var(--muted)" }}>
                iPad等のブラウザでURLを開き、このコードを入力すると接続できます。コードは1回使うと自動的に更新されます。
              </p>
            </div>
            <div className="text-center">
              {qrDataUrl ? (
                <img src={qrDataUrl} alt="接続用QRコード" className="mx-auto rounded" style={{ background: "#fff", padding: 4 }} />
              ) : (
                <p className="text-xs" style={{ color: "var(--muted)" }}>QRを生成できません</p>
              )}
              <div className="mt-1 flex justify-center gap-1">
                <button
                  onClick={() => setQrTarget("tailscale")}
                  className={`btn btn-sm ${qrTarget === "tailscale" ? "btn-outline" : "btn-ghost"}`}
                  disabled={!tsUrl}
                >
                  Tailscale
                </button>
                <button
                  onClick={() => setQrTarget("local")}
                  className={`btn btn-sm ${qrTarget === "local" ? "btn-outline" : "btn-ghost"}`}
                >
                  ローカル
                </button>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Tailscale */}
      <div className="card space-y-2 p-3 text-xs">
        <p className="section-label">Tailscale Serve（推奨の外部アクセス方式）</p>
        {!ts ? (
          <p style={{ color: "var(--muted)" }}>確認中...</p>
        ) : !ts.installed ? (
          <p style={{ color: "var(--warn)" }}>{ts.message}</p>
        ) : (
          <>
            <p>
              {ts.version}　状態:{" "}
              <span style={{ color: ts.connected ? "var(--success)" : "var(--warn)" }}>
                {ts.backendState}
              </span>
              　Serve設定: {ts.serveConfigured ? "済（このポートへ転送中）" : "未設定"}
            </p>
            {!ts.serveConfigured && (
              <div className="flex items-center gap-1.5">
                <span style={{ color: "var(--muted)" }}>PowerShellで実行:</span>
                <code className="rounded px-1.5 py-0.5" style={{ background: "var(--panel-3)" }}>
                  {ts.suggestedCommand}
                </code>
                <button onClick={() => copy(ts.suggestedCommand ?? "")} className="btn btn-ghost btn-sm">
                  コピー
                </button>
              </div>
            )}
            {ts.serveStatus && (
              <pre className="max-h-24 overflow-auto rounded p-1.5 text-[10px]" style={{ background: "var(--panel-2)" }}>
                {ts.serveStatus}
              </pre>
            )}
            <p style={{ color: "var(--muted)" }}>
              Tailscale Serveはtailnet内の自分の端末だけへHTTPSで公開します。教材サーバー自体は127.0.0.1のみで待ち受けます。
              一般公開（Funnel）は使用しないでください。
            </p>
          </>
        )}
      </div>

      {/* サーバー設定 */}
      <div className="card space-y-2 p-3 text-xs">
        <p className="section-label">サーバー設定</p>
        <div className="flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-1.5">
            ポート
            <input
              type="number"
              value={settings.port}
              min={1024}
              max={65535}
              onChange={(e) => setSettingsState({ ...settings, port: parseInt(e.target.value || "8760", 10) })}
              className="input w-24 font-mono"
            />
          </label>
          <label className="flex items-center gap-1.5">
            <input
              type="checkbox"
              checked={settings.serverAutostart}
              onChange={(e) => setSettingsState({ ...settings, serverAutostart: e.target.checked })}
            />
            アプリ起動時にサーバーを自動起動
          </label>
          <label className="flex items-center gap-1.5">
            <input
              type="checkbox"
              checked={autostart}
              onChange={async (e) => {
                try {
                  setAutostart(await autostartSet(e.target.checked));
                  showToast(e.target.checked ? "Windowsログイン時に自動起動します" : "自動起動を解除しました");
                } catch (err) {
                  showToast(String(err), "error");
                }
              }}
            />
            Windowsログイン時にアプリを自動起動
          </label>
          <label className="flex items-center gap-1.5" title="標準は127.0.0.1のみ。有効化するとLAN内の他端末から直接アクセス可能になります">
            <input
              type="checkbox"
              checked={settings.lanMode}
              onChange={(e) => setSettingsState({ ...settings, lanMode: e.target.checked })}
            />
            LAN直接アクセス（非推奨・既定オフ）
          </label>
          <button onClick={saveSettings} disabled={busy} className="btn btn-outline btn-sm">
            サーバー設定を保存
          </button>
        </div>
        <p style={{ color: "var(--muted)" }}>
          ※PCがスリープすると接続できなくなります。iPadから使う間は、Windowsの電源設定でスリープを「なし」にするか、電源に接続してください。
        </p>
      </div>

      {/* 承認済み端末 */}
      <div className="card space-y-2 p-3 text-xs">
        <p className="section-label">承認済み端末</p>
        {status.devices.length === 0 ? (
          <p style={{ color: "var(--muted)" }}>まだ端末が登録されていません。</p>
        ) : (
          <ul className="space-y-1">
            {status.devices.map((d) => (
              <li key={d.id} className="flex items-center gap-2">
                <span className={d.revoked ? "line-through opacity-50" : ""}>
                  {d.deviceName}
                </span>
                <span style={{ color: "var(--muted)" }}>最終アクセス: {d.lastSeenAt || "-"}</span>
                {!d.revoked && (
                  <button
                    onClick={async () => {
                      if (await confirm(`「${d.deviceName}」のアクセスを取り消しますか？`)) {
                        await revokeWebDevice(d.id).catch((e) => showToast(String(e), "error"));
                        await load();
                      }
                    }}
                    className="btn btn-danger btn-sm ml-auto"
                  >
                    取り消し
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* バックアップ */}
      <div className="card space-y-2 p-3 text-xs">
        <div className="flex items-center gap-2">
          <p className="section-label">バックアップ</p>
          <button
            onClick={() =>
              withBusy(async () => {
                const r = await backupNow();
                setBackups(await listBackups());
                showToast(`バックアップを作成しました: ${r.dbFile}`);
              })
            }
            disabled={busy}
            className="btn btn-outline btn-sm"
          >
            今すぐバックアップ
          </button>
          <button
            onClick={async () => setBackups(await listBackups().catch(() => []))}
            className="btn btn-ghost btn-sm"
          >
            世代一覧を表示
          </button>
        </div>
        <p style={{ color: "var(--muted)" }}>
          DBはSQLiteのオンラインバックアップAPIで安全にコピーされ（サーバー停止不要）、
          添付・テンプレート・グラフのアセットもミラーコピーされます。起動時の日次バックアップも従来通り動作します。
        </p>
        {backups && (
          <ul className="max-h-48 space-y-1 overflow-auto">
            {backups.length === 0 && <li style={{ color: "var(--muted)" }}>バックアップがありません</li>}
            {backups.map((b) => (
              <li key={b.fileName} className="flex items-center gap-2">
                <code className="truncate">{b.fileName}</code>
                <span style={{ color: "var(--muted)" }}>
                  {(b.sizeBytes / 1024).toFixed(0)}KB / {b.modified}
                </span>
                <button onClick={() => onRestore(b.fileName)} className="btn btn-ghost btn-sm ml-auto">
                  この世代へ復元
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* ログ */}
      <div className="card p-3 text-xs">
        <button onClick={() => setShowLog(!showLog)} className="btn btn-ghost btn-sm">
          {showLog ? "サーバーログを隠す" : "サーバーログを表示"}
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
