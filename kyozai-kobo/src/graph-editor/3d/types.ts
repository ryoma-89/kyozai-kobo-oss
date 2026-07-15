export type Vec3 = [number, number, number];
export type Vec2 = [number, number];

export type SpatialObjectType =
  | "point3d"
  | "segment3d"
  | "vector3d"
  | "polygon3d"
  | "plane3d"
  | "cube"
  | "cuboid"
  | "prism"
  | "pyramid"
  | "cylinder"
  | "cone"
  | "sphere"
  | "surface3d"
  | "planarGraph3d"
  | "sectionPlane"
  | "label3d";

export type EdgeDisplay = "auto" | "solid" | "dashed" | "hidden";

export interface SpatialTransform {
  position: Vec3;
  rotation: Vec3;
  scale: Vec3;
}

export interface SpatialStyle {
  lineColor: string;
  lineWidth: number;
  faceColor: string;
  faceOpacity: number;
  pointColor: string;
  pointSize: number;
  labelColor: string;
  labelFontSize: number;
  labelBackground: "transparent" | "white";
  hiddenLineColor: string;
  hiddenLineWidth: number;
  edgeOverrides: Record<string, EdgeDisplay>;
}

export interface SpatialLabel {
  id: string;
  text: string;
  position: Vec3;
  placement: "world" | "screen";
  alwaysOnTop: boolean;
  fontSize: number;
  color: string;
  background: boolean;
  border: boolean;
}

export interface SpatialObject {
  id: string;
  type: SpatialObjectType;
  name: string;
  visible: boolean;
  locked: boolean;
  transform: SpatialTransform;
  style: SpatialStyle;
  geometry: Record<string, unknown>;
  labels: SpatialLabel[];
  metadata: Record<string, unknown>;
}

export interface SpatialProjection {
  type: "orthographic" | "perspective";
  cameraPosition: Vec3;
  target: Vec3;
  up: Vec3;
  zoom: number;
  fov: number;
  viewHeight: number;
  preset: string;
}

export interface SpatialOutputSettings {
  widthMm: number;
  heightMm: number;
  pixelWidth: number;
}

export interface SpatialSceneSettings {
  background: "white" | "transparent";
  showAxes: boolean;
  axesColor: string;
  axesLabelSize: number;
  axesLabelGap: number;
  axesLabels: { x: string; y: string; z: string };
  axesLabelBackground: "transparent" | "white";
  showOriginLabel: boolean;
  originLabel: string;
  originLabelPosition: Vec3;
  /** 旧JSONの画面px指定。読込後はoriginLabelPositionへ置き換える。 */
  originLabelOffset?: Vec2;
  showGrid: boolean;
  showHiddenEdges: boolean;
  quality: "low" | "standard" | "high";
}

export interface SpatialGeometryDocument {
  schemaVersion: 2;
  documentType: "spatial-geometry";
  id: string;
  title: string;
  projection: SpatialProjection;
  output: SpatialOutputSettings;
  scene: SpatialSceneSettings;
  objects: SpatialObject[];
  createdAt: string;
  updatedAt: string;
  version: number;
}

export interface SpatialCameraState {
  position: Vec3;
  target: Vec3;
  up: Vec3;
  zoom: number;
}

export const SPATIAL_OBJECT_TYPES: readonly SpatialObjectType[] = [
  "point3d", "segment3d", "vector3d", "polygon3d", "plane3d", "cube", "cuboid",
  "prism", "pyramid", "cylinder", "cone", "sphere", "sectionPlane", "label3d",
  "surface3d", "planarGraph3d",
] as const;
