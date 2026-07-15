import { jsPDF } from "jspdf";
import "svg2pdf.js";
import type { Project } from "../types";
import { buildGraphSvg, type RenderItem } from "./buildSvg";
import { graphAspectRatio, graphDisplayRange, pdfPageAspectRatio } from "./aspect";
import fontUrl from "../assets/fonts/ipaexg.ttf?url";
import { initMathJax } from "./mathlabel";

const PX_PER_MM = 96 / 25.4;

/** 出力用の高品質設定 */
const EXPORT_QUALITY = { samples: 1400, implicitGrid: 300 };

// IPAexゴシック（日本語フォント）の base64 を一度だけ読み込んでキャッシュ
let fontB64Promise: Promise<string> | null = null;
function loadFontB64(): Promise<string> {
  fontB64Promise ??= (async () => {
    const res = await fetch(fontUrl);
    const buf = new Uint8Array(await res.arrayBuffer());
    let bin = "";
    const chunk = 0x8000;
    for (let i = 0; i < buf.length; i += chunk) {
      bin += String.fromCharCode(...buf.subarray(i, i + chunk));
    }
    return btoa(bin);
  })();
  return fontB64Promise;
}

async function registerJapaneseFont(doc: jsPDF): Promise<void> {
  const b64 = await loadFontB64();
  doc.addFileToVFS("ipaexg.ttf", b64);
  doc.addFont("ipaexg.ttf", "IPAexGothic", "normal");
  doc.setFont("IPAexGothic", "normal");
}

/** グラフ部分の SVG を出力品質で生成する（ピクセルサイズ指定） */
export function buildExportSvg(
  project: Project,
  items: RenderItem[],
  widthPx: number,
  heightPx: number,
): string {
  return buildGraphSvg(items, project.points, graphDisplayRange(project.paper, project.range), {
    width: widthPx,
    height: heightPx,
    paper: project.paper,
    labels: project.labels,
    idPrefix: "exp",
    detectIntersections: project.paper.showIntersections,
    ...EXPORT_QUALITY,
  }).svg;
}

/**
 * グラフSVGを指定位置・サイズでPDFにベクター描画する。
 * svg2pdf はレイアウト計算に DOM を使うため、一時的に DOM へ追加する。
 */
async function drawGraphToPdf(
  doc: jsPDF,
  project: Project,
  items: RenderItem[],
  gx: number,
  gy: number,
  gw: number,
  gh: number,
): Promise<void> {
  const svgStr = buildExportSvg(project, items, gw * PX_PER_MM, gh * PX_PER_MM);
  const holder = document.createElement("div");
  holder.style.position = "fixed";
  holder.style.left = "-100000px";
  holder.style.top = "0";
  holder.innerHTML = svgStr;
  document.body.appendChild(holder);
  try {
    const el = holder.querySelector("svg");
    if (!el) throw new Error("SVGの生成に失敗しました");
    doc.setFont("IPAexGothic", "normal");
    doc.setFontSize(10);
    await doc.svg(el, { x: gx, y: gy, width: gw, height: gh });
  } finally {
    holder.remove();
  }
}

/**
 * 教材レイアウトの A4 PDF を生成する。
 * グラフは svg2pdf.js によりベクターとして埋め込まれる。
 */
export async function buildPdf(
  project: Project,
  items: RenderItem[],
): Promise<jsPDF> {
  await initMathJax();
  const paper = project.paper;

  // --- グラフのみ出力: グラフの縦横比ぴったりの用紙に端から端まで描く ---
  // タイトルなしのPDFで、LaTeX の \includegraphics に使いやすい。
  // custom の用紙比率では、グラフ自体は伸縮せず余白を足して中央配置する。
  if (paper.pdfGraphOnly) {
    const pageRatio = pdfPageAspectRatio(project.paper, project.range);
    const graphRatio = graphAspectRatio(project.paper, project.range);
    const pageW = paper.pdfGraphWidthMm;
    const pageH = pageW / pageRatio;
    const doc = new jsPDF({
      unit: "mm",
      format: [pageW, pageH],
      orientation: pageW >= pageH ? "landscape" : "portrait",
      compress: true,
    });
    await registerJapaneseFont(doc);
    const pw2 = doc.internal.pageSize.getWidth();
    const ph2 = doc.internal.pageSize.getHeight();
    let gw = pw2;
    let gh = gw / graphRatio;
    if (gh > ph2) {
      gh = ph2;
      gw = gh * graphRatio;
    }
    const gx = (pw2 - gw) / 2;
    const gy = (ph2 - gh) / 2;
    await drawGraphToPdf(doc, project, items, gx, gy, gw, gh);
    return doc;
  }

  const doc = new jsPDF({
    unit: "mm",
    format: "a4",
    orientation: paper.orientation,
    compress: true,
  });
  await registerJapaneseFont(doc);

  const pw = doc.internal.pageSize.getWidth();
  const ph = doc.internal.pageSize.getHeight();
  const m = paper.marginMm;
  let yCur = m;

  // 問題番号（右上）
  if (paper.problemNumber.trim()) {
    doc.setFontSize(12);
    doc.setTextColor(20, 22, 28);
    doc.text(paper.problemNumber, pw - m, m + 5, { align: "right" });
  }

  // タイトル（中央上部）
  if (paper.title.trim()) {
    doc.setFontSize(17);
    doc.setTextColor(10, 12, 16);
    doc.text(paper.title, pw / 2, yCur + 6.5, { align: "center" });
    yCur += 13;
  }
  if (paper.subtitle.trim()) {
    doc.setFontSize(11);
    doc.setTextColor(75, 80, 92);
    doc.text(paper.subtitle, pw / 2, yCur + 4.5, { align: "center" });
    yCur += 9;
  }
  yCur += 2;

  // 説明文（下部固定）の必要領域を先に計算
  doc.setFontSize(10.5);
  const captionLines: string[] = paper.caption.trim()
    ? (doc.splitTextToSize(paper.caption, pw - 2 * m) as string[])
    : [];
  const lineH = 5.4;
  const captionH = captionLines.length > 0 ? captionLines.length * lineH + 8 : 0;

  // グラフ領域の計算
  const availW = pw - 2 * m;
  const availH = ph - m - captionH - yCur - 2;
  let gw = availW;
  let gh = availH;
  const ratio = graphAspectRatio(project.paper, project.range);
  if (gw / gh > ratio) gw = gh * ratio;
  else gh = gw / ratio;
  const gx = (pw - gw) / 2;
  const gy = yCur + Math.max(0, (availH - gh) / 2);

  await drawGraphToPdf(doc, project, items, gx, gy, gw, gh);

  // 説明文（グラフ下・ページ下部）
  if (captionLines.length > 0) {
    doc.setFont("IPAexGothic", "normal");
    doc.setFontSize(10.5);
    doc.setTextColor(30, 33, 40);
    const capTop = ph - m - captionLines.length * lineH + lineH - 1;
    doc.text(captionLines, m, capTop);
  }

  return doc;
}

/** SVG 文字列を PNG バイト列にラスタライズする */
export async function svgToPngBytes(
  svgStr: string,
  scale = 3,
): Promise<Uint8Array> {
  const blob = new Blob([svgStr], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  try {
    const img = new Image();
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("SVGの読み込みに失敗しました"));
      img.src = url;
    });
    const w = Math.round(img.naturalWidth * scale);
    const h = Math.round(img.naturalHeight * scale);
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvasを初期化できませんでした");
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, w, h);
    ctx.drawImage(img, 0, 0, w, h);
    const pngBlob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png"),
    );
    if (!pngBlob) throw new Error("PNGの生成に失敗しました");
    return new Uint8Array(await pngBlob.arrayBuffer());
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** スタンドアロン SVG ファイルの内容を作る */
export function svgFileContent(svgStr: string): string {
  return `<?xml version="1.0" encoding="UTF-8"?>\n` + svgStr;
}
