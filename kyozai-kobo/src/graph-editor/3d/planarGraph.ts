import { parseExpression, parsedToG } from "../../../../mathgraph-pdf-studio/src/lib/parser";
import type { ParsedExpr } from "../../../../mathgraph-pdf-studio/src/types";
import type { Topology } from "./geometry";
import type { SpatialObject, Vec3 } from "./types";

const numeric = (value: unknown, fallback: number) => typeof value === "number" && Number.isFinite(value) ? value : fallback;
export type PlanarPlane = "xy" | "xz" | "yz";

export function planarPlane(value: unknown): PlanarPlane {
  return value === "xz" || value === "yz" ? value : "xy";
}

export function planarAxes(plane: PlanarPlane): ["x" | "y", "y" | "z"] {
  return plane === "xz" ? ["x", "z"] : plane === "yz" ? ["y", "z"] : ["x", "y"];
}

function expressionForParser(raw: string, plane: PlanarPlane): { ok: true; expression: string } | { ok: false; message: string } {
  const value = raw.replace(/[ｘＸ]/g, "x").replace(/[ｙＹ]/g, "y").replace(/[ｚＺ]/g, "z");
  if (plane === "xy") {
    if (/\bz\b/i.test(value)) return { ok: false, message: "XY平面では変数 x, y を使ってください" };
    return { ok: true, expression: value };
  }
  if (plane === "xz") {
    if (/\by\b/i.test(value)) return { ok: false, message: "XZ平面では変数 x, z を使ってください" };
    return { ok: true, expression: value.replace(/\bz\b/gi, "y") };
  }
  if (/\bx\b/i.test(value)) return { ok: false, message: "YZ平面では変数 y, z を使ってください" };
  return { ok: true, expression: value.replace(/\by\b/gi, "\uE000").replace(/\bz\b/gi, "y").replace(/\uE000/g, "x") };
}

function planePoint(plane: PlanarPlane, first: number, second: number, offset = 0): Vec3 {
  if (plane === "xz") return [first, second, -offset];
  if (plane === "yz") return [offset, second, -first];
  return [first, offset, -second];
}

export function compilePlanarExpression(raw: string, plane: PlanarPlane = "xy") {
  if ([...raw].length > 500) return { ok: false as const, message: "数式は500文字までです" };
  const converted = expressionForParser(raw, plane);
  return converted.ok ? parseExpression(converted.expression) : converted;
}

function addMarchingSegments(
  vertices: Vec3[],
  edges: Array<[number, number]>,
  g: (x: number, y: number) => number,
  parsed: ParsedExpr,
  xMin: number,
  xMax: number,
  yMin: number,
  yMax: number,
  resolution: number,
  plane: PlanarPlane,
) {
  const dx = (xMax - xMin) / resolution;
  const dy = (yMax - yMin) / resolution;
  const valueGrid = Array.from({ length: resolution + 1 }, (_, yi) =>
    Array.from({ length: resolution + 1 }, (_, xi) => g(xMin + xi * dx, yMin + yi * dy)),
  );
  const crossing = (x1: number, y1: number, v1: number, x2: number, y2: number, v2: number): [number, number] | null => {
    if (!Number.isFinite(v1) || !Number.isFinite(v2)) return null;
    const epsilon = 1e-10;
    if (Math.abs(v1) > epsilon && Math.abs(v2) > epsilon && Math.sign(v1) === Math.sign(v2)) return null;
    if (Math.abs(v1) <= epsilon && Math.abs(v2) <= epsilon) return null;
    const ratio = Math.abs(v1 - v2) <= epsilon ? 0.5 : Math.max(0, Math.min(1, v1 / (v1 - v2)));
    return [x1 + (x2 - x1) * ratio, y1 + (y2 - y1) * ratio];
  };
  for (let yi = 0; yi < resolution; yi++) {
    const y0 = yMin + yi * dy, y1 = y0 + dy;
    for (let xi = 0; xi < resolution; xi++) {
      const x0 = xMin + xi * dx, x1 = x0 + dx;
      const a = valueGrid[yi][xi], b = valueGrid[yi][xi + 1];
      const c = valueGrid[yi + 1][xi + 1], d = valueGrid[yi + 1][xi];
      const candidates = [
        crossing(x0, y0, a, x1, y0, b),
        crossing(x1, y0, b, x1, y1, c),
        crossing(x1, y1, c, x0, y1, d),
        crossing(x0, y1, d, x0, y0, a),
      ].filter((value): value is [number, number] => !!value);
      const intersections = candidates.filter((point, index) => candidates.findIndex((other) => Math.hypot(point[0] - other[0], point[1] - other[1]) < 1e-9) === index);
      for (let index = 0; index + 1 < intersections.length; index += 2) {
        const from = intersections[index], to = intersections[index + 1];
        const middleX = (from[0] + to[0]) / 2, middleY = (from[1] + to[1]) / 2;
        if (parsed.clipGxy && parsed.clipGxy(middleX, middleY) > 0) continue;
        const first = vertices.length;
        vertices.push(planePoint(plane, from[0], from[1], 0.015), planePoint(plane, to[0], to[1], 0.015));
        edges.push([first, first + 1]);
      }
    }
  }
}

export function planarGraphTopology(object: SpatialObject): Topology | null {
  if (object.type !== "planarGraph3d") return null;
  const plane = planarPlane(object.geometry.plane);
  const parsed = compilePlanarExpression(String(object.geometry.expression ?? ""), plane);
  if (!parsed.ok) return null;
  const xMin = numeric(object.geometry.xMin, -4), xMax = numeric(object.geometry.xMax, 4);
  const yMin = numeric(object.geometry.yMin, -4), yMax = numeric(object.geometry.yMax, 4);
  if (!(xMin < xMax && yMin < yMax)) return null;
  const resolution = Math.max(12, Math.min(240, Math.round(numeric(object.geometry.resolution, 64))));
  const vertices: Vec3[] = [];
  const faces: number[][] = [];
  const edges: Array<[number, number]> = [];

  if (parsed.kind === "parametric" && parsed.xt && parsed.yt) {
    const tMin = numeric(object.geometry.tMin, 0), tMax = numeric(object.geometry.tMax, Math.PI * 2);
    if (!(tMin < tMax)) return null;
    let previous = -1;
    for (let index = 0; index <= resolution * 4; index++) {
      const t = tMin + (tMax - tMin) * index / (resolution * 4);
      const x = parsed.xt(t), y = parsed.yt(t);
      if (!Number.isFinite(x) || !Number.isFinite(y) || x < xMin || x > xMax || y < yMin || y > yMax) { previous = -1; continue; }
      const current = vertices.length;
      vertices.push(planePoint(plane, x, y, 0.015));
      if (previous >= 0) edges.push([previous, current]);
      previous = current;
    }
    return { vertices, faces, edges };
  }

  const g = parsedToG(parsed);
  if (parsed.isInequality && object.geometry.fill !== false) {
    const dx = (xMax - xMin) / resolution, dy = (yMax - yMin) / resolution;
    type ScalarPoint = { x: number; y: number; value: number };
    const grid = Array.from({ length: resolution + 1 }, (_, yi) =>
      Array.from({ length: resolution + 1 }, (_, xi): ScalarPoint => {
        const x = xMin + xi * dx, y = yMin + yi * dy;
        return { x, y, value: g(x, y) };
      }),
    );
    const clippedTriangle = (triangle: ScalarPoint[]) => {
      const polygon: ScalarPoint[] = [];
      for (let index = 0; index < triangle.length; index++) {
        const current = triangle[index], next = triangle[(index + 1) % triangle.length];
        const currentInside = Number.isFinite(current.value) && current.value <= 0;
        const nextInside = Number.isFinite(next.value) && next.value <= 0;
        if (currentInside) polygon.push(current);
        if (currentInside !== nextInside && Number.isFinite(current.value) && Number.isFinite(next.value)) {
          const ratio = Math.max(0, Math.min(1, current.value / (current.value - next.value)));
          polygon.push({ x: current.x + (next.x - current.x) * ratio, y: current.y + (next.y - current.y) * ratio, value: 0 });
        }
      }
      return polygon;
    };
    const addFace = (polygon: ScalarPoint[]) => {
      if (polygon.length < 3) return;
      const face: number[] = [];
      for (const point of polygon) {
        face.push(vertices.length);
        vertices.push(planePoint(plane, point.x, point.y));
      }
      faces.push(face);
    };
    for (let yi = 0; yi < resolution; yi++) for (let xi = 0; xi < resolution; xi++) {
      const a = grid[yi][xi], b = grid[yi][xi + 1], c = grid[yi + 1][xi + 1], d = grid[yi + 1][xi];
      if ([a, b, c, d].every((point) => Number.isFinite(point.value) && point.value <= 0)) addFace([a, b, c, d]);
      else {
        addFace(clippedTriangle([a, b, c]));
        addFace(clippedTriangle([a, c, d]));
      }
    }
  }
  addMarchingSegments(vertices, edges, g, parsed, xMin, xMax, yMin, yMax, resolution, plane);
  return { vertices, faces, edges };
}
