/**
 * トランスポート層:
 * - デスクトップ（Tauri）では invoke("dispatch", {cmd, args})
 * - ブラウザでは POST /api/invoke/:cmd
 * どちらも同じサービス層（Rust側 dispatch）に到達する。
 */
import { convertFileSrc, invoke as tauriInvoke } from "@tauri-apps/api/core";

export const isTauri: boolean =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** ブラウザ版でのAPIベース（同一オリジン） */
const API_BASE = "";

export class ConflictError extends Error {
  serverVersion: number;
  constructor(serverVersion: number) {
    super(`CONFLICT:${serverVersion}`);
    this.serverVersion = serverVersion;
  }
}

export class AuthError extends Error {}

function toConflict(msg: string): Error {
  if (msg.startsWith("CONFLICT:")) {
    const v = parseInt(msg.slice("CONFLICT:".length), 10);
    return new ConflictError(Number.isFinite(v) ? v : -1);
  }
  return new Error(msg);
}

/** 全コマンド共通の呼び出し */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    try {
      return await tauriInvoke<T>("dispatch", { cmd, args: args ?? {} });
    } catch (e) {
      throw toConflict(String(e));
    }
  }
  const res = await fetch(`${API_BASE}/api/invoke/${encodeURIComponent(cmd)}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Requested-With": "kyozai-kobo",
    },
    credentials: "same-origin",
    body: JSON.stringify(args ?? {}),
  });
  if (res.status === 401) {
    // 未認証 → ペアリング画面へ
    window.dispatchEvent(new CustomEvent("kk-auth-required"));
    throw new AuthError("未認証です。ペアリングしてください");
  }
  const body = await res.json().catch(() => null);
  if (!res.ok) {
    const msg = (body && (body.error as string)) || `HTTPエラー ${res.status}`;
    throw toConflict(msg);
  }
  return body as T;
}

/** Tauri専用コマンド（open_path 等）。ブラウザでは呼ばない */
export async function invokeTauriOnly<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) throw new Error("この操作はデスクトップ版でのみ利用できます");
  return tauriInvoke<T>(cmd, args ?? {});
}

// ---- ファイルURL（PDFプレビュー・添付画像の表示） ----

/** コンパイル成果物（ビルドPDF等）の表示URL */
export function buildFileUrl(absPath: string, cacheBust?: string | number): string {
  if (isTauri) {
    const url = convertFileSrc(absPath);
    return cacheBust == null ? url : `${url}?t=${encodeURIComponent(String(cacheBust))}`;
  }
  const url = `${API_BASE}/api/files/build?path=${encodeURIComponent(absPath)}`;
  return cacheBust == null ? url : `${url}&t=${encodeURIComponent(String(cacheBust))}`;
}

/**
 * PDFプレビュー用の成果物URL。
 * デスクトップ: asset protocol（asset.localhost）へのfetchはクロスオリジンで
 * CORSに阻まれるため、IPCでバイト列を取得してblob URLを返す。
 * Web: 認証付き /api/files/build のURLを返す（従来どおり）。
 * 返り値がblob:のときは呼び出し側で不要時に URL.revokeObjectURL すること。
 */
export async function compiledPdfUrl(absPath: string, cacheBust?: string | number): Promise<string> {
  if (!isTauri) return buildFileUrl(absPath, cacheBust);
  const b64 = await invoke<string>("read_compiled_file", { path: absPath });
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
}

/** blob URLなら破棄する（それ以外は何もしない） */
export function revokeIfBlobUrl(url: string | null | undefined): void {
  if (url && url.startsWith("blob:")) URL.revokeObjectURL(url);
}

/** 問題添付画像の表示URL（Webのみ。デスクトップは従来通りファイル名参照） */
export function attachmentUrl(storedName: string): string {
  return `${API_BASE}/api/files/attachment/${encodeURIComponent(storedName)}`;
}

/** 認証済みグラフ派生ファイル。ローカル絶対パスをブラウザへ渡さない。 */
export function graphFileUrl(
  graphId: string,
  format: "thumbnail" | "pdf" | "png" | "svg" | "tex" | "json" | "zip",
  download = false,
): string {
  const base = `${API_BASE}/api/graphs/${encodeURIComponent(graphId)}/files/${format}`;
  return download ? `${base}?download=1` : base;
}

/** 生成されたPDF等を開く（デスクトップ: 既定アプリ / Web: 新しいタブ） */
export async function openCompiledFile(absPath: string): Promise<void> {
  if (isTauri) {
    await tauriInvoke<void>("open_path", { path: absPath });
  } else {
    window.open(buildFileUrl(absPath), "_blank");
  }
}

// ---- 認証（Web版のみ） ----

export async function authMe(): Promise<{ authenticated: boolean }> {
  const res = await fetch(`${API_BASE}/api/auth/me`, { credentials: "same-origin" });
  return res.json();
}

export async function authPair(code: string, deviceName: string): Promise<void> {
  const res = await fetch(`${API_BASE}/api/auth/pair`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Requested-With": "kyozai-kobo" },
    credentials: "same-origin",
    body: JSON.stringify({ code, deviceName }),
  });
  const body = await res.json().catch(() => null);
  if (!res.ok) {
    throw new Error((body && body.error) || "ペアリングに失敗しました");
  }
}

export async function authLogout(): Promise<void> {
  await fetch(`${API_BASE}/api/auth/logout`, {
    method: "POST",
    headers: { "X-Requested-With": "kyozai-kobo" },
    credentials: "same-origin",
  });
}

// ---- 変更イベントの購読（デスクトップ: Tauriイベント / Web: SSE） ----

export interface AppEvent {
  kind: string;
  cmd: string;
  ids: Record<string, number> | null;
}

export type EventHandler = (ev: AppEvent) => void;

/** イベント購読を開始する。返り値は購読解除関数 */
export function subscribeEvents(
  handler: EventHandler,
  onConnectionChange?: (connected: boolean) => void,
): () => void {
  if (isTauri) {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<AppEvent>("app-event", (e) => handler(e.payload)).then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    });
    onConnectionChange?.(true);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }

  // Web: SSE（切断時は自動再接続）
  let es: EventSource | null = null;
  let closed = false;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;

  const connect = () => {
    if (closed) return;
    es = new EventSource(`${API_BASE}/api/events`);
    es.onopen = () => onConnectionChange?.(true);
    es.onmessage = (e) => {
      try {
        handler(JSON.parse(e.data));
      } catch {
        /* keep-alive等は無視 */
      }
    };
    es.onerror = () => {
      onConnectionChange?.(false);
      es?.close();
      es = null;
      if (!closed) {
        retryTimer = setTimeout(connect, 3000);
      }
    };
  };
  connect();
  return () => {
    closed = true;
    if (retryTimer) clearTimeout(retryTimer);
    es?.close();
  };
}
