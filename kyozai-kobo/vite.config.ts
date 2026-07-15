import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],

  // iPad/PWAの初回読込と更新キャッシュを小さく保つため、重い依存を
  // アプリ本体から分離する。各ファイル名はViteのcontent hashで更新される。
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("/react/") || id.includes("/react-dom/") || id.includes("/zustand/")) {
            return "vendor-react";
          }
          if (id.includes("/@codemirror/") || id.includes("/@lezer/")) {
            return "vendor-editor";
          }
          if (id.includes("/@dnd-kit/")) {
            return "vendor-dnd";
          }
          if (
            id.includes("/mathjs/") || id.includes("/jspdf/") || id.includes("/svg2pdf.js/")
            || id.includes("/katex/") || id.includes("/lucide-react/")
          ) {
            return "vendor-graph";
          }
          return undefined;
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
