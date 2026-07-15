import type {
  ExprItem,
  FillStyle,
  MathLabel,
  PaperSettings,
  PointItem,
  ViewRange,
  ParseResult,
} from "../types";
import {
  sampleCurve,
  sampleCurveY,
  sampleParametric,
  explicitRegionPath,
  explicitRegionPathX,
  implicitRegion,
  polylinePath,
  type Transform,
} from "./graph";
import { mathLabelSvg, measureMathLabel, normalizeMathLabelLatex } from "./mathlabel";
import { findIntersections, fmtCoord, type Intersection } from "./intersections";

export interface RenderItem {
  item: ExprItem;
  parsed: ParseResult;
}

export interface SvgBuildOptions {
  width: number;
  height: number;
  /** 表示・領域モードなどの設定 */
  paper: PaperSettings;
  /** グラフ上に自由配置する数式ラベル */
  labels?: MathLabel[];
  /** 明示関数のサンプル数 */
  samples?: number;
  /** 陰関数評価のグリッド解像度 */
  implicitGrid?: number;
  /** id 衝突回避用プレフィックス */
  idPrefix?: string;
  /** 選択中のラベルID（プレビューで枠を表示。出力時は渡さない） */
  selectedLabelId?: string;
  /** 操作中（パン/ズーム）は塗りを簡略化して描画を軽くする */
  fastFill?: boolean;
  /** 交点の自動検出・表示を行うか */
  detectIntersections?: boolean;
}

export interface LabelLayout {
  id: string;
  /** ラベル左上のピクセル座標 */
  px: number;
  py: number;
  width: number;
  height: number;
  error: boolean;
}

export interface SvgBuildResult {
  svg: string;
  /** 式ID → 警告メッセージ（定義域外など） */
  warnings: Map<string, string>;
  /** ラベルの配置情報（ドラッグの当たり判定に使う） */
  labelLayouts: LabelLayout[];
  /** 検出した交点（UI一覧用） */
  intersections: Intersection[];
}

/** ラベルのフォントサイズを基準幅700pxで正規化する係数 */
const LABEL_REF_W = 700;

const esc = (s: string) =>
  s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");

const F = (n: number) => (Math.round(n * 100) / 100).toString();

/** 目盛りラベル用の数値整形（浮動小数点ノイズを除去） */
export function fmtNum(v: number): string {
  if (Math.abs(v) < 1e-12) return "0";
  const s = parseFloat(v.toFixed(10));
  return String(s);
}

const FONT = "IPAexGothic, 'Yu Gothic UI', 'Yu Gothic', sans-serif";
const AXIS_COLOR = "#1a1d24";
const GRID_COLOR = "#dfe3ea";
const TICK_COLOR = "#3a3f4a";

/** 45度の斜線パターンのパスを作る（dir=1: ／ 方向, dir=-1: ＼ 方向） */
function hatchLinesPath(W: number, H: number, spacing: number, dir: 1 | -1): string {
  let d = "";
  if (dir === 1) {
    for (let x0 = -H; x0 <= W; x0 += spacing) {
      d += `M${F(x0)} 0L${F(x0 + H)} ${F(H)}`;
    }
  } else {
    for (let x0 = 0; x0 <= W + H; x0 += spacing) {
      d += `M${F(x0)} 0L${F(x0 - H)} ${F(H)}`;
    }
  }
  return d;
}

/**
 * プロジェクトの内容から自己完結した SVG 文字列を生成する。
 * プレビュー・PNG・SVG・PDF 出力すべてで共通に使う。
 */
export function buildGraphSvg(
  items: RenderItem[],
  points: PointItem[],
  range: ViewRange,
  o: SvgBuildOptions,
): SvgBuildResult {
  const W = o.width;
  const H = o.height;
  const paper = o.paper;
  const warnings = new Map<string, string>();
  const samples = o.samples ?? 900;
  const gridN = o.implicitGrid ?? 140;
  const prefix = o.idPrefix ?? "g";
  const clipId = `${prefix}-clip`;
  const intersectionMode = paper.regionMode === "intersection";

  const { xmin, xmax, ymin, ymax, xstep, ystep } = range;
  const xr = xmax - xmin;
  const yr = ymax - ymin;
  const invalid =
    !Number.isFinite(xr) || !Number.isFinite(yr) || xr <= 0 || yr <= 0;

  const header = `<svg xmlns="http://www.w3.org/2000/svg" width="${F(W)}" height="${F(H)}" viewBox="0 0 ${F(W)} ${F(H)}" font-family="${FONT}">`;

  if (invalid) {
    return {
      svg:
        header +
        `<rect x="0" y="0" width="${F(W)}" height="${F(H)}" fill="#ffffff"/>` +
        `<text x="${F(W / 2)}" y="${F(H / 2)}" text-anchor="middle" font-size="14" fill="#b91c1c">範囲設定が不正です（最小値 &lt; 最大値 になるようにしてください）</text></svg>`,
      warnings,
      labelLayouts: [],
      intersections: [],
    };
  }

  const t: Transform = {
    px: (x) => ((x - xmin) / xr) * W,
    py: (y) => H - ((y - ymin) / yr) * H,
  };

  let s = header;
  s += `<defs><clipPath id="${clipId}"><rect x="0" y="0" width="${F(W)}" height="${F(H)}"/></clipPath></defs>`;
  // グラフ領域の背景（印刷を想定して白）
  s += `<rect x="0" y="0" width="${F(W)}" height="${F(H)}" fill="#ffffff"/>`;

  // --- 方眼 ---
  const xTicks: number[] = [];
  const yTicks: number[] = [];
  if (xstep > 0 && xr / xstep <= 400) {
    for (let k = Math.ceil(xmin / xstep - 1e-9); k <= Math.floor(xmax / xstep + 1e-9); k++) {
      xTicks.push(k * xstep);
    }
  }
  if (ystep > 0 && yr / ystep <= 400) {
    for (let k = Math.ceil(ymin / ystep - 1e-9); k <= Math.floor(ymax / ystep + 1e-9); k++) {
      yTicks.push(k * ystep);
    }
  }

  if (paper.showGrid) {
    let grid = "";
    for (const v of xTicks) {
      const x = F(t.px(v));
      grid += `<line x1="${x}" y1="0" x2="${x}" y2="${F(H)}"/>`;
    }
    for (const v of yTicks) {
      const y = F(t.py(v));
      grid += `<line x1="0" y1="${y}" x2="${F(W)}" y2="${y}"/>`;
    }
    s += `<g stroke="${GRID_COLOR}" stroke-width="1">${grid}</g>`;
  }

  // --- 塗りつぶしの出力ヘルパー（ベタ塗り / 斜線 / 網掛け / 点描） ---
  let regionUid = 0;
  const emitFill = (
    d: string,
    color: string,
    opacity: number,
    style: FillStyle,
    fillRule: "evenodd" | "nonzero" = "evenodd",
  ): string => {
    if (!d) return "";
    // 操作中は斜線・網掛け・点描をベタ塗りに簡略化（描画コスト削減）
    if (style === "solid" || o.fastFill) {
      const op = o.fastFill && style !== "solid" ? Math.min(0.5, opacity + 0.1) : opacity;
      return `<path d="${d}" fill="${color}" fill-opacity="${F(op)}" fill-rule="${fillRule}" stroke="none" clip-path="url(#${clipId})"/>`;
    }
    // 斜線・網掛け・点描: 領域を clipPath にしてパターンを重ねる（ベクターのままPDF化できる）
    const rid = `${prefix}-region${regionUid++}`;
    const lineOp = Math.min(1, opacity * 3);
    let out = `<defs><clipPath id="${rid}"><path d="${d}" clip-rule="${fillRule}"/></clipPath></defs>`;
    out += `<g clip-path="url(#${clipId})"><g clip-path="url(#${rid})">`;
    if (style === "dot") {
      // 点描: 格子状に小さな円を敷き詰める
      const sp = 8;
      const r = 1.25;
      let dots = "";
      for (let yy = sp / 2; yy < H; yy += sp) {
        for (let xx = sp / 2; xx < W; xx += sp) {
          dots += `<circle cx="${F(xx)}" cy="${F(yy)}" r="${r}"/>`;
        }
      }
      out += `<g fill="${color}" fill-opacity="${F(Math.min(1, opacity * 1.6))}">${dots}</g>`;
    } else {
      out += `<path d="${hatchLinesPath(W, H, 9, 1)}" stroke="${color}" stroke-width="1.2" stroke-opacity="${F(lineOp)}" fill="none"/>`;
      if (style === "crosshatch") {
        out += `<path d="${hatchLinesPath(W, H, 9, -1)}" stroke="${color}" stroke-width="1.2" stroke-opacity="${F(lineOp)}" fill="none"/>`;
      }
    }
    out += `</g></g>`;
    return out;
  };

  // --- 各式の塗り・線の生成 ---
  const fillParts: string[] = [];
  const strokeParts: string[] = [];
  let strokeClipUid = 0;
  const emitStroke = (
    d: string,
    item: ExprItem,
    clipGxy?: (x: number, y: number) => number,
  ): string => {
    if (!d) return "";
    const dash =
      item.lineStyle === "dashed"
        ? ` stroke-dasharray="${F(item.lineWidth * 4)} ${F(item.lineWidth * 3)}"`
        : "";
    const path = `<path d="${d}" fill="none" stroke="${item.color}" stroke-width="${item.lineWidth}" stroke-linejoin="round" stroke-linecap="round"${dash}/>`;
    if (!clipGxy) {
      return `<g clip-path="url(#${clipId})">${path}</g>`;
    }
    const clipRes = implicitRegion(clipGxy, range, t, gridN, true);
    if (!clipRes.fillPath) return "";
    const curveClipId = `${prefix}-curveclip${strokeClipUid++}`;
    return (
      `<defs><clipPath id="${curveClipId}"><path d="${clipRes.fillPath}" clip-rule="evenodd"/></clipPath></defs>` +
      `<g clip-path="url(#${clipId})"><g clip-path="url(#${curveClipId})">${path}</g></g>`
    );
  };
  // 共通部分モード用: 各不等式を「G(x,y) <= 0 が内側」の関数に正規化して集める
  const constraints: Array<(x: number, y: number) => number> = [];

  for (const { item, parsed } of items) {
    if (!item.visible || !parsed.ok) continue;

    let fillD = "";
    let strokeD = "";
    let fillRule: "evenodd" | "nonzero" = "evenodd";

    if (parsed.kind === "explicit-y" && parsed.fx) {
      const fx = parsed.fx;
      const curve = sampleCurve(fx, range, samples);
      if (curve.allInvalid) {
        warnings.set(item.id, "定義域外の値が含まれています。表示範囲内に定義域がありません。");
      }
      for (const seg of curve.segments) strokeD += polylinePath(seg, t);
      if (parsed.isInequality) {
        const above = parsed.rel === ">" || parsed.rel === ">=";
        constraints.push(
          above ? (x, y) => fx(x) - y : (x, y) => y - fx(x),
        );
        if (!intersectionMode) {
          fillD = explicitRegionPath(curve, range, above, t);
        }
      }
    } else if (parsed.kind === "explicit-x" && parsed.xconst !== undefined) {
      const c = parsed.xconst;
      if (c >= xmin && c <= xmax) {
        const x = F(t.px(c));
        strokeD = `M${x} 0L${x} ${F(H)}`;
      }
      if (parsed.isInequality) {
        const right = parsed.rel === ">" || parsed.rel === ">=";
        constraints.push(right ? (x) => c - x : (x) => x - c);
        if (!intersectionMode) {
          const x0 = Math.max(xmin, Math.min(xmax, right ? c : xmin));
          const x1 = Math.max(xmin, Math.min(xmax, right ? xmax : c));
          if (x1 > x0) {
            fillD = `M${F(t.px(x0))} 0L${F(t.px(x1))} 0L${F(t.px(x1))} ${F(H)}L${F(t.px(x0))} ${F(H)}Z`;
          }
        }
      }
    } else if (parsed.kind === "explicit-x-fn" && parsed.fy) {
      const fy = parsed.fy;
      const curve = sampleCurveY(fy, range, samples);
      if (curve.allInvalid) {
        warnings.set(item.id, "定義域外の値が含まれています。表示範囲内に定義域がありません。");
      }
      for (const seg of curve.segments) strokeD += polylinePath(seg, t);
      if (parsed.isInequality && parsed.gxy) {
        constraints.push(parsed.gxy);
        if (!intersectionMode) {
          const right = parsed.rel === ">" || parsed.rel === ">=";
          fillD = explicitRegionPathX(curve, range, right, t);
        }
      }
    } else if (parsed.kind === "parametric" && parsed.xt && parsed.yt) {
      const tmin = Number.isFinite(item.tmin) ? item.tmin : 0;
      const tmax = Number.isFinite(item.tmax) ? item.tmax : 2 * Math.PI;
      if (tmax > tmin) {
        const curve = sampleParametric(parsed.xt, parsed.yt, tmin, tmax, range, samples * 2);
        if (curve.allInvalid) {
          warnings.set(item.id, "定義域外の値が含まれています。t の範囲を確認してください。");
        }
        for (const seg of curve.segments) strokeD += polylinePath(seg, t);
      } else {
        warnings.set(item.id, "t の範囲を確認してください（最小値 < 最大値）。");
      }
    } else if (parsed.kind === "implicit" && parsed.gxy) {
      const needFill = parsed.isInequality && !intersectionMode;
      // 結合Gの min/max 合成で and/or 領域を一括評価する（union の境界が滑らかで、
      // 場合分けの継ぎ目に切れ込みが出ない。重なりのある or も正しく合併になる）。
      const res = implicitRegion(parsed.gxy, range, t, gridN, needFill);
      strokeD = res.boundaryPath;
      if (needFill) fillD = res.fillPath;
      if (!res.any) {
        warnings.set(
          item.id,
          "表示範囲内にグラフがありません。範囲設定を確認してください。",
        );
      }
      if (parsed.isInequality) constraints.push(parsed.gxy);
    }

    if (fillD) {
      fillParts.push(
        emitFill(fillD, item.fillColor, item.fillOpacity, item.fillStyle, fillRule),
      );
    }
    if (strokeD) {
      strokeParts.push(emitStroke(strokeD, item, parsed.clipGxy));
    }
  }

  // --- 共通部分モード: すべての不等式を同時に満たす領域だけを塗る ---
  if (intersectionMode && constraints.length > 0) {
    const combined = (x: number, y: number): number => {
      let m = -Infinity;
      for (const g of constraints) {
        const v = g(x, y);
        if (Number.isNaN(v)) return NaN;
        if (v > m) m = v;
      }
      return m;
    };
    const res = implicitRegion(combined, range, t, gridN, true);
    fillParts.unshift(
      emitFill(
        res.fillPath,
        paper.intersectionColor,
        paper.intersectionOpacity,
        paper.intersectionStyle,
      ),
    );
  }

  // 塗り（下層）→ 曲線・境界線（上層）
  s += fillParts.join("");
  s += strokeParts.join("");

  // --- 座標軸 ---
  const hasXAxis = ymin < 0 && ymax > 0;
  const hasYAxis = xmin < 0 && xmax > 0;
  if (paper.showAxes) {
    const y0 = t.py(0);
    const x0 = t.px(0);
    const axisFs = paper.axisLabelSize * (W / LABEL_REF_W);

    // 軸ラベルを MathJax で数式フォント組版する（未対応時は斜体テキストで代替）。
    // ax/ay は配置の基準点、alignX/alignY は基準点に対する寄せ方。
    const axisLabel = (
      latex: string,
      anchorX: number,
      anchorY: number,
      alignX: "start" | "end",
      alignY: "top" | "bottom",
    ): string => {
      if (!latex.trim()) return "";
      const m = measureMathLabel(latex, axisFs);
      if (m.error || m.width === 0) {
        const anchor = alignX === "end" ? "end" : "start";
        const dy = alignY === "bottom" ? 0 : axisFs;
        return `<text x="${F(anchorX)}" y="${F(anchorY + dy * 0.8)}" font-size="${F(axisFs)}" font-style="italic" text-anchor="${anchor}" fill="${AXIS_COLOR}">${esc(latex)}</text>`;
      }
      const cx = alignX === "end" ? anchorX - m.width : anchorX;
      const cy = alignY === "bottom" ? anchorY - m.height : anchorY;
      const r = mathLabelSvg(latex, cx, cy, axisFs, AXIS_COLOR, `${prefix}ax${latex}`);
      return r.svg;
    };

    let ax = "";
    if (hasXAxis) {
      ax += `<line x1="0" y1="${F(y0)}" x2="${F(W)}" y2="${F(y0)}" stroke="${AXIS_COLOR}" stroke-width="1.6"/>`;
      ax += `<path d="M${F(W - 11)} ${F(y0 - 4.5)}L${F(W)} ${F(y0)}L${F(W - 11)} ${F(y0 + 4.5)}Z" fill="${AXIS_COLOR}"/>`;
      ax += axisLabel(paper.axisLabelX, W - 13, y0 - 6, "end", "bottom");
    }
    if (hasYAxis) {
      ax += `<line x1="${F(x0)}" y1="0" x2="${F(x0)}" y2="${F(H)}" stroke="${AXIS_COLOR}" stroke-width="1.6"/>`;
      ax += `<path d="M${F(x0 - 4.5)} ${F(11)}L${F(x0)} 0L${F(x0 + 4.5)} ${F(11)}Z" fill="${AXIS_COLOR}"/>`;
      ax += axisLabel(paper.axisLabelY, x0 + 8, 5, "start", "top");
    }

    // 原点の名前は座標軸が表示されていれば常に描く
    if (hasXAxis && hasYAxis) {
      ax += axisLabel(paper.axisLabelO, x0 - 6, y0 + 5, "end", "top");
    }
    // 目盛り（目盛り線・数値）は showTicks が有効なときだけ表示
    if (paper.showTicks) {
      if (hasXAxis) {
        for (const v of xTicks) {
          if (Math.abs(v) < 1e-12) continue;
          const x = t.px(v);
          ax += `<line x1="${F(x)}" y1="${F(y0 - 3.5)}" x2="${F(x)}" y2="${F(y0 + 3.5)}" stroke="${AXIS_COLOR}" stroke-width="1.2"/>`;
          ax += `<text x="${F(x)}" y="${F(y0 + 17)}" font-size="12" text-anchor="middle" fill="${TICK_COLOR}">${esc(fmtNum(v))}</text>`;
        }
      }
      if (hasYAxis) {
        for (const v of yTicks) {
          if (Math.abs(v) < 1e-12) continue;
          const y = t.py(v);
          ax += `<line x1="${F(x0 - 3.5)}" y1="${F(y)}" x2="${F(x0 + 3.5)}" y2="${F(y)}" stroke="${AXIS_COLOR}" stroke-width="1.2"/>`;
          ax += `<text x="${F(x0 - 7)}" y="${F(y + 4)}" font-size="12" text-anchor="end" fill="${TICK_COLOR}">${esc(fmtNum(v))}</text>`;
        }
      }
    }
    s += `<g clip-path="url(#${clipId})">${ax}</g>`;
  }

  // --- 交点の自動検出 ---
  const intersections: Intersection[] = o.detectIntersections
    ? findIntersections(items, range)
    : [];
  if (intersections.length > 0) {
    const fs = Math.max(10, 12 * (W / LABEL_REF_W));
    let ig = "";
    for (const p of intersections) {
      const x = t.px(p.x);
      const y = t.py(p.y);
      if (x < -6 || x > W + 6 || y < -6 || y > H + 6) continue;
      ig += `<circle cx="${F(x)}" cy="${F(y)}" r="${F(fs * 0.36)}" fill="#e11d48" stroke="#ffffff" stroke-width="1.4"/>`;
      if (paper.showIntersectionCoords) {
        const label = `(${fmtCoord(p.x)}, ${fmtCoord(p.y)})`;
        // 点の右上に配置。右端に近ければ左寄せにする
        const anchor = x > W - 90 ? "end" : "start";
        const ox = anchor === "end" ? -fs * 0.5 : fs * 0.5;
        ig += `<text x="${F(x + ox)}" y="${F(y - fs * 0.5)}" font-size="${F(fs)}" text-anchor="${anchor}" fill="#9f1239" stroke="#ffffff" stroke-width="2.6" paint-order="stroke" style="paint-order:stroke">${esc(label)}</text>`;
      }
    }
    s += `<g clip-path="url(#${clipId})">${ig}</g>`;
  }

  // --- 点（交点・重要点） ---
  for (const p of points) {
    if (!p.visible) continue;
    const x = t.px(p.x);
    const y = t.py(p.y);
    if (x < -10 || x > W + 10 || y < -10 || y > H + 10) continue;
    let projection = "";
    if (p.showProjectionToXAxis) {
      projection += `<line x1="${F(x)}" y1="${F(y)}" x2="${F(x)}" y2="${F(t.py(0))}"/>`;
    }
    if (p.showProjectionToYAxis) {
      projection += `<line x1="${F(x)}" y1="${F(y)}" x2="${F(t.px(0))}" y2="${F(y)}"/>`;
    }
    if (projection) {
      s += `<g clip-path="url(#${clipId})" stroke="${p.color}" stroke-width="1.25" stroke-opacity="0.72" stroke-dasharray="5 3" fill="none">${projection}</g>`;
    }
    s += `<circle cx="${F(x)}" cy="${F(y)}" r="4" fill="${p.color}" stroke="#ffffff" stroke-width="1.5"/>`;
    if (p.label) {
      s += `<text x="${F(x + 7)}" y="${F(y - 7)}" font-size="12.5" fill="${AXIS_COLOR}">${esc(p.label)}</text>`;
    }
  }

  // --- 凡例（文字サイズは基準幅700pxで正規化して指定可能） ---
  if (paper.showLegend) {
    const rows = items.filter(({ item, parsed }) => item.visible && parsed.ok);
    if (rows.length > 0) {
      const fs = paper.legendFontSize * (W / LABEL_REF_W);
      const textW = (str: string) => {
        let w = 0;
        for (const ch of str) w += ch.codePointAt(0)! > 0x2e80 ? fs : fs * 0.56;
        return w;
      };
      const pad = fs * 0.5;
      const swatch = fs * 2.1;
      const gap = fs * 0.65;
      const labels = rows.map(({ item }) => item.name.trim() || item.input);
      const boxW = Math.min(
        W * 0.7,
        Math.max(...labels.map(textW)) + swatch + gap + pad * 2,
      );
      const rowH = fs * 1.7;
      const boxH = rows.length * rowH + pad * 2;
      const bx = W - boxW - 10;
      const by = 10;
      s += `<rect x="${F(bx)}" y="${F(by)}" width="${F(boxW)}" height="${F(boxH)}" rx="${F(fs * 0.35)}" fill="#ffffff" fill-opacity="0.92" stroke="#aab0bc" stroke-width="1"/>`;
      rows.forEach(({ item }, i) => {
        const cy = by + pad + i * rowH + rowH / 2;
        const dash =
          item.lineStyle === "dashed"
            ? ` stroke-dasharray="${F(item.lineWidth * 4)} ${F(item.lineWidth * 3)}"`
            : "";
        s += `<line x1="${F(bx + pad)}" y1="${F(cy)}" x2="${F(bx + pad + swatch)}" y2="${F(cy)}" stroke="${item.color}" stroke-width="${item.lineWidth}"${dash}/>`;
        s += `<text x="${F(bx + pad + swatch + gap)}" y="${F(cy + fs * 0.34)}" font-size="${F(fs)}" fill="#22262e">${esc(labels[i].length > 44 ? labels[i].slice(0, 44) + "…" : labels[i])}</text>`;
      });
    }
  }

  // --- 数式ラベル（MathJax でベクター組版、最前面） ---
  const labelLayouts: LabelLayout[] = [];
  const labels = o.labels ?? [];
  if (labels.length > 0) {
    const scale = W / LABEL_REF_W;
    let labelSvg = "";
    for (const lb of labels) {
      if (!lb.visible) continue;
      const px = t.px(lb.x);
      const py = t.py(lb.y);
      const eff = lb.fontSize * scale;
      const r = mathLabelSvg(lb.latex, px, py, eff, lb.color, `${prefix}${lb.id}`);
      if (r.error || !r.svg) {
        // MathJax読込中や構文エラーでもラベルを消さない。準備完了時は
        // mathReady依存で再描画され、ベクター数式へ自動的に置き換わる。
        const fallback = normalizeMathLabelLatex(lb.latex);
        const fallbackWidth = Math.max(eff, fallback.length * eff * 0.58);
        const fallbackHeight = Math.max(10, eff * 1.15);
        labelSvg += `<text data-math-label-fallback="${esc(lb.id)}" x="${F(px)}" y="${F(py + eff)}" font-size="${F(eff)}" font-style="italic" fill="${esc(lb.color)}">${esc(fallback || "数式ラベル")}</text>`;
        if (o.selectedLabelId === lb.id) {
          labelSvg += `<rect x="${F(px - 3)}" y="${F(py - 3)}" width="${F(fallbackWidth + 6)}" height="${F(fallbackHeight + 6)}" fill="none" stroke="#f59e0b" stroke-width="1.2" stroke-dasharray="4 3"/>`;
        }
        labelLayouts.push({ id: lb.id, px, py, width: fallbackWidth, height: fallbackHeight, error: false });
        continue;
      }
      labelSvg += r.svg;
      // 選択中ラベルには破線の枠を表示（プレビュー用）
      if (o.selectedLabelId === lb.id) {
        const pad = 3;
        labelSvg += `<rect x="${F(px - pad)}" y="${F(py - pad)}" width="${F(r.width + pad * 2)}" height="${F(r.height + pad * 2)}" fill="none" stroke="#22d3ee" stroke-width="1.2" stroke-dasharray="4 3"/>`;
      }
      labelLayouts.push({ id: lb.id, px, py, width: r.width, height: r.height, error: false });
    }
    if (labelSvg) {
      s += `<g clip-path="url(#${clipId})">${labelSvg}</g>`;
    }
  }

  // 枠線
  s += `<rect x="0.5" y="0.5" width="${F(W - 1)}" height="${F(H - 1)}" fill="none" stroke="#b6bcc8" stroke-width="1"/>`;
  s += `</svg>`;
  return { svg: s, warnings, labelLayouts, intersections };
}
