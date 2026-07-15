import type { PaperSettings, ViewRange } from "../types";

export function clampAspectRatio(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.min(6, Math.max(0.2, value));
}

/** グラフ描画領域の横÷縦。custom では表示範囲を広げてこの比率に合わせる。 */
export function graphAspectRatio(paper: PaperSettings, range: ViewRange): number {
  if (paper.aspectMode === "custom") {
    return clampAspectRatio(paper.customAspectRatio);
  }
  return rangeAspectRatio(range);
}

/** 座標単位を伸縮しない自然な横÷縦。 */
export function rangeAspectRatio(range: ViewRange): number {
  const ratio = (range.xmax - range.xmin) / (range.ymax - range.ymin);
  return clampAspectRatio(ratio);
}

/**
 * 座標単位を伸縮せず、指定した描画領域比率に合うよう表示範囲だけを広げる。
 * 元の範囲は必ず含め、中心位置は保つ。
 */
export function expandRangeToAspect(range: ViewRange, targetAspect: number): ViewRange {
  const xr = range.xmax - range.xmin;
  const yr = range.ymax - range.ymin;
  const aspect = clampAspectRatio(targetAspect);
  if (!Number.isFinite(xr) || !Number.isFinite(yr) || xr <= 0 || yr <= 0) {
    return range;
  }

  const current = xr / yr;
  let nextXr = xr;
  let nextYr = yr;
  if (current < aspect) {
    nextXr = yr * aspect;
  } else if (current > aspect) {
    nextYr = xr / aspect;
  }
  if (!Number.isFinite(nextXr) || !Number.isFinite(nextYr)) {
    return range;
  }

  const cx = (range.xmin + range.xmax) / 2;
  const cy = (range.ymin + range.ymax) / 2;
  return {
    ...range,
    xmin: cx - nextXr / 2,
    xmax: cx + nextXr / 2,
    ymin: cy - nextYr / 2,
    ymax: cy + nextYr / 2,
  };
}

export function graphDisplayRange(paper: PaperSettings, range: ViewRange): ViewRange {
  return expandRangeToAspect(range, graphAspectRatio(paper, range));
}

export function pdfPageAspectRatio(paper: PaperSettings, range: ViewRange): number {
  return paper.pdfAspectMode === "custom"
    ? clampAspectRatio(paper.pdfCustomAspectRatio)
    : graphAspectRatio(paper, range);
}
