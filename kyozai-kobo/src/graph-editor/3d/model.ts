import type {
  EdgeDisplay,
  SpatialGeometryDocument,
  SpatialObject,
  SpatialObjectType,
  SpatialProjection,
  SpatialStyle,
  Vec3,
} from "./types";
import { SPATIAL_OBJECT_TYPES } from "./types";

const MAX_ABS_COORDINATE = 1_000_000;

export function spatialId(prefix = "spatial") {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return `${prefix}_${crypto.randomUUID()}`;
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2)}`;
}

export function defaultSpatialStyle(): SpatialStyle {
  return {
    lineColor: "#172033",
    lineWidth: 2,
    faceColor: "#dbeafe",
    faceOpacity: 0.2,
    pointColor: "#dc2626",
    pointSize: 0.16,
    labelColor: "#111827",
    labelFontSize: 18,
    labelBackground: "transparent",
    hiddenLineColor: "#64748b",
    hiddenLineWidth: 1.35,
    edgeOverrides: {},
  };
}

export function defaultProjection(): SpatialProjection {
  return {
    type: "orthographic",
    cameraPosition: [6, 5, 7],
    target: [0, 0, 0],
    up: [0, 1, 0],
    zoom: 1,
    fov: 38,
    viewHeight: 12,
    preset: "textbook",
  };
}

export function createSpatialDocument(title = "無題の空間図形"): SpatialGeometryDocument {
  const now = new Date().toISOString();
  return {
    schemaVersion: 2,
    documentType: "spatial-geometry",
    id: spatialId("document"),
    title,
    projection: defaultProjection(),
    output: { widthMm: 160, heightMm: 110, pixelWidth: 1600 },
    scene: {
      background: "white",
      showAxes: false,
      axesColor: "#334155",
      axesLabelSize: 16,
      axesLabelGap: 8,
      axesLabels: { x: "x", y: "y", z: "z" },
      axesLabelBackground: "transparent",
      showOriginLabel: true,
      originLabel: "O",
      originLabelPosition: [-0.3, -0.3, 0],
      showGrid: false,
      showHiddenEdges: true,
      quality: "standard",
    },
    objects: [],
    createdAt: now,
    updatedAt: now,
    version: 1,
  };
}

const DEFAULT_VERTEX_NAMES: Record<string, string[]> = {
  cube: ["A", "B", "C", "D", "E", "F", "G", "H"],
  cuboid: ["A", "B", "C", "D", "E", "F", "G", "H"],
  prism: ["A", "B", "C", "D", "E", "F"],
  pyramid: ["A", "B", "C", "D", "S"],
};

export function createSpatialObject(type: SpatialObjectType, name?: string): SpatialObject {
  const geometry: Record<string, unknown> = {};
  if (type === "cube") Object.assign(geometry, { sideLength: 4, vertexNames: DEFAULT_VERTEX_NAMES.cube });
  if (type === "cuboid") Object.assign(geometry, { width: 5, height: 3, depth: 4, vertexNames: DEFAULT_VERTEX_NAMES.cuboid });
  if (type === "prism") Object.assign(geometry, { radius: 2.2, height: 4, sides: 3, vertexNames: DEFAULT_VERTEX_NAMES.prism });
  if (type === "pyramid") Object.assign(geometry, { radius: 2.5, height: 4, sides: 4, vertexNames: DEFAULT_VERTEX_NAMES.pyramid });
  if (type === "cylinder") Object.assign(geometry, { radius: 2, height: 4, sides: 24 });
  if (type === "cone") Object.assign(geometry, { radius: 2.2, height: 4, sides: 24 });
  if (type === "sphere") Object.assign(geometry, { radius: 2 });
  if (type === "surface3d") Object.assign(geometry, {
    expression: "z = x^2 + y^2",
    xMin: -3,
    xMax: 3,
    yMin: -3,
    yMax: 3,
    resolution: 28,
    wireframe: true,
  });
  if (type === "planarGraph3d") Object.assign(geometry, {
    expression: "x^2 + y^2 <= 4",
    xMin: -4,
    xMax: 4,
    yMin: -4,
    yMax: 4,
    resolution: 64,
    tMin: 0,
    tMax: Math.PI * 2,
    fill: true,
    plane: "xy",
  });
  if (type === "point3d") Object.assign(geometry, { point: [0, 0, 0] });
  if (type === "segment3d" || type === "vector3d") Object.assign(geometry, { from: [-2, -2, -2], to: [2, 2, 2], lineType: "solid" });
  if (type === "polygon3d") Object.assign(geometry, { points: [[-2, 0, -2], [2, 0, -2], [0, 0, 2]] });
  if (type === "plane3d" || type === "sectionPlane") Object.assign(geometry, { point: [0, 0, 0], normal: [0, 1, 0], size: 6 });
  if (type === "label3d") Object.assign(geometry, { position: [0, 0, 0], text: "P" });
  const style = defaultSpatialStyle();
  if (type === "planarGraph3d") style.faceOpacity = 0.32;
  return {
    id: spatialId("object"),
    type,
    name: name ?? objectTypeLabel(type),
    visible: true,
    locked: false,
    transform: { position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
    style,
    geometry,
    labels: [],
    metadata: {},
  };
}

export function createTextbookCube(): SpatialGeometryDocument {
  const document = createSpatialDocument("立方体ABCD-EFGH");
  const cube = createSpatialObject("cube", "立方体ABCD-EFGH");
  return { ...document, objects: [cube] };
}

export function objectTypeLabel(type: SpatialObjectType) {
  const labels: Record<SpatialObjectType, string> = {
    point3d: "点", segment3d: "線分", vector3d: "ベクトル", polygon3d: "多角形",
    plane3d: "平面", cube: "立方体", cuboid: "直方体", prism: "角柱", pyramid: "角錐",
    cylinder: "円柱", cone: "円錐", sphere: "球", surface3d: "3D関数曲面", planarGraph3d: "2D式（XY平面）", sectionPlane: "切断面", label3d: "ラベル",
  };
  return labels[type];
}

function isFiniteVec3(value: unknown): value is Vec3 {
  return Array.isArray(value) && value.length === 3
    && value.every((part) => typeof part === "number" && Number.isFinite(part) && Math.abs(part) <= MAX_ABS_COORDINATE);
}

function isColor(value: unknown): value is string {
  return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value);
}

function cleanStyle(value: unknown): SpatialStyle {
  const base = defaultSpatialStyle();
  if (!value || typeof value !== "object") return base;
  const raw = value as Partial<SpatialStyle>;
  const overrides: Record<string, EdgeDisplay> = {};
  if (raw.edgeOverrides && typeof raw.edgeOverrides === "object") {
    for (const [key, display] of Object.entries(raw.edgeOverrides).slice(0, 2_000)) {
      if (/^\d+-\d+$/.test(key) && ["auto", "solid", "dashed", "hidden"].includes(display)) overrides[key] = display;
    }
  }
  return {
    lineColor: isColor(raw.lineColor) ? raw.lineColor : base.lineColor,
    lineWidth: finiteBetween(raw.lineWidth, 0.25, 12, base.lineWidth),
    faceColor: isColor(raw.faceColor) ? raw.faceColor : base.faceColor,
    faceOpacity: finiteBetween(raw.faceOpacity, 0, 1, base.faceOpacity),
    pointColor: isColor(raw.pointColor) ? raw.pointColor : base.pointColor,
    pointSize: finiteBetween(raw.pointSize, 0.03, 1, base.pointSize),
    labelColor: isColor(raw.labelColor) ? raw.labelColor : base.labelColor,
    labelFontSize: finiteBetween(raw.labelFontSize, 8, 72, base.labelFontSize),
    labelBackground: raw.labelBackground === "white" ? "white" : "transparent",
    hiddenLineColor: isColor(raw.hiddenLineColor) ? raw.hiddenLineColor : base.hiddenLineColor,
    hiddenLineWidth: finiteBetween(raw.hiddenLineWidth, 0.25, 12, base.hiddenLineWidth),
    edgeOverrides: overrides,
  };
}

function finiteBetween(value: unknown, min: number, max: number, fallback: number) {
  return typeof value === "number" && Number.isFinite(value) && value >= min && value <= max ? value : fallback;
}

function isPositiveNumber(value: unknown, max = 10_000) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0.01 && value <= max;
}

function validVertexNames(value: unknown) {
  return value === undefined || Array.isArray(value) && value.length <= 100
    && value.every((name) => typeof name === "string" && [...name].length <= 30 && !/[\u0000-\u001f]/.test(name));
}

function validateGeometry(object: SpatialObject) {
  const geometry = object.geometry;
  if (!geometry || typeof geometry !== "object" || Array.isArray(geometry)) return false;
  switch (object.type) {
    case "cube":
      return isPositiveNumber(geometry.sideLength) && validVertexNames(geometry.vertexNames);
    case "cuboid":
      return isPositiveNumber(geometry.width) && isPositiveNumber(geometry.height) && isPositiveNumber(geometry.depth) && validVertexNames(geometry.vertexNames);
    case "prism":
    case "pyramid":
    case "cylinder":
    case "cone":
      return isPositiveNumber(geometry.radius) && isPositiveNumber(geometry.height)
        && Number.isInteger(geometry.sides) && (geometry.sides as number) >= 3 && (geometry.sides as number) <= 48
        && validVertexNames(geometry.vertexNames);
    case "sphere":
      return isPositiveNumber(geometry.radius);
    case "surface3d":
      return typeof geometry.expression === "string" && [...geometry.expression].length >= 1 && [...geometry.expression].length <= 500
        && typeof geometry.xMin === "number" && typeof geometry.xMax === "number" && geometry.xMin < geometry.xMax
        && typeof geometry.yMin === "number" && typeof geometry.yMax === "number" && geometry.yMin < geometry.yMax
        && [geometry.xMin, geometry.xMax, geometry.yMin, geometry.yMax].every((value) => Number.isFinite(value) && Math.abs(value as number) <= 1_000_000)
        && Number.isInteger(geometry.resolution) && (geometry.resolution as number) >= 4 && (geometry.resolution as number) <= 160
        && typeof geometry.wireframe === "boolean";
    case "planarGraph3d":
      return typeof geometry.expression === "string" && [...geometry.expression].length >= 1 && [...geometry.expression].length <= 500
        && typeof geometry.xMin === "number" && typeof geometry.xMax === "number" && geometry.xMin < geometry.xMax
        && typeof geometry.yMin === "number" && typeof geometry.yMax === "number" && geometry.yMin < geometry.yMax
        && [geometry.xMin, geometry.xMax, geometry.yMin, geometry.yMax].every((value) => Number.isFinite(value) && Math.abs(value as number) <= 1_000_000)
        && Number.isInteger(geometry.resolution) && (geometry.resolution as number) >= 12 && (geometry.resolution as number) <= 240
        && (geometry.tMin === undefined || typeof geometry.tMin === "number" && Number.isFinite(geometry.tMin) && Math.abs(geometry.tMin) <= 1_000_000)
        && (geometry.tMax === undefined || typeof geometry.tMax === "number" && Number.isFinite(geometry.tMax) && Math.abs(geometry.tMax) <= 1_000_000)
        && (geometry.tMin === undefined || geometry.tMax === undefined || geometry.tMin < geometry.tMax)
        && (geometry.fill === undefined || typeof geometry.fill === "boolean")
        && (geometry.plane === undefined || ["xy", "xz", "yz"].includes(String(geometry.plane)));
    case "point3d":
      return isFiniteVec3(geometry.point);
    case "segment3d":
    case "vector3d":
      return isFiniteVec3(geometry.from) && isFiniteVec3(geometry.to)
        && (geometry.lineType === undefined || geometry.lineType === "solid" || geometry.lineType === "dashed");
    case "polygon3d":
      return Array.isArray(geometry.points) && geometry.points.length >= 3 && geometry.points.length <= 500 && geometry.points.every(isFiniteVec3);
    case "plane3d":
    case "sectionPlane":
      return isFiniteVec3(geometry.point) && isFiniteVec3(geometry.normal)
        && Math.hypot(...geometry.normal) > 1e-9 && isPositiveNumber(geometry.size);
    case "label3d":
      return isFiniteVec3(geometry.position) && typeof geometry.text === "string" && [...geometry.text].length <= 1_000 && !/[\u0000-\u001f]/.test(geometry.text);
  }
}

function validateLabels(labels: unknown) {
  return Array.isArray(labels) && labels.length <= 200 && labels.every((label) => {
    if (!label || typeof label !== "object" || Array.isArray(label)) return false;
    const value = label as Record<string, unknown>;
    return typeof value.id === "string" && /^[A-Za-z0-9_-]{1,100}$/.test(value.id)
      && typeof value.text === "string" && [...value.text].length <= 1_000 && !/[\u0000-\u001f]/.test(value.text)
      && isFiniteVec3(value.position) && (value.placement === "world" || value.placement === "screen")
      && typeof value.alwaysOnTop === "boolean" && finiteBetween(value.fontSize, 6, 200, -1) !== -1
      && isColor(value.color) && typeof value.background === "boolean" && typeof value.border === "boolean";
  });
}

export function parseSpatialDocument(text: string): { ok: true; document: SpatialGeometryDocument } | { ok: false; message: string } {
  if (text.length > 2 * 1024 * 1024) return { ok: false, message: "空間図形JSONは2MBまでです" };
  let value: unknown;
  try { value = JSON.parse(text); } catch { return { ok: false, message: "JSONとして読み取れません" }; }
  if (!value || typeof value !== "object") return { ok: false, message: "JSONのルートが不正です" };
  const raw = value as Partial<SpatialGeometryDocument>;
  if (raw.documentType !== "spatial-geometry" || raw.schemaVersion !== 2) return { ok: false, message: "未対応の空間図形形式です" };
  if (!Array.isArray(raw.objects) || raw.objects.length > 1_000) return { ok: false, message: "オブジェクト数が不正です" };
  if (!raw.projection || !isFiniteVec3(raw.projection.cameraPosition) || !isFiniteVec3(raw.projection.target) || !isFiniteVec3(raw.projection.up)) {
    return { ok: false, message: "カメラ設定が不正です" };
  }
  if (Math.hypot(...raw.projection.up) < 1e-9 || Math.hypot(
    raw.projection.cameraPosition[0] - raw.projection.target[0],
    raw.projection.cameraPosition[1] - raw.projection.target[1],
    raw.projection.cameraPosition[2] - raw.projection.target[2],
  ) < 1e-9) return { ok: false, message: "カメラの向きが不正です" };
  const seen = new Set<string>();
  const objects: SpatialObject[] = [];
  for (const item of raw.objects) {
    if (!item || typeof item !== "object") return { ok: false, message: "オブジェクトが不正です" };
    const object = item as SpatialObject;
    if (!SPATIAL_OBJECT_TYPES.includes(object.type) || typeof object.id !== "string" || !/^[A-Za-z0-9_-]{1,100}$/.test(object.id) || seen.has(object.id)) {
      return { ok: false, message: "オブジェクトIDまたは型が不正です" };
    }
    if (!object.transform || !isFiniteVec3(object.transform.position) || !isFiniteVec3(object.transform.rotation) || !isFiniteVec3(object.transform.scale)) {
      return { ok: false, message: `${object.name || object.id}の座標が不正です` };
    }
    if (!validateGeometry(object)) return { ok: false, message: `${object.name || object.id}の形状データが不正です` };
    if (!validateLabels(object.labels)) return { ok: false, message: `${object.name || object.id}のラベルが不正です` };
    seen.add(object.id);
    objects.push({
      ...createSpatialObject(object.type),
      ...object,
      name: String(object.name || objectTypeLabel(object.type)).slice(0, 200),
      style: cleanStyle(object.style),
      geometry: object.geometry && typeof object.geometry === "object" ? object.geometry : {},
      labels: Array.isArray(object.labels) ? object.labels.slice(0, 200) : [],
      metadata: {},
    });
  }
  const projection = raw.projection;
  const now = new Date().toISOString();
  return {
    ok: true,
    document: {
      schemaVersion: 2,
      documentType: "spatial-geometry",
      id: typeof raw.id === "string" ? raw.id.slice(0, 100) : spatialId("document"),
      title: String(raw.title || "無題の空間図形").slice(0, 200),
      projection: {
        type: projection.type === "perspective" ? "perspective" : "orthographic",
        cameraPosition: projection.cameraPosition,
        target: projection.target,
        up: projection.up,
        zoom: finiteBetween(projection.zoom, 0.05, 100, 1),
        fov: finiteBetween(projection.fov, 10, 100, 38),
        viewHeight: finiteBetween(projection.viewHeight, 0.01, 5_000_000, 12),
        preset: String(projection.preset || "custom").slice(0, 50),
      },
      output: {
        widthMm: finiteBetween(raw.output?.widthMm, 10, 1_000, 160),
        heightMm: finiteBetween(raw.output?.heightMm, 10, 1_000, 110),
        pixelWidth: Math.round(finiteBetween(raw.output?.pixelWidth, 400, 8_000, 1_600)),
      },
      scene: {
        background: raw.scene?.background === "transparent" ? "transparent" : "white",
        showAxes: !!raw.scene?.showAxes,
        axesColor: isColor(raw.scene?.axesColor) ? raw.scene.axesColor : "#334155",
        axesLabelSize: finiteBetween(raw.scene?.axesLabelSize, 8, 72, 16),
        axesLabelGap: finiteBetween(raw.scene?.axesLabelGap, 0, 200, 8),
        axesLabels: {
          x: typeof raw.scene?.axesLabels?.x === "string" ? raw.scene.axesLabels.x.slice(0, 30) : "x",
          y: typeof raw.scene?.axesLabels?.y === "string" ? raw.scene.axesLabels.y.slice(0, 30) : "y",
          z: typeof raw.scene?.axesLabels?.z === "string" ? raw.scene.axesLabels.z.slice(0, 30) : "z",
        },
        axesLabelBackground: raw.scene?.axesLabelBackground === "white" ? "white" : "transparent",
        showOriginLabel: raw.scene?.showOriginLabel !== false,
        originLabel: typeof raw.scene?.originLabel === "string" ? raw.scene.originLabel.slice(0, 30) : "O",
        originLabelPosition: isFiniteVec3(raw.scene?.originLabelPosition) ? raw.scene.originLabelPosition : [-0.3, -0.3, 0],
        showGrid: !!raw.scene?.showGrid,
        showHiddenEdges: raw.scene?.showHiddenEdges !== false,
        quality: ["low", "high"].includes(raw.scene?.quality ?? "") ? raw.scene!.quality : "standard",
      },
      objects,
      createdAt: typeof raw.createdAt === "string" ? raw.createdAt : now,
      updatedAt: typeof raw.updatedAt === "string" ? raw.updatedAt : now,
      version: Number.isInteger(raw.version) && (raw.version ?? 0) > 0 ? raw.version! : 1,
    },
  };
}

export function serializeSpatialDocument(document: SpatialGeometryDocument) {
  return JSON.stringify({ ...document, updatedAt: new Date().toISOString() }, null, 2);
}

export const VIEW_PRESETS: Record<string, { label: string; position: Vec3; up?: Vec3 }> = {
  front: { label: "正面", position: [0, 0, 9] },
  back: { label: "背面", position: [0, 0, -9] },
  top: { label: "上面", position: [0, 9, 0], up: [0, 0, -1] },
  bottom: { label: "底面", position: [0, -9, 0], up: [0, 0, 1] },
  left: { label: "左側面", position: [-9, 0, 0] },
  right: { label: "右側面", position: [9, 0, 0] },
  isometric: { label: "等角投影", position: [7, 7, 7] },
  textbook: { label: "教科書風", position: [6, 5, 7] },
};
