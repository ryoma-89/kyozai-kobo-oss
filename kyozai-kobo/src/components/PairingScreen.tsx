import { useState } from "react";
import { authPair } from "../transport";

/** Web版の初回ペアリング画面。PC側に表示される8桁コードを入力する */
export function PairingScreen({ onPaired }: { onPaired: () => void }) {
  const [code, setCode] = useState("");
  const [deviceName, setDeviceName] = useState(defaultDeviceName());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await authPair(code.trim(), deviceName.trim());
      onPaired();
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center p-4" style={{ background: "var(--bg)" }}>
      <div
        className="fade-in w-full max-w-sm rounded-md border p-6 shadow-2xl"
        style={{ background: "var(--panel)", borderColor: "var(--border-strong)" }}
      >
        <h1 className="mb-1 text-lg font-bold">
          <span className="brand-mark">◆</span> 教材工房
        </h1>
        <p className="mb-5 text-xs" style={{ color: "var(--muted)" }}>
          この端末を教材サーバーへ登録します。
          Windowsアプリの「設定 → 教材サーバー」に表示されているペアリングコードを入力してください。
        </p>
        <label className="section-label mb-1 block">ペアリングコード（8桁）</label>
        <input
          value={code}
          onChange={(e) => setCode(e.target.value.replace(/[^0-9]/g, "").slice(0, 8))}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
          }}
          inputMode="numeric"
          autoComplete="one-time-code"
          className="input mb-4 w-full text-center font-mono text-2xl tracking-[0.4em]"
          placeholder="00000000"
          autoFocus
        />
        <label className="section-label mb-1 block">この端末の名前</label>
        <input
          value={deviceName}
          onChange={(e) => setDeviceName(e.target.value)}
          className="input mb-4 w-full"
          placeholder="例: iPad"
        />
        {error && (
          <p className="mb-3 text-xs" style={{ color: "var(--danger)" }}>
            {error}
          </p>
        )}
        <button
          onClick={submit}
          disabled={busy || code.length !== 8}
          className="btn btn-solid w-full justify-center py-2"
        >
          {busy ? "接続中..." : "接続する"}
        </button>
        <p className="mt-4 text-[11px] leading-relaxed" style={{ color: "var(--muted)" }}>
          コードは接続のたびに使い捨てです。接続後はこの端末が承認済みとして記録され、
          PC側の設定画面からいつでも取り消せます。
        </p>
      </div>
    </div>
  );
}

function defaultDeviceName(): string {
  const ua = navigator.userAgent;
  if (/iPad/.test(ua) || (/Macintosh/.test(ua) && "ontouchend" in document)) return "iPad";
  if (/iPhone/.test(ua)) return "iPhone";
  if (/Android/.test(ua)) return "Android端末";
  if (/Windows/.test(ua)) return "Windows PC";
  if (/Macintosh/.test(ua)) return "Mac";
  return "ブラウザ端末";
}
