import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  ExprItem,
  MathLabel,
  PointItem,
  PaperSettings,
  Project,
  ViewRange,
  ParseResult,
} from "./types";
import { parseExpression } from "./lib/parser";
import type { RenderItem } from "./lib/buildSvg";
import { buildPdf, buildExportSvg, svgToPngBytes, svgFileContent } from "./lib/export";
import {
  sampleProject,
  defaultExprItem,
  defaultLabel,
  serializeProject,
  deserializeProject,
  defaultFileName,
  newId,
} from "./lib/project";
import { initMathJax } from "./lib/mathlabel";
import type { Intersection } from "./lib/intersections";
import { graphAspectRatio } from "./lib/aspect";
import { isTauri, saveBinaryFile, saveTextFile, openTextFile, sanitizeFileName } from "./lib/platform";
import ExpressionPanel from "./components/ExpressionPanel";
import GraphView from "./components/GraphView";
import SettingsPanel from "./components/SettingsPanel";
import PdfPreviewModal from "./components/PdfPreviewModal";
import type { jsPDF } from "jspdf";

interface Toast {
  msg: string;
  kind: "info" | "error";
}

interface IntegrationRequestInfo {
  requestId: string;
  requestPath: string;
  returnFolder: string;
  mode: string;
  updateAssetId: string | null;
  latexWidth: string | null;
  initialProjectJson: string | null;
}

function bytesToBase64(data: Uint8Array): string {
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < data.length; i += chunk) {
    bin += String.fromCharCode(...data.subarray(i, i + chunk));
  }
  return btoa(bin);
}

export default function App() {
  const [project, setProject] = useState<Project>(() => sampleProject());
  const [warnings, setWarnings] = useState<Map<string, string>>(new Map());
  const [toast, setToast] = useState<Toast | null>(null);
  const [busy, setBusy] = useState(false);
  const [pdfPreview, setPdfPreview] = useState<{ url: string } | null>(null);
  const [mathReady, setMathReady] = useState(false);
  const [selectedLabelId, setSelectedLabelId] = useState<string | null>(null);
  const [intersections, setIntersections] = useState<Intersection[]>([]);
  const [integration, setIntegration] = useState<IntegrationRequestInfo | null>(null);
  const pdfDocRef = useRef<jsPDF | null>(null);
  const toastTimer = useRef<number>(0);

  // MathJax（数式ラベルのベクター組版）を起動時に読み込む
  useEffect(() => {
    let alive = true;
    initMathJax().then((ok) => {
      if (alive) setMathReady(ok);
    });
    return () => {
      alive = false;
    };
  }, []);

  // 交点表示をオフにしたら一覧をクリア
  useEffect(() => {
    if (!project.paper.showIntersections) setIntersections([]);
  }, [project.paper.showIntersections]);

  const showToast = useCallback((msg: string, kind: Toast["kind"] = "info") => {
    setToast({ msg, kind });
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3500);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    invoke<IntegrationRequestInfo | null>("get_integration_request")
      .then((info) => {
        if (!alive || !info) return;
        setIntegration(info);
        if (info.initialProjectJson) {
          const res = deserializeProject(info.initialProjectJson);
          if (!res.ok) {
            showToast(res.message, "error");
            return;
          }
          parseCache.current.clear();
          setSelectedLabelId(null);
          setProject(res.project);
        }
        showToast(info.mode === "reedit" ? "教材連携: 再編集モードです" : "教材連携: 挿入モードです");
      })
      .catch((e) => showToast(String(e), "error"));
    return () => {
      alive = false;
    };
  }, [showToast]);

  // ---- 数式パース（入力文字列単位でキャッシュ） ----
  const parseCache = useRef(new Map<string, ParseResult>());
  const rendered: RenderItem[] = useMemo(() => {
    const cache = parseCache.current;
    if (cache.size > 800) cache.clear();
    return project.expressions.map((item) => {
      let parsed = cache.get(item.input);
      if (!parsed) {
        parsed = parseExpression(item.input);
        cache.set(item.input, parsed);
      }
      return { item, parsed };
    });
  }, [project.expressions]);

  // ---- 更新ヘルパー ----
  const updateExpr = useCallback((id: string, patch: Partial<ExprItem>) => {
    setProject((p) => ({
      ...p,
      expressions: p.expressions.map((e) => (e.id === id ? { ...e, ...patch } : e)),
    }));
  }, []);

  const addExpr = useCallback(() => {
    setProject((p) => ({
      ...p,
      expressions: [...p.expressions, defaultExprItem("", p.expressions.length)],
    }));
  }, []);

  const removeExpr = useCallback((id: string) => {
    setProject((p) => ({
      ...p,
      expressions: p.expressions.filter((e) => e.id !== id),
    }));
  }, []);

  const reorderExpr = useCallback((from: number, to: number) => {
    setProject((p) => {
      if (from === to || from < 0 || to < 0) return p;
      const arr = [...p.expressions];
      const [moved] = arr.splice(from, 1);
      arr.splice(to, 0, moved);
      return { ...p, expressions: arr };
    });
  }, []);

  const updateRange = useCallback((patch: Partial<ViewRange>) => {
    setProject((p) => ({ ...p, range: { ...p.range, ...patch } }));
  }, []);

  const updatePaper = useCallback((patch: Partial<PaperSettings>) => {
    setProject((p) => ({ ...p, paper: { ...p.paper, ...patch } }));
  }, []);

  const addPoint = useCallback(() => {
    setProject((p) => ({
      ...p,
      points: [
        ...p.points,
        {
          id: newId(),
          x: 0,
          y: 0,
          label: "",
          color: "#dc2626",
          visible: true,
          showProjectionToXAxis: false,
          showProjectionToYAxis: false,
        },
      ],
    }));
  }, []);

  const updatePoint = useCallback((id: string, patch: Partial<PointItem>) => {
    setProject((p) => ({
      ...p,
      points: p.points.map((pt) => (pt.id === id ? { ...pt, ...patch } : pt)),
    }));
  }, []);

  const removePoint = useCallback((id: string) => {
    setProject((p) => ({ ...p, points: p.points.filter((pt) => pt.id !== id) }));
  }, []);

  // ---- 数式ラベル ----
  const addLabel = useCallback(() => {
    setProject((p) => {
      // 表示範囲の中央付近に新規ラベルを置く
      const cx = p.range.xmin + (p.range.xmax - p.range.xmin) * 0.5;
      const cy = p.range.ymin + (p.range.ymax - p.range.ymin) * 0.55;
      const lb = defaultLabel("y = x^2", cx, cy);
      queueMicrotask(() => setSelectedLabelId(lb.id));
      return { ...p, labels: [...p.labels, lb] };
    });
  }, []);

  const updateLabel = useCallback((id: string, patch: Partial<MathLabel>) => {
    setProject((p) => ({
      ...p,
      labels: p.labels.map((l) => (l.id === id ? { ...l, ...patch } : l)),
    }));
  }, []);

  const removeLabel = useCallback((id: string) => {
    setProject((p) => ({ ...p, labels: p.labels.filter((l) => l.id !== id) }));
    setSelectedLabelId((cur) => (cur === id ? null : cur));
  }, []);

  const moveLabel = useCallback((id: string, x: number, y: number) => {
    setProject((p) => ({
      ...p,
      labels: p.labels.map((l) => (l.id === id ? { ...l, x, y } : l)),
    }));
  }, []);

  // ---- 出力 ----
  const exportBaseName = () => sanitizeFileName(project.paper.title || "graph");

  const exportSizePx = () => {
    const ratio = graphAspectRatio(project.paper, project.range);
    const w = 1600;
    return { w, h: Math.round(w / ratio) };
  };

  const handlePdfPreview = async () => {
    setBusy(true);
    try {
      const doc = await buildPdf(project, rendered);
      pdfDocRef.current = doc;
      if (pdfPreview) URL.revokeObjectURL(pdfPreview.url);
      const url = doc.output("bloburl") as unknown as string;
      setPdfPreview({ url });
    } catch (e) {
      console.error(e);
      showToast(`PDFの生成に失敗しました: ${e instanceof Error ? e.message : e}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const handlePdfSave = async () => {
    const doc = pdfDocRef.current;
    if (!doc) return;
    try {
      const bytes = new Uint8Array(doc.output("arraybuffer"));
      const saved = await saveBinaryFile(
        defaultFileName(exportBaseName(), "pdf"),
        bytes,
        "PDFファイル",
        ["pdf"],
        "application/pdf",
      );
      if (saved) {
        showToast("PDFを保存しました");
        setPdfPreview(null);
      }
    } catch (e) {
      showToast(`保存に失敗しました: ${e instanceof Error ? e.message : e}`, "error");
    }
  };

  const handlePng = async () => {
    setBusy(true);
    try {
      await initMathJax();
      const { w, h } = exportSizePx();
      const svg = buildExportSvg(project, rendered, w, h);
      const bytes = await svgToPngBytes(svg, 2);
      const saved = await saveBinaryFile(
        defaultFileName(exportBaseName(), "png"),
        bytes,
        "PNG画像",
        ["png"],
        "image/png",
      );
      if (saved) showToast("PNGを保存しました");
    } catch (e) {
      showToast(`PNG出力に失敗しました: ${e instanceof Error ? e.message : e}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const handleSvg = async () => {
    setBusy(true);
    try {
      await initMathJax();
      const { w, h } = exportSizePx();
      const svg = svgFileContent(buildExportSvg(project, rendered, w, h));
      const saved = await saveTextFile(
        defaultFileName(exportBaseName(), "svg"),
        svg,
        "SVG画像",
        ["svg"],
        "image/svg+xml",
      );
      if (saved) showToast("SVGを保存しました");
    } catch (e) {
      showToast(`SVG出力に失敗しました: ${e instanceof Error ? e.message : e}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const handleSaveProject = async () => {
    try {
      const saved = await saveTextFile(
        `${exportBaseName()}.mathgraph.json`,
        serializeProject(project),
        "MathGraphプロジェクト",
        ["json"],
        "application/json",
      );
      if (saved) showToast("プロジェクトを保存しました");
    } catch (e) {
      showToast(`保存に失敗しました: ${e instanceof Error ? e.message : e}`, "error");
    }
  };

  const handleOpenProject = async () => {
    try {
      const text = await openTextFile("MathGraphプロジェクト", ["json"]);
      if (text === null) return;
      const res = deserializeProject(text);
      if (!res.ok) {
        showToast(res.message, "error");
        return;
      }
      parseCache.current.clear();
      setSelectedLabelId(null);
      setProject(res.project);
      showToast("プロジェクトを読み込みました");
    } catch (e) {
      showToast(`読み込みに失敗しました: ${e instanceof Error ? e.message : e}`, "error");
    }
  };

  const handleNewProject = () => {
    if (!window.confirm("現在の内容を破棄して新規プロジェクトを作成しますか？")) return;
    setSelectedLabelId(null);
    setProject({
      ...sampleProject(),
      expressions: [defaultExprItem("y = x^2", 0)],
      labels: [],
      paper: { ...sampleProject().paper, title: "" },
    });
  };

  const handleCompleteIntegration = async () => {
    if (!integration) return;
    setBusy(true);
    try {
      const doc = await buildPdf(project, rendered);
      const pdfBytes = new Uint8Array(doc.output("arraybuffer"));
      const { w, h } = exportSizePx();
      const svg = buildExportSvg(project, rendered, w, h);
      const pngBytes = await svgToPngBytes(svg, 2);
      const displayName = project.paper.title.trim() || project.paper.problemNumber.trim() || "Graph";
      const graphId =
        integration.updateAssetId ||
        `graph_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
      const width = integration.latexWidth || "0.72\\linewidth";
      const graphTex =
        `% Generated by MathGraph PDF Studio\n` +
        `\\begin{center}\n  \\includegraphics[width=${width}]{graph.pdf}\n\\end{center}\n`;
      await invoke("complete_integration", {
        payload: {
          requestPath: integration.requestPath,
          graphId,
          displayName,
          pdfBase64: bytesToBase64(pdfBytes),
          pngBase64: bytesToBase64(pngBytes),
          thumbnailBase64: bytesToBase64(pngBytes),
          graphJson: serializeProject(project),
          graphTex,
          graphType: "function",
        },
      });
      showToast(
        integration.mode === "reedit" ? "教材へ更新データを書き出しました" : "教材へ挿入データを書き出しました",
      );
    } catch (e) {
      showToast(`教材連携の書き出しに失敗しました: ${e instanceof Error ? e.message : e}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const handleCancelIntegration = async () => {
    if (!integration) return;
    try {
      await invoke("cancel_integration", { requestPath: integration.requestPath });
      showToast("教材連携を中止しました");
    } catch (e) {
      showToast(`中止状態を書き出せませんでした: ${e instanceof Error ? e.message : e}`, "error");
    }
  };

  return (
    <div className="h-full flex flex-col">
      {/* ヘッダー */}
      <header
        className="flex items-center gap-3 px-4 h-11 flex-none"
        style={{ background: "var(--panel)", borderBottom: "1px solid var(--border)" }}
      >
        <div
          className="w-5 h-5 rounded flex-none"
          style={{
            background: "linear-gradient(135deg, #22d3ee, #8b5cf6)",
            boxShadow: "0 0 10px #22d3ee66",
          }}
        />
        <h1 className="text-[13.5px] font-bold tracking-wide">
          MathGraph <span style={{ color: "var(--accent)" }}>PDF Studio</span>
        </h1>
        <span className="text-[11px]" style={{ color: "var(--text-faint)" }}>
          数式グラフ教材メーカー
        </span>
        <div className="flex-1" />
        {integration && (
          <>
            <span
              className="rounded border px-2 py-0.5 text-[11px]"
              style={{ borderColor: "var(--accent)", color: "var(--accent)", background: "#0e2a3355" }}
            >
              {integration.mode === "reedit" ? "教材連携: 再編集" : "教材連携: 挿入"}
            </span>
            <button className="btn btn-primary" onClick={handleCompleteIntegration} disabled={busy}>
              {busy ? "書き出し中..." : integration.mode === "reedit" ? "既存の図を更新" : "教材へ挿入"}
            </button>
            <button className="btn" onClick={handleCancelIntegration} disabled={busy}>
              連携を中止
            </button>
          </>
        )}
        <span className="text-[11px]" style={{ color: "var(--text-faint)" }}>
          {project.paper.title || "無題のプロジェクト"}
        </span>
      </header>

      {/* 3カラム */}
      <div className="flex-1 flex min-h-0">
        <aside className="w-[330px] flex-none panel border-t-0 border-l-0 border-b-0 overflow-y-auto">
          <ExpressionPanel
            items={rendered}
            points={project.points}
            labels={project.labels}
            selectedLabelId={selectedLabelId}
            mathReady={mathReady}
            warnings={warnings}
            onAdd={addExpr}
            onUpdate={updateExpr}
            onRemove={removeExpr}
            onReorder={reorderExpr}
            onAddPoint={addPoint}
            onUpdatePoint={updatePoint}
            onRemovePoint={removePoint}
            onAddLabel={addLabel}
            onUpdateLabel={updateLabel}
            onRemoveLabel={removeLabel}
            onSelectLabel={setSelectedLabelId}
          />
        </aside>

        <main className="flex-1 min-w-0 graph-stage">
          <GraphView
            items={rendered}
            points={project.points}
            labels={project.labels}
            range={project.range}
            paper={project.paper}
            mathReady={mathReady}
            selectedLabelId={selectedLabelId}
            onRangeChange={updateRange}
            onWarningsChange={setWarnings}
            onLabelMove={moveLabel}
            onSelectLabel={setSelectedLabelId}
            onIntersectionsChange={setIntersections}
          />
        </main>

        <aside className="w-[300px] flex-none panel border-t-0 border-r-0 border-b-0 overflow-y-auto">
          <SettingsPanel
            range={project.range}
            paper={project.paper}
            intersections={intersections}
            busy={busy}
            onRangeChange={updateRange}
            onPaperChange={updatePaper}
            onPdf={handlePdfPreview}
            onPng={handlePng}
            onSvg={handleSvg}
            onSaveProject={handleSaveProject}
            onOpenProject={handleOpenProject}
            onNewProject={handleNewProject}
          />
        </aside>
      </div>

      {pdfPreview && (
        <PdfPreviewModal
          url={pdfPreview.url}
          onSave={handlePdfSave}
          onClose={() => {
            URL.revokeObjectURL(pdfPreview.url);
            setPdfPreview(null);
          }}
        />
      )}

      {toast && (
        <div className={`toast ${toast.kind === "error" ? "toast-error" : "toast-info"}`}>
          {toast.msg}
        </div>
      )}
    </div>
  );
}
