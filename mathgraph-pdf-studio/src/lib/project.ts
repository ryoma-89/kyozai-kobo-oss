import type {
  AspectMode,
  ExprItem,
  FillStyle,
  MathLabel,
  PointItem,
  PaperSettings,
  Project,
  ViewRange,
} from "../types";
import { clampAspectRatio } from "./aspect";

/** 塗りつぶし方式の値を検証する */
function toFillStyle(v: unknown, fallback: FillStyle): FillStyle {
  return v === "solid" || v === "hatch" || v === "crosshatch" || v === "dot"
    ? v
    : fallback;
}

function toAspectMode(v: unknown, lockAspect: boolean): AspectMode {
  if (v === "custom") return "custom";
  if (v === "range") return "range";
  return lockAspect ? "range" : "custom";
}

export const PROJECT_EXT = "mathgraph.json";

let idCounter = 0;
export function newId(): string {
  idCounter += 1;
  return `e${Date.now().toString(36)}-${idCounter}`;
}

/** 式の色の既定パレット（黒背景UIでも白いグラフ面でも映える色） */
export const COLOR_PALETTE = [
  "#0891b2", // シアン
  "#7c3aed", // 青紫
  "#059669", // エメラルド
  "#d97706", // アンバー
  "#dc2626", // レッド
  "#2563eb", // ブルー
  "#db2777", // ピンク
];

export function defaultExprItem(input: string, index: number): ExprItem {
  const color = COLOR_PALETTE[index % COLOR_PALETTE.length];
  return {
    id: newId(),
    input,
    name: "",
    visible: true,
    color,
    lineWidth: 2,
    lineStyle: "solid",
    fillColor: color,
    fillOpacity: 0.25,
    fillStyle: "solid",
    tmin: 0,
    tmax: 2 * Math.PI,
  };
}

export function defaultLabel(latex: string, x: number, y: number): MathLabel {
  return {
    id: newId(),
    latex,
    x,
    y,
    fontSize: 20,
    color: "#111318",
    visible: true,
  };
}

export function defaultRange(): ViewRange {
  return { xmin: -5, xmax: 5, ymin: -5, ymax: 5, xstep: 1, ystep: 1 };
}

export function defaultPaper(): PaperSettings {
  return {
    orientation: "portrait",
    title: "",
    subtitle: "",
    problemNumber: "",
    caption: "",
    showAxes: true,
    axisLabelX: "x",
    axisLabelY: "y",
    axisLabelO: "O",
    axisLabelSize: 17,
    showTicks: true,
    showGrid: true,
    showLegend: true,
    legendFontSize: 13,
    showIntersections: false,
    showIntersectionCoords: true,
    regionMode: "overlay",
    intersectionColor: "#7c3aed",
    intersectionOpacity: 0.3,
    intersectionStyle: "hatch",
    lockAspect: true,
    aspectMode: "range",
    customAspectRatio: 4 / 3,
    marginMm: 18,
    pdfGraphOnly: true,
    pdfGraphWidthMm: 120,
    pdfAspectMode: "graph",
    pdfCustomAspectRatio: 16 / 9,
  };
}

/**
 * 初回起動時に読み込むサンプルプロジェクト。
 * 駿台の研究問題18.1（曲線 y=x² と単位正方形が共有点をもつ点(a,b)の存在範囲）を、
 * 4つの場合分けを or で結合した1つの領域として再現し、点描で塗る。
 * 各境界曲線には自由配置できる数式ラベルを添える。
 */
export function sampleProject(): Project {
  // 境界曲線の式（a→x, b→y とみなす）
  const boundaries = [
    "y = (x+1)^2 - 1",
    "y = x^2",
    "y = (x+1)^2",
    "y = x^2 - 1",
  ].map((input, i) => {
    const e = defaultExprItem(input, i + 1);
    e.color = "#3a4150";
    e.lineWidth = 1.4;
    e.visible = false; // 既定では非表示（領域の輪郭を主役にする）
    return e;
  });

  // 存在範囲（4つの場合分けの和集合）
  const region = defaultExprItem(
    "(x <= -1 and (x+1)^2-1 <= y and y <= x^2) or " +
      "(-1 <= x <= -1/2 and -1 <= y <= x^2) or " +
      "(-1/2 <= x <= 0 and -1 <= y <= (x+1)^2) or " +
      "(x >= 0 and x^2-1 <= y <= (x+1)^2)",
    0,
  );
  region.name = "点(a,b)の存在範囲";
  region.color = "#1f2430";
  region.lineWidth = 1.6;
  region.fillColor = "#2f3646";
  region.fillOpacity = 0.5;
  region.fillStyle = "dot";

  const labels: MathLabel[] = [
    defaultLabel("b=(a+1)^2-1", -2.85, 1.55),
    defaultLabel("b=a^2", -2.75, 3.1),
    defaultLabel("b=(a+1)^2", 0.15, 4.35),
    defaultLabel("b=a^2-1", 1.15, 0.05),
  ];

  return {
    version: 1,
    appName: "MathGraph PDF Studio",
    expressions: [region, ...boundaries],
    points: [],
    labels,
    range: { xmin: -3, xmax: 2.2, ymin: -1.6, ymax: 4.8, xstep: 1, ystep: 1 },
    paper: {
      ...defaultPaper(),
      title: "曲線と正方形の共有点 — 点(a,b)の存在範囲",
      subtitle: "",
      problemNumber: "問題 18.1",
      caption: "",
      showLegend: false,
      // この問題の座標は (a, b) なので軸名を a, b にする
      axisLabelX: "a",
      axisLabelY: "b",
    },
  };
}

export function serializeProject(p: Project): string {
  return JSON.stringify(p, null, 2);
}

const num = (v: unknown, fallback: number): number =>
  typeof v === "number" && Number.isFinite(v) ? v : fallback;
const str = (v: unknown, fallback: string): string =>
  typeof v === "string" ? v : fallback;
const bool = (v: unknown, fallback: boolean): boolean =>
  typeof v === "boolean" ? v : fallback;

/**
 * プロジェクトJSONを読み込む。壊れたファイルでも落ちないよう
 * フィールドごとに検証して既定値で補完する。
 */
export function deserializeProject(
  text: string,
): { ok: true; project: Project } | { ok: false; message: string } {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { ok: false, message: "JSONとして読み込めませんでした。ファイルが壊れている可能性があります。" };
  }
  if (typeof raw !== "object" || raw === null) {
    return { ok: false, message: "プロジェクトファイルの形式が正しくありません。" };
  }
  const r = raw as Record<string, unknown>;
  if (!Array.isArray(r.expressions)) {
    return { ok: false, message: "プロジェクトファイルの形式が正しくありません（expressions がありません）。" };
  }

  const dp = defaultPaper();
  const dr = defaultRange();
  const paperRaw = (typeof r.paper === "object" && r.paper !== null ? r.paper : {}) as Record<string, unknown>;
  const rangeRaw = (typeof r.range === "object" && r.range !== null ? r.range : {}) as Record<string, unknown>;

  const expressions: ExprItem[] = r.expressions.map((e, i) => {
    const er = (typeof e === "object" && e !== null ? e : {}) as Record<string, unknown>;
    const base = defaultExprItem(str(er.input, ""), i);
    return {
      ...base,
      id: str(er.id, base.id) || base.id,
      name: str(er.name, ""),
      visible: bool(er.visible, true),
      color: str(er.color, base.color),
      lineWidth: Math.min(8, Math.max(0.5, num(er.lineWidth, 2))),
      lineStyle: er.lineStyle === "dashed" ? "dashed" : "solid",
      fillColor: str(er.fillColor, base.fillColor),
      fillOpacity: Math.min(1, Math.max(0, num(er.fillOpacity, 0.25))),
      fillStyle: toFillStyle(er.fillStyle, "solid"),
      tmin: num(er.tmin, 0),
      tmax: num(er.tmax, 2 * Math.PI),
    };
  });

  const points: PointItem[] = Array.isArray(r.points)
    ? r.points.map((p, i) => {
        const pr = (typeof p === "object" && p !== null ? p : {}) as Record<string, unknown>;
        return {
          id: str(pr.id, `pt${i}`),
          x: num(pr.x, 0),
          y: num(pr.y, 0),
          label: str(pr.label, ""),
          color: str(pr.color, "#dc2626"),
          visible: bool(pr.visible, true),
          showProjectionToXAxis: bool(pr.showProjectionToXAxis, false),
          showProjectionToYAxis: bool(pr.showProjectionToYAxis, false),
        };
      })
    : [];

  const labels: MathLabel[] = Array.isArray(r.labels)
    ? r.labels.map((l, i) => {
        const lr = (typeof l === "object" && l !== null ? l : {}) as Record<string, unknown>;
        return {
          id: str(lr.id, `lb${i}`),
          latex: str(lr.latex, ""),
          x: num(lr.x, 0),
          y: num(lr.y, 0),
          fontSize: Math.min(72, Math.max(8, num(lr.fontSize, 20))),
          color: str(lr.color, "#111318"),
          visible: bool(lr.visible, true),
        };
      })
    : [];

  const lockAspect = bool(paperRaw.lockAspect, dp.lockAspect);
  const aspectMode = toAspectMode(paperRaw.aspectMode, lockAspect);
  const project: Project = {
    version: 1,
    appName: "MathGraph PDF Studio",
    expressions,
    points,
    labels,
    range: {
      xmin: num(rangeRaw.xmin, dr.xmin),
      xmax: num(rangeRaw.xmax, dr.xmax),
      ymin: num(rangeRaw.ymin, dr.ymin),
      ymax: num(rangeRaw.ymax, dr.ymax),
      xstep: num(rangeRaw.xstep, dr.xstep),
      ystep: num(rangeRaw.ystep, dr.ystep),
    },
    paper: {
      orientation: paperRaw.orientation === "landscape" ? "landscape" : "portrait",
      title: str(paperRaw.title, dp.title),
      subtitle: str(paperRaw.subtitle, dp.subtitle),
      problemNumber: str(paperRaw.problemNumber, dp.problemNumber),
      caption: str(paperRaw.caption, dp.caption),
      showAxes: bool(paperRaw.showAxes, dp.showAxes),
      axisLabelX: str(paperRaw.axisLabelX, dp.axisLabelX),
      axisLabelY: str(paperRaw.axisLabelY, dp.axisLabelY),
      axisLabelO: str(paperRaw.axisLabelO, dp.axisLabelO),
      axisLabelSize: Math.min(40, Math.max(8, num(paperRaw.axisLabelSize, dp.axisLabelSize))),
      showTicks: bool(paperRaw.showTicks, dp.showTicks),
      showGrid: bool(paperRaw.showGrid, dp.showGrid),
      showLegend: bool(paperRaw.showLegend, dp.showLegend),
      legendFontSize: Math.min(40, Math.max(8, num(paperRaw.legendFontSize, dp.legendFontSize))),
      showIntersections: bool(paperRaw.showIntersections, dp.showIntersections),
      showIntersectionCoords: bool(paperRaw.showIntersectionCoords, dp.showIntersectionCoords),
      regionMode: paperRaw.regionMode === "intersection" ? "intersection" : "overlay",
      intersectionColor: str(paperRaw.intersectionColor, dp.intersectionColor),
      intersectionOpacity: Math.min(
        1,
        Math.max(0, num(paperRaw.intersectionOpacity, dp.intersectionOpacity)),
      ),
      intersectionStyle: toFillStyle(paperRaw.intersectionStyle, dp.intersectionStyle),
      lockAspect,
      aspectMode,
      customAspectRatio: clampAspectRatio(num(paperRaw.customAspectRatio, dp.customAspectRatio)),
      marginMm: Math.min(40, Math.max(5, num(paperRaw.marginMm, dp.marginMm))),
      pdfGraphOnly: bool(paperRaw.pdfGraphOnly, dp.pdfGraphOnly),
      pdfGraphWidthMm: Math.min(400, Math.max(20, num(paperRaw.pdfGraphWidthMm, dp.pdfGraphWidthMm))),
      pdfAspectMode: paperRaw.pdfAspectMode === "custom" ? "custom" : "graph",
      pdfCustomAspectRatio: clampAspectRatio(num(paperRaw.pdfCustomAspectRatio, dp.pdfCustomAspectRatio)),
    },
  };
  return { ok: true, project };
}

/** PDFなどのファイル名の既定値: タイトル_YYYYMMDD_HHMM */
export function defaultFileName(title: string, ext: string): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}`;
  const base = title.trim() || "graph";
  return `${base}_${stamp}.${ext}`;
}
