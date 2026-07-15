import { useEffect, useRef, useState } from "react";
import type { PDFDocumentLoadingTask, PDFDocumentProxy, RenderTask } from "pdfjs-dist";

let pdfJsPromise: Promise<typeof import("pdfjs-dist")> | null = null;

async function loadPdfJs() {
  if (!pdfJsPromise) {
    pdfJsPromise = Promise.all([
      import("pdfjs-dist"),
      import("pdfjs-dist/build/pdf.worker.min.mjs?url"),
    ]).then(([pdfjs, worker]) => {
      pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
      return pdfjs;
    });
  }
  return pdfJsPromise;
}

/** iOSの組み込みPDF表示に依存せず、全ページを高精細Canvasで表示するビューア。 */
export function PdfCanvasViewer({ src, zoom }: { src: string; zoom: number }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const pagesRef = useRef<HTMLDivElement>(null);
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [width, setWidth] = useState(0);
  const [status, setStatus] = useState("PDFを読み込んでいます...");
  const [error, setError] = useState("");

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const update = () => setWidth(Math.max(0, Math.floor(host.clientWidth)));
    update();
    const observer = new ResizeObserver(update);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let active = true;
    let loadingTask: PDFDocumentLoadingTask | null = null;
    setPdf(null);
    setError("");
    setStatus("PDFを読み込んでいます...");
    void loadPdfJs()
      .then((pdfjs) => {
        // withCredentialsは付けない: 同一オリジン（Web版のセッションCookie）は
        // 既定のcredentials: "same-origin"で送信される。trueにするとデスクトップで
        // asset.localhostへのクロスオリジンfetchがCORSで必ず失敗する。
        loadingTask = pdfjs.getDocument({
          url: src,
          cMapUrl: "/pdfjs/cmaps/",
          cMapPacked: true,
          standardFontDataUrl: "/pdfjs/standard_fonts/",
          wasmUrl: "/pdfjs/wasm/",
          useSystemFonts: true,
        });
        return loadingTask.promise;
      })
      .then((loaded) => {
        if (!active) {
          void loadingTask?.destroy();
          return;
        }
        setPdf(loaded);
        setStatus(`${loaded.numPages}ページ`);
      })
      .catch((reason) => {
        if (active) setError(`PDFを表示できません: ${String(reason)}`);
      });
    return () => {
      active = false;
      if (loadingTask) void loadingTask.destroy();
    };
  }, [src]);

  useEffect(() => {
    const pages = pagesRef.current;
    if (!pages || !pdf || width <= 0) return;

    let active = true;
    const tasks = new Map<HTMLDivElement, RenderTask>();
    pages.replaceChildren();
    setError("");
    setStatus(`${pdf.numPages}ページを準備中...`);

    const renderPage = async (
      wrapper: HTMLDivElement,
      canvas: HTMLCanvasElement,
      pageNumber: number,
      cssWidth: number,
      cssHeight: number,
    ) => {
      if (!active || wrapper.dataset.rendering === "1" || wrapper.dataset.rendered === "1") return;
      wrapper.dataset.rendering = "1";
      try {
        const page = await pdf.getPage(pageNumber);
        if (!active || wrapper.dataset.near !== "1") {
          page.cleanup();
          return;
        }
        const viewport = page.getViewport({ scale: cssWidth / page.getViewport({ scale: 1 }).width });
        // iPhoneの3x表示を保ち、デスクトップでも最低2xで描画する。
        // 高倍率時も解像度を落とさず、近接ページだけを描画してメモリを抑える。
        const deviceRatio = Math.max(2, window.devicePixelRatio || 1);
        const pixelRatio = Math.min(deviceRatio, zoom > 250 ? 2.5 : 3);
        canvas.width = Math.max(1, Math.ceil(viewport.width * pixelRatio));
        canvas.height = Math.max(1, Math.ceil(viewport.height * pixelRatio));
        canvas.style.width = `${cssWidth}px`;
        canvas.style.height = `${cssHeight}px`;
        const context = canvas.getContext("2d", { alpha: false });
        if (!context) throw new Error("Canvasを初期化できません");
        context.imageSmoothingEnabled = true;
        context.imageSmoothingQuality = "high";
        const task = page.render({
          canvas,
          canvasContext: context,
          viewport,
          transform: pixelRatio === 1 ? undefined : [pixelRatio, 0, 0, pixelRatio, 0, 0],
        });
        tasks.set(wrapper, task);
        await task.promise;
        if (active) wrapper.dataset.rendered = "1";
        page.cleanup();
      } catch (reason) {
        if (active && (reason as { name?: string })?.name !== "RenderingCancelledException") {
          setError(`PDFの描画に失敗しました: ${String(reason)}`);
        }
      } finally {
        tasks.delete(wrapper);
        delete wrapper.dataset.rendering;
      }
    };

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const wrapper = entry.target as HTMLDivElement;
          const canvas = wrapper.querySelector("canvas");
          if (!(canvas instanceof HTMLCanvasElement)) continue;
          if (entry.isIntersecting) {
            wrapper.dataset.near = "1";
            void renderPage(
              wrapper,
              canvas,
              Number(wrapper.dataset.page),
              Number(wrapper.dataset.width),
              Number(wrapper.dataset.height),
            );
          } else {
            wrapper.dataset.near = "0";
            tasks.get(wrapper)?.cancel();
            delete wrapper.dataset.rendered;
            canvas.width = 1;
            canvas.height = 1;
          }
        }
      },
      { root: null, rootMargin: "120% 0px", threshold: 0.01 },
    );

    void (async () => {
      try {
        for (let pageNumber = 1; pageNumber <= pdf.numPages && active; pageNumber += 1) {
          const page = await pdf.getPage(pageNumber);
          if (!active) break;
          const natural = page.getViewport({ scale: 1 });
          const fitScale = Math.max(0.1, (width - 40) / natural.width);
          const viewport = page.getViewport({ scale: fitScale * (zoom / 100) });
          const wrapper = document.createElement("div");
          wrapper.className = "pdf-canvas-page";
          wrapper.dataset.page = String(pageNumber);
          wrapper.dataset.width = String(viewport.width);
          wrapper.dataset.height = String(viewport.height);
          wrapper.style.width = `${viewport.width}px`;
          wrapper.style.height = `${viewport.height}px`;
          wrapper.setAttribute("aria-label", `${pageNumber}ページ目`);
          const canvas = document.createElement("canvas");
          canvas.width = 1;
          canvas.height = 1;
          canvas.style.width = `${viewport.width}px`;
          canvas.style.height = `${viewport.height}px`;
          wrapper.appendChild(canvas);
          pages.appendChild(wrapper);
          observer.observe(wrapper);
          page.cleanup();
        }
        if (active) setStatus(`${pdf.numPages}ページ・${zoom}%・高精細表示`);
      } catch (reason) {
        if (active) setError(`PDFの準備に失敗しました: ${String(reason)}`);
      }
    })();

    return () => {
      active = false;
      observer.disconnect();
      for (const task of tasks.values()) task.cancel();
      tasks.clear();
    };
  }, [pdf, width, zoom]);

  return (
    <div ref={hostRef} className="pdf-canvas-viewer min-w-0">
      <div className="pdf-canvas-status">{error || status}</div>
      <div ref={pagesRef} className="pdf-canvas-pages" />
    </div>
  );
}
