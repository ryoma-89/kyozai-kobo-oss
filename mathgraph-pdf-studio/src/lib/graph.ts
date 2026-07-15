import type { ViewRange } from "../types";

/** グラフ座標 → ピクセル座標の変換 */
export interface Transform {
  px: (x: number) => number;
  py: (y: number) => number;
}

export interface Pt {
  x: number;
  y: number;
}

const F = (n: number) => (Math.round(n * 100) / 100).toString();

/** 点列を SVG パス (M/L) に変換 */
export function polylinePath(pts: Pt[], t: Transform): string {
  if (pts.length < 2) return "";
  let d = `M${F(t.px(pts[0].x))} ${F(t.py(pts[0].y))}`;
  for (let i = 1; i < pts.length; i++) {
    d += `L${F(t.px(pts[i].x))} ${F(t.py(pts[i].y))}`;
  }
  return d;
}

/** 点列を閉じた SVG パスに変換 */
export function polygonPath(pts: Pt[], t: Transform): string {
  const d = polylinePath(pts, t);
  return d ? d + "Z" : "";
}

// ---------------------------------------------------------------------------
// 明示関数 y = f(x) のサンプリング
// ---------------------------------------------------------------------------

export interface CurveSamples {
  /** 不連続点で分割された点列（グラフ座標） */
  segments: Pt[][];
  /** 全サンプル点が無効（定義域外など）だったか */
  allInvalid: boolean;
}

/**
 * f(x) を x 方向にサンプリングし、漸近線・定義域の切れ目でセグメントを分割する。
 * y 値は表示範囲の少し外側でクランプする（SVG 側で clip する前提）。
 */
export function sampleCurve(
  fx: (x: number) => number,
  range: ViewRange,
  samples = 1000,
): CurveSamples {
  const { xmin, xmax, ymin, ymax } = range;
  const yr = ymax - ymin;
  const clampLo = ymin - yr * 0.5;
  const clampHi = ymax + yr * 0.5;
  // これを超えたら「発散」とみなして線を切る
  const huge = yr * 20;
  // 隣接サンプル間の跳びがこれを超えたら不連続とみなす
  const jump = yr * 8;

  const segments: Pt[][] = [];
  let cur: Pt[] = [];
  let prevRaw = NaN;
  let validCount = 0;

  const flush = () => {
    if (cur.length >= 2) segments.push(cur);
    cur = [];
  };

  for (let i = 0; i <= samples; i++) {
    const x = xmin + ((xmax - xmin) * i) / samples;
    const raw = fx(x);
    if (Number.isNaN(raw) || Math.abs(raw - (ymin + ymax) / 2) > huge) {
      // 定義域外 or 発散 → 線を切る（発散方向の端点はクランプして残す）
      if (!Number.isNaN(raw) && cur.length > 0) {
        cur.push({ x, y: Math.min(clampHi, Math.max(clampLo, raw)) });
        validCount++;
      }
      flush();
      prevRaw = NaN;
      continue;
    }
    validCount++;
    // 符号をまたぐ大ジャンプ（漸近線）で分割
    if (!Number.isNaN(prevRaw) && Math.abs(raw - prevRaw) > jump) {
      // 切断の両側をクランプ値で伸ばして漸近線らしく見せる
      flush();
    }
    cur.push({ x, y: Math.min(clampHi, Math.max(clampLo, raw)) });
    prevRaw = raw;
  }
  flush();

  return { segments, allInvalid: validCount === 0 };
}

/**
 * x = f(y) を y 方向にサンプリングする（sampleCurve の x/y 入れ替え版）。
 * 各点は {x: f(y), y}。x 方向の不連続・発散で分割する。
 */
export function sampleCurveY(
  fy: (y: number) => number,
  range: ViewRange,
  samples = 1000,
): CurveSamples {
  const { xmin, xmax, ymin, ymax } = range;
  const xr = xmax - xmin;
  const clampLo = xmin - xr * 0.5;
  const clampHi = xmax + xr * 0.5;
  const huge = xr * 20;
  const jump = xr * 8;

  const segments: Pt[][] = [];
  let cur: Pt[] = [];
  let prevRaw = NaN;
  let validCount = 0;
  const flush = () => {
    if (cur.length >= 2) segments.push(cur);
    cur = [];
  };

  for (let i = 0; i <= samples; i++) {
    const y = ymin + ((ymax - ymin) * i) / samples;
    const raw = fy(y);
    if (Number.isNaN(raw) || Math.abs(raw - (xmin + xmax) / 2) > huge) {
      if (!Number.isNaN(raw) && cur.length > 0) {
        cur.push({ x: Math.min(clampHi, Math.max(clampLo, raw)), y });
        validCount++;
      }
      flush();
      prevRaw = NaN;
      continue;
    }
    validCount++;
    if (!Number.isNaN(prevRaw) && Math.abs(raw - prevRaw) > jump) flush();
    cur.push({ x: Math.min(clampHi, Math.max(clampLo, raw)), y });
    prevRaw = raw;
  }
  flush();
  return { segments, allInvalid: validCount === 0 };
}

/**
 * 媒介変数表示 (x(t), y(t)) を t 方向にサンプリングする。
 * 表示範囲に対して大きく跳んだ箇所や NaN で分割する。
 */
export function sampleParametric(
  xt: (t: number) => number,
  yt: (t: number) => number,
  tmin: number,
  tmax: number,
  range: ViewRange,
  samples = 2000,
): CurveSamples {
  const { xmin, xmax, ymin, ymax } = range;
  const xr = xmax - xmin;
  const yr = ymax - ymin;
  const clampXLo = xmin - xr * 0.5, clampXHi = xmax + xr * 0.5;
  const clampYLo = ymin - yr * 0.5, clampYHi = ymax + yr * 0.5;
  const jumpX = xr * 4;
  const jumpY = yr * 4;

  const segments: Pt[][] = [];
  let cur: Pt[] = [];
  let prevX = NaN;
  let prevY = NaN;
  let validCount = 0;
  const flush = () => {
    if (cur.length >= 2) segments.push(cur);
    cur = [];
  };
  const span = tmax - tmin;
  for (let i = 0; i <= samples; i++) {
    const t = tmin + (span * i) / samples;
    const rx = xt(t);
    const ry = yt(t);
    if (Number.isNaN(rx) || Number.isNaN(ry)) {
      flush();
      prevX = NaN;
      prevY = NaN;
      continue;
    }
    validCount++;
    if (
      !Number.isNaN(prevX) &&
      (Math.abs(rx - prevX) > jumpX || Math.abs(ry - prevY) > jumpY)
    ) {
      flush();
    }
    cur.push({
      x: Math.min(clampXHi, Math.max(clampXLo, rx)),
      y: Math.min(clampYHi, Math.max(clampYLo, ry)),
    });
    prevX = rx;
    prevY = ry;
  }
  flush();
  return { segments, allInvalid: validCount === 0 };
}

/**
 * x REL f(y) 型不等式の塗りつぶしパスを作る。
 * right=true なら曲線から右端まで、false なら左端まで塗る。
 */
export function explicitRegionPathX(
  curve: CurveSamples,
  range: ViewRange,
  right: boolean,
  t: Transform,
): string {
  const xr = range.xmax - range.xmin;
  const edge = right ? range.xmax + xr * 0.5 : range.xmin - xr * 0.5;
  let d = "";
  for (const seg of curve.segments) {
    if (seg.length < 2) continue;
    const pts: Pt[] = [
      ...seg,
      { x: edge, y: seg[seg.length - 1].y },
      { x: edge, y: seg[0].y },
    ];
    d += polygonPath(pts, t);
  }
  return d;
}

/**
 * y REL f(x) 型不等式の塗りつぶしパスを作る。
 * above=true なら曲線から上端まで、false なら下端まで塗る。
 */
export function explicitRegionPath(
  curve: CurveSamples,
  range: ViewRange,
  above: boolean,
  t: Transform,
): string {
  const yr = range.ymax - range.ymin;
  const edge = above ? range.ymax + yr * 0.5 : range.ymin - yr * 0.5;
  let d = "";
  for (const seg of curve.segments) {
    if (seg.length < 2) continue;
    const pts: Pt[] = [
      ...seg,
      { x: seg[seg.length - 1].x, y: edge },
      { x: seg[0].x, y: edge },
    ];
    d += polygonPath(pts, t);
  }
  return d;
}

// ---------------------------------------------------------------------------
// 陰関数 G(x,y) <= 0 のマーチングスクエア法
// ---------------------------------------------------------------------------

export interface ImplicitResult {
  /** 領域塗りつぶし用パス（fill-rule: evenodd 用） */
  fillPath: string;
  /** 境界線（G=0 の等高線）用パス */
  boundaryPath: string;
  /** 領域・境界が1つでも存在するか */
  any: boolean;
}

interface Chord {
  startKey: string;
  endKey: string;
  p1: Pt; // グリッドインデックス座標 (u, v)
  p2: Pt;
}

/**
 * G(x,y) <= 0 の領域をマーチングスクエア法で抽出する。
 * 返すパスは閉ループ＋表示範囲の縁で閉じたポリゴンで構成され、
 * 半透明塗りでも継ぎ目が出ない。
 */
export function implicitRegion(
  gxy: (x: number, y: number) => number,
  range: ViewRange,
  t: Transform,
  n = 140,
  fillNeeded = true,
): ImplicitResult {
  const nx = n;
  const ny = n;
  const { xmin, xmax, ymin, ymax } = range;
  const dx = (xmax - xmin) / nx;
  const dy = (ymax - ymin) / ny;

  // グリッド値を評価
  const vals = new Float64Array((nx + 1) * (ny + 1));
  let maxAbs = 1;
  for (let j = 0; j <= ny; j++) {
    for (let i = 0; i <= nx; i++) {
      const v = gxy(xmin + i * dx, ymin + j * dy);
      vals[j * (nx + 1) + i] = v;
      if (Number.isFinite(v) && Math.abs(v) > maxAbs) maxAbs = Math.abs(v);
    }
  }
  // NaN（定義域外）は「外側」の大きな値に置換。ちょうど 0 は内側に寄せる。
  const eps = maxAbs * 1e-9 || 1e-12;
  const outside = maxAbs * 2;
  for (let k = 0; k < vals.length; k++) {
    const v = vals[k];
    if (Number.isNaN(v)) vals[k] = outside;
    else if (v === 0) vals[k] = -eps;
  }

  const at = (i: number, j: number) => vals[j * (nx + 1) + i];
  const inside = (i: number, j: number) => at(i, j) <= 0;

  // 各セルからコード（等高線の弦）を抽出。向きは「内側が左」になっている。
  const chords: Chord[] = [];
  const startMap = new Map<string, number>();

  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx; i++) {
      const cA = inside(i, j);
      const cB = inside(i + 1, j);
      const cC = inside(i + 1, j + 1);
      const cD = inside(i, j + 1);
      if (cA === cB && cB === cC && cC === cD) continue; // 全部内側 or 全部外側

      // セルの辺を反時計回りに走査: 下→右→上→左
      const walk: Array<{
        ia: number; ja: number; ib: number; jb: number; key: string;
      }> = [
        { ia: i, ja: j, ib: i + 1, jb: j, key: `H${i},${j}` },
        { ia: i + 1, ja: j, ib: i + 1, jb: j + 1, key: `V${i + 1},${j}` },
        { ia: i + 1, ja: j + 1, ib: i, jb: j + 1, key: `H${i},${j + 1}` },
        { ia: i, ja: j + 1, ib: i, jb: j, key: `V${i},${j}` },
      ];

      const crossings: Array<{ pt: Pt; key: string; isExit: boolean }> = [];
      for (const e of walk) {
        const inA = inside(e.ia, e.ja);
        const inB = inside(e.ib, e.jb);
        if (inA === inB) continue;
        const va = at(e.ia, e.ja);
        const vb = at(e.ib, e.jb);
        let tt = va / (va - vb);
        tt = Math.min(1 - 1e-6, Math.max(1e-6, tt));
        crossings.push({
          pt: {
            x: e.ia + (e.ib - e.ia) * tt,
            y: e.ja + (e.jb - e.ja) * tt,
          },
          key: e.key,
          isExit: inA,
        });
      }
      // 交点は exit/entry が交互に並ぶ。exit → 次の entry が1本の弦。
      const m = crossings.length;
      for (let k = 0; k < m; k++) {
        const c = crossings[k];
        if (!c.isExit) continue;
        const nx2 = crossings[(k + 1) % m];
        const idx = chords.length;
        chords.push({ startKey: c.key, endKey: nx2.key, p1: c.pt, p2: nx2.pt });
        startMap.set(c.key, idx);
      }
    }
  }

  // インデックス座標 → ピクセル座標
  const toPx = (p: Pt): Pt => ({
    x: t.px(xmin + p.x * dx),
    y: t.py(ymin + p.y * dy),
  });
  const pathOf = (pts: Pt[], close: boolean): string => {
    if (pts.length < 2) return "";
    let d = `M${F(pts[0].x)} ${F(pts[0].y)}`;
    for (let k = 1; k < pts.length; k++) d += `L${F(pts[k].x)} ${F(pts[k].y)}`;
    return close ? d + "Z" : d;
  };

  // 表示範囲のピクセル境界（縁の点は動かさないための判定に使う）
  const pxW = t.px(xmax);
  const pxH = t.py(ymin);
  const onEdge = (p: Pt): boolean => {
    const e = 0.75;
    return (
      p.x <= e || p.x >= pxW - e || p.y <= e || p.y >= pxH - e
    );
  };
  // 角（急な折れ）と表示範囲の縁の点は固定する、角保持つき Chaikin 平滑化
  const CORNER_COS = 0.55; // これより鋭い折れ（cosがこれ未満）は角として保持
  const isFixed = (cur: Pt[], i: number, closed: boolean): boolean => {
    const n = cur.length;
    if (!closed && (i === 0 || i === n - 1)) return true;
    if (onEdge(cur[i])) return true;
    const p = cur[i];
    const a = cur[(i - 1 + n) % n];
    const b = cur[(i + 1) % n];
    const v1x = p.x - a.x, v1y = p.y - a.y;
    const v2x = b.x - p.x, v2y = b.y - p.y;
    const l1 = Math.hypot(v1x, v1y) || 1;
    const l2 = Math.hypot(v2x, v2y) || 1;
    return (v1x * v2x + v1y * v2y) / (l1 * l2) < CORNER_COS;
  };
  const smoothChain = (pts: Pt[], closed: boolean, iters = 2): Pt[] => {
    if (pts.length < 4) return pts;
    let cur = pts;
    for (let it = 0; it < iters; it++) {
      const n = cur.length;
      const fixed = cur.map((_, i) => isFixed(cur, i, closed));
      const out: Pt[] = [];
      const edges = closed ? n : n - 1;
      if (!closed) out.push(cur[0]);
      for (let i = 0; i < edges; i++) {
        const p = cur[i];
        const q = cur[(i + 1) % n];
        // 各辺から、両端の固定状況に応じてカット点を出す
        out.push(
          fixed[i] ? p : { x: p.x * 0.75 + q.x * 0.25, y: p.y * 0.75 + q.y * 0.25 },
        );
        out.push(
          fixed[(i + 1) % n]
            ? q
            : { x: p.x * 0.25 + q.x * 0.75, y: p.y * 0.25 + q.y * 0.75 },
        );
      }
      if (!closed) out.push(cur[n - 1]);
      // 連続する重複点を除去
      const dedup: Pt[] = [];
      for (const p of out) {
        const last = dedup[dedup.length - 1];
        if (!last || Math.abs(last.x - p.x) > 0.01 || Math.abs(last.y - p.y) > 0.01) {
          dedup.push(p);
        }
      }
      cur = dedup;
    }
    return cur;
  };

  // 弦を連結してチェーンを作る
  const used = new Array<boolean>(chords.length).fill(false);
  const endSet = new Set<string>();
  for (const c of chords) endSet.add(c.endKey);

  interface Chain {
    pts: Pt[]; // ピクセル座標
    startIdx: Pt; // インデックス座標（境界パラメータ計算用）
    endIdx: Pt;
    closed: boolean;
  }
  const chains: Chain[] = [];

  const walkChain = (head: number): Chain => {
    const ptsIdx: Pt[] = [chords[head].p1, chords[head].p2];
    used[head] = true;
    let cur = head;
    for (;;) {
      const nextIdx = startMap.get(chords[cur].endKey);
      if (nextIdx === undefined || used[nextIdx]) {
        return {
          pts: ptsIdx.map(toPx),
          startIdx: ptsIdx[0],
          endIdx: ptsIdx[ptsIdx.length - 1],
          closed: nextIdx === head,
        };
      }
      used[nextIdx] = true;
      ptsIdx.push(chords[nextIdx].p2);
      cur = nextIdx;
    }
  };

  // 開いたチェーン（表示範囲の縁で終わるもの）から辿る
  for (let k = 0; k < chords.length; k++) {
    if (!used[k] && !endSet.has(chords[k].startKey)) {
      chains.push(walkChain(k));
    }
  }
  // 残りは閉ループ
  for (let k = 0; k < chords.length; k++) {
    if (!used[k]) {
      const ch = walkChain(k);
      ch.closed = true;
      chains.push(ch);
    }
  }

  let boundaryPath = "";
  for (const ch of chains) boundaryPath += pathOf(smoothChain(ch.pts, ch.closed), ch.closed);

  if (!fillNeeded) {
    return { fillPath: "", boundaryPath, any: chains.length > 0 };
  }

  // --- 塗りつぶしポリゴンの構築 ---
  let fillPath = "";
  const openChains = chains.filter((c) => !c.closed);
  for (const ch of chains) {
    if (ch.closed) fillPath += pathOf(smoothChain(ch.pts, true), true);
  }

  // 表示範囲の縁を反時計回りに一周するパラメータ s（インデックス単位）
  const P = 2 * nx + 2 * ny;
  const sOf = (p: Pt): number => {
    const du = p.x;
    const dv = p.y;
    if (dv < 1e-9) return du; // 下辺
    if (du > nx - 1e-9) return nx + dv; // 右辺
    if (dv > ny - 1e-9) return nx + ny + (nx - du); // 上辺
    return 2 * nx + ny + (ny - dv); // 左辺
  };
  const cornerS = [0, nx, nx + ny, 2 * nx + ny];
  const cornerPt: Pt[] = [
    { x: 0, y: 0 },
    { x: nx, y: 0 },
    { x: nx, y: ny },
    { x: 0, y: ny },
  ];

  if (openChains.length > 0) {
    const entries = openChains.map((c, idx) => ({
      idx,
      startS: sOf(c.startIdx),
      endS: sOf(c.endIdx),
      used: false,
    }));
    for (const start of entries) {
      if (start.used) continue;
      const poly: Pt[] = [];
      let cur = start;
      for (;;) {
        cur.used = true;
        poly.push(...openChains[cur.idx].pts);
        const s0 = cur.endS;
        // s0 から反時計回りに進んで最初に現れるチェーン始点を探す
        let best: typeof entries[number] | null = null;
        let bestDelta = Infinity;
        for (const e of entries) {
          if (e.used && e !== start) continue;
          let delta = e.startS - s0;
          if (delta <= 1e-9) delta += P;
          if (delta < bestDelta) {
            bestDelta = delta;
            best = e;
          }
        }
        if (!best) break;
        // s0 から best.startS までの間に通過する角を距離順に挿入
        const corners: Array<{ delta: number; pt: Pt }> = [];
        for (let c = 0; c < 4; c++) {
          let dc = cornerS[c] - s0;
          if (dc <= 1e-9) dc += P;
          if (dc < bestDelta) corners.push({ delta: dc, pt: cornerPt[c] });
        }
        corners.sort((a, b) => a.delta - b.delta);
        for (const c of corners) poly.push(toPx(c.pt));
        if (best === start) break;
        cur = best;
      }
      fillPath += pathOf(smoothChain(poly, true), true);
    }
  } else if (chains.length === 0) {
    // コードが1つもない → 全面が内側か外側か
    const ci = Math.floor(nx / 2);
    const cj = Math.floor(ny / 2);
    if (inside(ci, cj)) {
      fillPath += pathOf(
        [cornerPt[0], cornerPt[1], cornerPt[2], cornerPt[3]].map(toPx),
        true,
      );
    }
  }

  return { fillPath, boundaryPath, any: chains.length > 0 || fillPath !== "" };
}
