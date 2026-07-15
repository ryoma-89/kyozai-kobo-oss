import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";

/** Tauri 環境（デスクトップアプリ）で動いているか */
export const isTauri = (): boolean => "__TAURI_INTERNALS__" in window;

function toBase64(data: Uint8Array): string {
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < data.length; i += chunk) {
    bin += String.fromCharCode(...data.subarray(i, i + chunk));
  }
  return btoa(bin);
}

function browserDownload(name: string, blob: Blob): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}

/** Windows のファイル名に使えない文字を除去 */
export function sanitizeFileName(name: string): string {
  return name.replace(/[\\/:*?"<>|]/g, "_").trim() || "untitled";
}

/**
 * バイナリを保存する。Tauri なら保存ダイアログ、ブラウザならダウンロード。
 * @returns 保存した場合 true、キャンセルした場合 false
 */
export async function saveBinaryFile(
  defaultName: string,
  data: Uint8Array,
  filterName: string,
  extensions: string[],
  mime: string,
): Promise<boolean> {
  if (isTauri()) {
    const path = await save({
      defaultPath: defaultName,
      filters: [{ name: filterName, extensions }],
    });
    if (!path) return false;
    await invoke("write_file", { path, contentsBase64: toBase64(data) });
    return true;
  }
  browserDownload(defaultName, new Blob([data.buffer as ArrayBuffer], { type: mime }));
  return true;
}

/** テキストを保存する */
export async function saveTextFile(
  defaultName: string,
  text: string,
  filterName: string,
  extensions: string[],
  mime: string,
): Promise<boolean> {
  if (isTauri()) {
    const path = await save({
      defaultPath: defaultName,
      filters: [{ name: filterName, extensions }],
    });
    if (!path) return false;
    await invoke("write_text_file", { path, contents: text });
    return true;
  }
  browserDownload(defaultName, new Blob([text], { type: mime }));
  return true;
}

/**
 * テキストファイルを開く。
 * @returns ファイル内容。キャンセル時は null
 */
export async function openTextFile(
  filterName: string,
  extensions: string[],
): Promise<string | null> {
  if (isTauri()) {
    const path = await open({
      multiple: false,
      filters: [{ name: filterName, extensions }],
    });
    if (typeof path !== "string") return null;
    return await invoke<string>("read_text_file", { path });
  }
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = extensions.map((e) => `.${e}`).join(",");
    input.onchange = () => {
      const file = input.files?.[0];
      if (!file) return resolve(null);
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result ?? ""));
      reader.onerror = () => resolve(null);
      reader.readAsText(file);
    };
    // キャンセルは検出できないケースがあるが実害はない
    input.oncancel = () => resolve(null);
    input.click();
  });
}
