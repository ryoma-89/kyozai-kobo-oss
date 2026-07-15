import { useEffect, useMemo, useRef, useState } from "react";
import { Home, ZoomIn, ZoomOut } from "lucide-react";
import type { MathLabel, PaperSettings, PointItem, ViewRange } from "../types";
import { buildGraphSvg, type LabelLayout, type RenderItem } from "../lib/buildSvg";
import type { Intersection } from "../lib/intersections";
import { graphAspectRatio, graphDisplayRange } from "../lib/aspect";

interface Props {
  items: RenderItem[];
  points: PointItem[];
  labels: MathLabel[];
  range: ViewRange;
  paper: PaperSettings;
  /** MathJax の準備が整ったか（整うとラベルが再描画される） */
  mathReady: boolean;
  selectedLabelId: string | null;
  onRangeChange: (patch: Partial<ViewRange>) => void;
  onWarningsChange: (w: Map<string, string>) => void;
  onLabelMove: (id: string, x: number, y: number) => void;
  onSelectLabel: (id: string | null) => void;
  onIntersectionsChange: (pts: Intersection[]) => void;
}

const MIN_SPAN = 1e-6;
const MAX_SPAN = 1e6;

export default function GraphView({
  items,
  points,
  labels,
  range,
  paper,
  mathReady,
  selectedLabelId,
  onRangeChange,
  onWarningsChange,
  onLabelMove,
  onSelectLabel,
  onIntersectionsChange,
}: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const holderRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [interacting, setInteracting] = useState(false);
  const interactTimer = useRef(0);
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  // パン用ドラッグ状態
  const dragRef = useRef<{
    sx: number;
    sy: number;
    baseRange: ViewRange;
    viewRange: ViewRange;
    pw: number;
    ph: number;
  } | null>(null);
  // ラベル移動用ドラッグ状態
  const labelDragRef = useRef<{ id: string; grabDx: number; grabDy: number } | null>(null);
  const pointersRef = useRef(new Map<number, { x: number; y: number }>());
  const pinchRef = useRef<{
    startDistance: number;
    anchorX: number;
    anchorY: number;
    baseRange: ViewRange;
  } | null>(null);
  const layoutsRef = useRef<LabelLayout[]>([]);

  const rangeRef = useRef(range);
  rangeRef.current = range;
  const displayRange = useMemo(() => graphDisplayRange(paper, range), [paper, range]);
  const displayRangeRef = useRef(displayRange);
  displayRangeRef.current = displayRange;

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const measure = () =>
      setSize((s) =>
        s.w === el.clientWidth && s.h === el.clientHeight
          ? s
          : { w: el.clientWidth, h: el.clientHeight },
      );
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    window.addEventListener("resize", measure);
    const iv = window.setInterval(measure, 400);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
      window.clearInterval(iv);
    };
  }, []);

  const markInteracting = () => {
    setInteracting(true);
    window.clearTimeout(interactTimer.current);
    interactTimer.current = window.setTimeout(() => setInteracting(false), 260);
  };

  useEffect(() => {
    const el = holderRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const svgEl = el.querySelector("svg");
      if (!svgEl) return;
      const rect = svgEl.getBoundingClientRect();
      if (rect.width < 2 || rect.height < 2) return;
      const fx = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
      const fy = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
      const k = e.deltaY > 0 ? 1.18 : 1 / 1.18;
      const r = rangeRef.current;
      const view = displayRangeRef.current;
      const xr = r.xmax - r.xmin;
      const yr = r.ymax - r.ymin;
      if ((k > 1 && (xr > MAX_SPAN || yr > MAX_SPAN)) || (k < 1 && (xr < MIN_SPAN || yr < MIN_SPAN))) {
        return;
      }
      const gx = view.xmin + fx * (view.xmax - view.xmin);
      const gy = view.ymax - fy * (view.ymax - view.ymin);
      onRangeChange({
        xmin: gx - (gx - r.xmin) * k,
        xmax: gx + (r.xmax - gx) * k,
        ymin: gy - (gy - r.ymin) * k,
        ymax: gy + (r.ymax - gy) * k,
      });
      markInteracting();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [onRangeChange]);

  const pad = 26;
  const availW = Math.max(60, size.w - pad * 2);
  const availH = Math.max(60, size.h - pad * 2);
  const ratio = graphAspectRatio(paper, range);
  let W = availW;
  let H = availH;
  if (W / H > ratio) W = H * ratio;
  else H = W / ratio;

  const built = useMemo(() => {
    if (size.w === 0) return null;
    return buildGraphSvg(items, points, displayRange, {
      width: W,
      height: H,
      paper,
      labels,
      samples: interacting ? 500 : 900,
      implicitGrid: interacting ? 90 : 200,
      idPrefix: "prev",
      selectedLabelId: selectedLabelId ?? undefined,
      fastFill: interacting,
      detectIntersections: paper.showIntersections && !interacting,
    });
    // mathReady はラベル再描画のトリガーとして依存に含める
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, points, labels, displayRange, W, H, paper, interacting, size.w, mathReady, selectedLabelId]);

  layoutsRef.current = built?.labelLayouts ?? [];

  const lastWarnKey = useRef("__init__");
  useEffect(() => {
    if (!built) return;
    const key = [...built.warnings].map(([k, v]) => k + v).join("|");
    if (key !== lastWarnKey.current) {
      lastWarnKey.current = key;
      onWarningsChange(built.warnings);
    }
  }, [built, onWarningsChange]);

  // 検出した交点を親へ通知（操作中は前回値を保持するため built が無いときは何もしない）
  const lastInterKey = useRef("__init__");
  useEffect(() => {
    if (!built || !paper.showIntersections || interacting) return;
    const key = built.intersections.map((p) => `${p.x.toFixed(4)},${p.y.toFixed(4)}`).join("|");
    if (key !== lastInterKey.current) {
      lastInterKey.current = key;
      onIntersectionsChange(built.intersections);
    }
  }, [built, paper.showIntersections, interacting, onIntersectionsChange]);

  const svgRect = () => holderRef.current?.querySelector("svg")?.getBoundingClientRect() ?? null;

  const toGraphCoords = (clientX: number, clientY: number) => {
    const rect = svgRect();
    if (!rect || rect.width < 2) return null;
    const r = displayRangeRef.current;
    return {
      x: r.xmin + ((clientX - rect.left) / rect.width) * (r.xmax - r.xmin),
      y: r.ymax - ((clientY - rect.top) / rect.height) * (r.ymax - r.ymin),
    };
  };

  /** クリック位置にあるラベルを探す（最前面優先） */
  const hitLabel = (clientX: number, clientY: number): LabelLayout | null => {
    const rect = svgRect();
    if (!rect) return null;
    const ox = clientX - rect.left;
    const oy = clientY - rect.top;
    const layouts = layoutsRef.current;
    for (let i = layouts.length - 1; i >= 0; i--) {
      const l = layouts[i];
      if (l.error) continue;
      if (ox >= l.px - 4 && ox <= l.px + l.width + 4 && oy >= l.py - 4 && oy <= l.py + l.height + 4) {
        return l;
      }
    }
    return null;
  };

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const rect = svgRect();
    if (!rect) return;
    pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    e.currentTarget.setPointerCapture(e.pointerId);
    if (pointersRef.current.size >= 2) {
      const [a, b] = [...pointersRef.current.values()].slice(0, 2);
      const midpoint = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      const anchor = toGraphCoords(midpoint.x, midpoint.y);
      if (anchor) {
        pinchRef.current = {
          startDistance: Math.max(1, Math.hypot(a.x - b.x, a.y - b.y)),
          anchorX: anchor.x,
          anchorY: anchor.y,
          baseRange: { ...rangeRef.current },
        };
        dragRef.current = null;
        labelDragRef.current = null;
        setInteracting(true);
      }
      return;
    }
    // まずラベルの当たり判定（ラベル移動を優先）
    const lab = hitLabel(e.clientX, e.clientY);
    if (lab) {
      const ox = e.clientX - rect.left;
      const oy = e.clientY - rect.top;
      labelDragRef.current = { id: lab.id, grabDx: ox - lab.px, grabDy: oy - lab.py };
      onSelectLabel(lab.id);
      setInteracting(true);
      return;
    }
    // ラベル以外をクリックしたら選択解除してパン開始
    if (selectedLabelId) onSelectLabel(null);
    dragRef.current = {
      sx: e.clientX,
      sy: e.clientY,
      baseRange: { ...rangeRef.current },
      viewRange: { ...displayRangeRef.current },
      pw: rect.width,
      ph: rect.height,
    };
    setInteracting(true);
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (pointersRef.current.has(e.pointerId)) {
      pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    }
    const g = toGraphCoords(e.clientX, e.clientY);
    if (g) setCursor(g);

    const pinch = pinchRef.current;
    if (pinch && pointersRef.current.size >= 2) {
      const rect = svgRect();
      if (!rect) return;
      const [a, b] = [...pointersRef.current.values()].slice(0, 2);
      const distance = Math.max(1, Math.hypot(a.x - b.x, a.y - b.y));
      const scale = Math.min(20, Math.max(0.05, pinch.startDistance / distance));
      const fx = Math.min(1, Math.max(0, ((a.x + b.x) / 2 - rect.left) / rect.width));
      const fy = Math.min(1, Math.max(0, ((a.y + b.y) / 2 - rect.top) / rect.height));
      const xSpan = (pinch.baseRange.xmax - pinch.baseRange.xmin) * scale;
      const ySpan = (pinch.baseRange.ymax - pinch.baseRange.ymin) * scale;
      if (xSpan >= MIN_SPAN && ySpan >= MIN_SPAN && xSpan <= MAX_SPAN && ySpan <= MAX_SPAN) {
        onRangeChange({
          xmin: pinch.anchorX - fx * xSpan,
          xmax: pinch.anchorX + (1 - fx) * xSpan,
          ymin: pinch.anchorY - (1 - fy) * ySpan,
          ymax: pinch.anchorY + fy * ySpan,
        });
      }
      return;
    }

    // ラベル移動
    const ld = labelDragRef.current;
    if (ld) {
      const rect = svgRect();
      if (!rect) return;
      const leftPx = e.clientX - rect.left - ld.grabDx;
      const topPx = e.clientY - rect.top - ld.grabDy;
      const r = displayRangeRef.current;
      const x = r.xmin + (leftPx / rect.width) * (r.xmax - r.xmin);
      const y = r.ymax - (topPx / rect.height) * (r.ymax - r.ymin);
      onLabelMove(ld.id, x, y);
      return;
    }

    // パン
    const d = dragRef.current;
    if (!d) return;
    const dgx = (-(e.clientX - d.sx) / d.pw) * (d.viewRange.xmax - d.viewRange.xmin);
    const dgy = ((e.clientY - d.sy) / d.ph) * (d.viewRange.ymax - d.viewRange.ymin);
    onRangeChange({
      xmin: d.baseRange.xmin + dgx,
      xmax: d.baseRange.xmax + dgx,
      ymin: d.baseRange.ymin + dgy,
      ymax: d.baseRange.ymax + dgy,
    });
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    pointersRef.current.delete(e.pointerId);
    if (pinchRef.current) {
      if (pointersRef.current.size < 2) pinchRef.current = null;
      if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
      markInteracting();
      return;
    }
    if (labelDragRef.current) {
      labelDragRef.current = null;
      if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
      markInteracting();
      return;
    }
    if (dragRef.current) {
      dragRef.current = null;
      if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
      markInteracting();
    }
  };

  const [hoverLabel, setHoverLabel] = useState(false);

  const zoomCenter = (k: number) => {
    const r = rangeRef.current;
    const cx = (r.xmin + r.xmax) / 2;
    const cy = (r.ymin + r.ymax) / 2;
    const xr = ((r.xmax - r.xmin) / 2) * k;
    const yr = ((r.ymax - r.ymin) / 2) * k;
    if (xr * 2 < MIN_SPAN || xr * 2 > MAX_SPAN) return;
    onRangeChange({ xmin: cx - xr, xmax: cx + xr, ymin: cy - yr, ymax: cy + yr });
  };

  const resetOrigin = () => {
    const r = rangeRef.current;
    const xr = (r.xmax - r.xmin) / 2;
    const yr = (r.ymax - r.ymin) / 2;
    onRangeChange({ xmin: -xr, xmax: xr, ymin: -yr, ymax: yr });
  };

  const fmt = (v: number) => {
    const span = rangeRef.current.xmax - rangeRef.current.xmin;
    const digits = span < 0.1 ? 4 : span < 10 ? 2 : 1;
    return v.toFixed(digits);
  };

  const cursorStyle = labelDragRef.current
    ? "grabbing"
    : dragRef.current
      ? "grabbing"
      : hoverLabel
        ? "move"
        : "grab";

  return (
    <div ref={wrapRef} className="graph-view relative w-full h-full overflow-hidden">
      <div className="graph-stage-controls absolute top-3 left-3 z-10 flex gap-1.5">
        <button className="btn graph-reset-button !px-2.5" onClick={resetOrigin} title="原点を中心に戻す">
          <Home size={14} /> 原点に戻す
        </button>
        <button className="btn !px-2" onClick={() => zoomCenter(1 / 1.3)} title="拡大">
          <ZoomIn size={14} />
        </button>
        <button className="btn !px-2" onClick={() => zoomCenter(1.3)} title="縮小">
          <ZoomOut size={14} />
        </button>
      </div>

      {cursor && (
        <div
          className="absolute bottom-3 left-3 z-10 px-2.5 py-1 rounded-md text-[11px] font-mono"
          style={{
            background: "#11141ccc",
            border: "1px solid var(--border)",
            color: "var(--text-dim)",
          }}
        >
          ( {fmt(cursor.x)} , {fmt(cursor.y)} )
        </div>
      )}

      <div
        className="graph-stage-hint absolute bottom-3 right-3 z-10 text-[10.5px]"
        style={{ color: "var(--text-faint)" }}
      >
        ホイール・ピンチ: ズーム ／ ドラッグ: 移動 ／ 数式ラベルはドラッグで移動
      </div>

      <div
        ref={holderRef}
        className="graph-svg-holder w-full h-full flex items-center justify-center"
        style={{ cursor: cursorStyle, touchAction: "none" }}
        onPointerDown={handlePointerDown}
        onPointerMove={(e) => {
          handlePointerMove(e);
          if (!dragRef.current && !labelDragRef.current) {
            setHoverLabel(!!hitLabel(e.clientX, e.clientY));
          }
        }}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onPointerLeave={() => {
          setCursor(null);
          setHoverLabel(false);
        }}
        dangerouslySetInnerHTML={{ __html: built?.svg ?? "" }}
      />
    </div>
  );
}
