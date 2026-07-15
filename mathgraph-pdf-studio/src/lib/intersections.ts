import type { ParsedExpr, ViewRange } from "../types";
import type { RenderItem } from "./buildSvg";

export interface Intersection {
  x: number;
  y: number;
}

type Fn1 = (t: number) => number;

/** 区間 [a,b] で h の符号が変わる点を二分法で求める */
function bisect(h: Fn1, a: number, b: number, iter = 60): number {
  let fa = h(a);
  for (let i = 0; i < iter; i++) {
    const m = (a + b) / 2;
    const fm = h(m);
    if (fm === 0 || !Number.isFinite(fm)) return m;
    if (fa * fm < 0) {
      b = m;
    } else {
      a = m;
      fa = fm;
    }
  }
  return (a + b) / 2;
}

/**
 * h(t)=0 の解を [a,b] で走査して求める。
 * 不連続（漸近線）での偽の符号変化を避けるため、両端の値が過大なものは無視し、
 * 求めた解で |h| が十分小さいことを確認する。
 */
function scanRoots(h: Fn1, a: number, b: number, samples: number, huge: number): number[] {
  const roots: number[] = [];
  const step = (b - a) / samples;
  let prevX = a;
  let prev = h(a);
  for (let k = 1; k <= samples; k++) {
    const x = a + step * k;
    const cur = h(x);
    if (Number.isFinite(prev) && Number.isFinite(cur) && Math.abs(prev) < huge && Math.abs(cur) < huge) {
      if (prev === 0) {
        roots.push(prevX);
      } else if (prev * cur < 0) {
        const r = bisect(h, prevX, x);
        const scale = Math.max(Math.abs(prev), Math.abs(cur), 1);
        if (Math.abs(h(r)) < scale * 1e-3 + 1e-9) roots.push(r);
      }
    }
    prevX = x;
    prev = cur;
  }
  return roots;
}

/** 2つの陰関数 g1=0, g2=0 の交点を格子評価で近似する */
function implicitPair(
  g1: (x: number, y: number) => number,
  g2: (x: number, y: number) => number,
  range: ViewRange,
  out: Intersection[],
): void {
  const n = 160;
  const { xmin, xmax, ymin, ymax } = range;
  const dx = (xmax - xmin) / n;
  const dy = (ymax - ymin) / n;
  const v1 = new Float64Array((n + 1) * (n + 1));
  const v2 = new Float64Array((n + 1) * (n + 1));
  for (let j = 0; j <= n; j++) {
    for (let i = 0; i <= n; i++) {
      const x = xmin + i * dx;
      const y = ymin + j * dy;
      v1[j * (n + 1) + i] = g1(x, y);
      v2[j * (n + 1) + i] = g2(x, y);
    }
  }
  const crosses = (a: number, b: number, c: number, d: number): boolean => {
    let hasNeg = false;
    let hasPos = false;
    for (const v of [a, b, c, d]) {
      if (!Number.isFinite(v)) return false;
      if (v < 0) hasNeg = true;
      else if (v > 0) hasPos = true;
    }
    return hasNeg && hasPos;
  };
  const at = (v: Float64Array, i: number, j: number) => v[j * (n + 1) + i];
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      const a1 = at(v1, i, j), b1 = at(v1, i + 1, j), c1 = at(v1, i + 1, j + 1), d1 = at(v1, i, j + 1);
      const a2 = at(v2, i, j), b2 = at(v2, i + 1, j), c2 = at(v2, i + 1, j + 1), d2 = at(v2, i, j + 1);
      if (!crosses(a1, b1, c1, d1) || !crosses(a2, b2, c2, d2)) continue;
      // セル内を細分して g1^2+g2^2 が最小の点を採用
      const x0 = xmin + i * dx;
      const y0 = ymin + j * dy;
      let best = Infinity;
      let bx = x0 + dx / 2;
      let by = y0 + dy / 2;
      const sub = 6;
      for (let sj = 0; sj <= sub; sj++) {
        for (let si = 0; si <= sub; si++) {
          const x = x0 + (dx * si) / sub;
          const y = y0 + (dy * sj) / sub;
          const e1 = g1(x, y);
          const e2 = g2(x, y);
          if (!Number.isFinite(e1) || !Number.isFinite(e2)) continue;
          const e = e1 * e1 + e2 * e2;
          if (e < best) {
            best = e;
            bx = x;
            by = y;
          }
        }
      }
      const tol = (Math.abs(dx) + Math.abs(dy)) * 0.5;
      if (best < tol * tol) out.push({ x: bx, y: by });
    }
  }
}

function pairIntersect(
  p: ParsedExpr,
  q: ParsedExpr,
  range: ViewRange,
  out: Intersection[],
): void {
  const { xmin, xmax, ymin, ymax } = range;
  const xr = xmax - xmin;
  const yr = ymax - ymin;
  const hugeY = yr * 60;
  const samples = 1200;

  const hugeX = xr * 60;

  // 明示関数どうし y=f(x), y=g(x)
  if (p.kind === "explicit-y" && q.kind === "explicit-y" && p.fx && q.fx) {
    const f = p.fx, g = q.fx;
    const roots = scanRoots((x) => f(x) - g(x), xmin, xmax, samples, hugeY);
    for (const x of roots) out.push({ x, y: f(x) });
    return;
  }

  // x=f(y) を含む交点は y をパラメータに解く
  if (p.kind === "explicit-x-fn" || q.kind === "explicit-x-fn") {
    const pf = p.kind === "explicit-x-fn" ? p : q;
    const other = pf === p ? q : p;
    const fy = pf.fy!;
    if (other.kind === "explicit-x-fn" && other.fy) {
      const g = other.fy;
      const roots = scanRoots((y) => fy(y) - g(y), ymin, ymax, samples, hugeX);
      for (const y of roots) out.push({ x: fy(y), y });
    } else if (other.kind === "explicit-y" && other.fx) {
      const g = other.fx;
      const roots = scanRoots((y) => g(fy(y)) - y, ymin, ymax, samples, hugeX);
      for (const y of roots) out.push({ x: fy(y), y });
    } else if (other.kind === "explicit-x" && other.xconst !== undefined) {
      const c = other.xconst;
      const roots = scanRoots((y) => fy(y) - c, ymin, ymax, samples, hugeX);
      for (const y of roots) out.push({ x: c, y });
    } else if (other.kind === "implicit" && other.gxy) {
      const g = other.gxy;
      const roots = scanRoots((y) => g(fy(y), y), ymin, ymax, samples, 1e6);
      for (const y of roots) out.push({ x: fy(y), y });
    }
    return;
  }

  // 明示関数 と 縦線 x=c
  const yc =
    p.kind === "explicit-y" && q.kind === "explicit-x" ? { f: p.fx!, c: q.xconst! } :
    q.kind === "explicit-y" && p.kind === "explicit-x" ? { f: q.fx!, c: p.xconst! } : null;
  if (yc) {
    const y = yc.f(yc.c);
    if (Number.isFinite(y)) out.push({ x: yc.c, y });
    return;
  }

  // 明示関数 と 陰関数
  const ei =
    p.kind === "explicit-y" && q.kind === "implicit" ? { f: p.fx!, g: q.gxy! } :
    q.kind === "explicit-y" && p.kind === "implicit" ? { f: q.fx!, g: p.gxy! } : null;
  if (ei) {
    const roots = scanRoots(
      (x) => {
        const y = ei.f(x);
        return Number.isFinite(y) ? ei.g(x, y) : NaN;
      },
      xmin, xmax, samples, 1e6,
    );
    for (const x of roots) out.push({ x, y: ei.f(x) });
    return;
  }

  // 縦線 x=c と 陰関数
  const xi =
    p.kind === "explicit-x" && q.kind === "implicit" ? { c: p.xconst!, g: q.gxy! } :
    q.kind === "explicit-x" && p.kind === "implicit" ? { c: q.xconst!, g: p.gxy! } : null;
  if (xi) {
    const roots = scanRoots((y) => xi.g(xi.c, y), ymin, ymax, samples, 1e6);
    for (const y of roots) out.push({ x: xi.c, y });
    return;
  }

  // 陰関数どうし
  if (p.kind === "implicit" && q.kind === "implicit" && p.gxy && q.gxy) {
    implicitPair(p.gxy, q.gxy, range, out);
    return;
  }
  void xr;
}

/**
 * 表示中の式（曲線・境界）どうしの交点を検出する。
 * 近接点はまとめ、表示範囲外は除く。
 */
export function findIntersections(
  items: RenderItem[],
  range: ViewRange,
  maxPoints = 80,
): Intersection[] {
  const curves = items
    .filter((it) => it.item.visible && it.parsed.ok)
    .map((it) => it.parsed as ParsedExpr);

  const raw: Intersection[] = [];
  for (let i = 0; i < curves.length; i++) {
    for (let j = i + 1; j < curves.length; j++) {
      pairIntersect(curves[i], curves[j], range, raw);
      if (raw.length > maxPoints * 8) break;
    }
  }

  const { xmin, xmax, ymin, ymax } = range;
  const mx = (xmax - xmin) * 0.002;
  const my = (ymax - ymin) * 0.002;
  const inRange = (p: Intersection) =>
    Number.isFinite(p.x) && Number.isFinite(p.y) &&
    p.x >= xmin - mx && p.x <= xmax + mx && p.y >= ymin - my && p.y <= ymax + my;

  // 近接点をクラスタリングして重複を除去
  const tolX = (xmax - xmin) * 0.006;
  const tolY = (ymax - ymin) * 0.006;
  const uniq: Intersection[] = [];
  for (const p of raw) {
    if (!inRange(p)) continue;
    if (uniq.some((u) => Math.abs(u.x - p.x) < tolX && Math.abs(u.y - p.y) < tolY)) continue;
    uniq.push(p);
    if (uniq.length >= maxPoints) break;
  }
  return uniq;
}

/** 交点座標を短い文字列に整形する（整数・簡単な小数に丸める） */
export function fmtCoord(v: number): string {
  const r = Math.round(v);
  if (Math.abs(v - r) < 1e-6) return String(r);
  const s = parseFloat(v.toFixed(3));
  return String(s);
}
