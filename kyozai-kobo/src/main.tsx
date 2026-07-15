import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { isTauri } from "./transport";

// Web版のみ: Service Worker登録（PWA対応・更新通知）
if (!isTauri && "serviceWorker" in navigator && !import.meta.env.DEV) {
  window.addEventListener("load", () => {
    navigator.serviceWorker
      .register("/sw.js")
      .then((reg) => {
        reg.addEventListener("updatefound", () => {
          const worker = reg.installing;
          if (!worker) return;
          worker.addEventListener("statechange", () => {
            if (worker.state === "installed" && navigator.serviceWorker.controller) {
              // 新バージョンが待機中 → 適用して次回リロードで反映
              worker.postMessage("SKIP_WAITING");
              const note = document.createElement("div");
              note.textContent = "新しいバージョンがあります。再読み込みで更新されます。";
              note.style.cssText =
                "position:fixed;bottom:12px;right:12px;z-index:9999;background:#1b2434;color:#d7e0ec;border:1px solid #31456384;border-radius:6px;padding:8px 14px;font-size:12px;cursor:pointer;";
              note.onclick = () => window.location.reload();
              document.body.appendChild(note);
              setTimeout(() => note.remove(), 15000);
            }
          });
        });
      })
      .catch(() => {});
  });
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
