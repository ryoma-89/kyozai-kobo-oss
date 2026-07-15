import { compileBivariateExpression } from "../../../../mathgraph-pdf-studio/src/lib/parser";
import type { Topology } from "./geometry";
import type { SpatialObject, Vec3 } from "./types";

export type CompiledSurface =
  | { ok: true; normalized: string; evaluate: (x: number, y: number) => number }
  | { ok: false; message: string };

export function compileSurfaceExpression(raw: string): CompiledSurface {
  if ([...raw].length > 500) return { ok: false, message: "数式は500文字までです" };
  const withoutLeft = raw.trim().replace(/^\s*[zｚ]\s*[=＝]\s*/i, "");
  const result = compileBivariateExpression(withoutLeft);
  return result.ok ? result : { ok: false, message: result.message.replace("f(x,y)", "z=f(x,y)") };
}

const numeric = (value: unknown, fallback: number) => typeof value === "number" && Number.isFinite(value) ? value : fallback;

export function surfaceTopology(object: SpatialObject): Topology | null {
  if (object.type !== "surface3d") return null;
  const compiled = compileSurfaceExpression(String(object.geometry.expression ?? ""));
  if (!compiled.ok) return null;
  const xMin = numeric(object.geometry.xMin, -3), xMax = numeric(object.geometry.xMax, 3);
  const yMin = numeric(object.geometry.yMin, -3), yMax = numeric(object.geometry.yMax, 3);
  if (!(xMin < xMax && yMin < yMax)) return null;
  const resolution = Math.max(4, Math.min(160, Math.round(numeric(object.geometry.resolution, 28))));
  const vertices: Vec3[] = [];
  const grid = Array.from({ length: resolution + 1 }, () => Array<number>(resolution + 1).fill(-1));
  for (let yi = 0; yi <= resolution; yi++) {
    const y = yMin + (yMax - yMin) * yi / resolution;
    for (let xi = 0; xi <= resolution; xi++) {
      const x = xMin + (xMax - xMin) * xi / resolution;
      const z = compiled.evaluate(x, y);
      if (!Number.isFinite(z)) continue;
      grid[yi][xi] = vertices.length;
      vertices.push([x, z, -y]);
    }
  }
  const faces: number[][] = [];
  for (let yi = 0; yi < resolution; yi++) {
    for (let xi = 0; xi < resolution; xi++) {
      const a = grid[yi][xi], b = grid[yi][xi + 1], c = grid[yi + 1][xi + 1], d = grid[yi + 1][xi];
      if ([a, b, c, d].every((index) => index >= 0)) faces.push([a, b, c, d]);
    }
  }
  const edgeMap = new Map<string, [number, number]>();
  for (const face of faces) for (let index = 0; index < face.length; index++) {
    const a = face[index], b = face[(index + 1) % face.length];
    const key = a < b ? `${a}-${b}` : `${b}-${a}`;
    if (!edgeMap.has(key)) edgeMap.set(key, a < b ? [a, b] : [b, a]);
  }
  return { vertices, faces, edges: [...edgeMap.values()] };
}
