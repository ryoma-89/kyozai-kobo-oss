import {
  Euler,
  Matrix4,
  Quaternion,
  Vector3,
} from "three";
import type { EdgeDisplay, SpatialGeometryDocument, SpatialObject, SpatialProjection, Vec3 } from "./types";
import { planarGraphTopology } from "./planarGraph";
import { surfaceTopology } from "./surface";

export interface Topology {
  vertices: Vec3[];
  faces: number[][];
  edges: Array<[number, number]>;
}

export interface ClassifiedEdge {
  key: string;
  from: Vec3;
  to: Vec3;
  display: Exclude<EdgeDisplay, "auto">;
}

/** 右手系の数学座標(x,y,z)とThree.js内部座標(x,z,-y)の相互変換。 */
export function mathToWorld([x, y, z]: Vec3): Vec3 { return [x, z, -y]; }
export function worldToMath([x, z, negativeY]: Vec3): Vec3 { return [x, -negativeY, z]; }

export function framedCameraPosition(projection: SpatialProjection): Vec3 {
  const target = new Vector3(...projection.target);
  const current = new Vector3(...projection.cameraPosition);
  const direction = current.clone().sub(target);
  if (direction.lengthSq() < 1e-12) direction.set(6, 5, 7);
  const viewHeight = Math.max(0.01, projection.viewHeight || 12);
  const perspectiveDistance = viewHeight / Math.max(0.01, 2 * Math.tan((projection.fov || 38) * Math.PI / 360));
  const minimumDistance = projection.type === "perspective" ? perspectiveDistance * 1.35 : viewHeight * 1.8;
  direction.setLength(Math.max(direction.length(), minimumDistance));
  const position = target.add(direction);
  return [position.x, position.y, position.z];
}

export function spatialDocumentBounds(document: SpatialGeometryDocument) {
  const points: Vec3[] = [];
  for (const object of document.objects) {
    if (!object.visible) continue;
    const mesh = worldTopology(object);
    if (mesh) points.push(...mesh.vertices);
    points.push(...primitivePoints(object));
    for (const segment of primitiveSegments(object)) points.push(segment.from, segment.to);
    if (object.type === "label3d") {
      const position: Vec3 = Array.isArray(object.geometry.position) && object.geometry.position.length === 3 ? object.geometry.position as Vec3 : [0, 0, 0];
      points.push(transformPoint(position, object));
    }
  }
  if (!points.length) return null;
  const min = new Vector3(Infinity, Infinity, Infinity), max = new Vector3(-Infinity, -Infinity, -Infinity);
  for (const point of points) { min.min(new Vector3(...point)); max.max(new Vector3(...point)); }
  const center = min.clone().add(max).multiplyScalar(0.5);
  const span = max.clone().sub(min);
  return {
    center: [center.x, center.y, center.z] as Vec3,
    span: [span.x, span.y, span.z] as Vec3,
    viewHeight: Math.min(5_000_000, Math.max(1, span.length() * 1.2)),
  };
}

const numberValue = (value: unknown, fallback: number, min = 0.01, max = 10_000) =>
  typeof value === "number" && Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;

const integerValue = (value: unknown, fallback: number, min: number, max: number) =>
  Math.round(numberValue(value, fallback, min, max));

function topology(vertices: Vec3[], faces: number[][]): Topology {
  const map = new Map<string, [number, number]>();
  for (const face of faces) {
    for (let index = 0; index < face.length; index++) {
      const a = face[index];
      const b = face[(index + 1) % face.length];
      const key = a < b ? `${a}-${b}` : `${b}-${a}`;
      if (!map.has(key)) map.set(key, a < b ? [a, b] : [b, a]);
    }
  }
  return { vertices, faces, edges: [...map.values()] };
}

function boxTopology(width: number, height: number, depth: number): Topology {
  const x = width / 2, y = height / 2, z = depth / 2;
  return topology(
    [[-x, -y, z], [x, -y, z], [x, -y, -z], [-x, -y, -z], [-x, y, z], [x, y, z], [x, y, -z], [-x, y, -z]],
    [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]],
  );
}

function prismTopology(radius: number, height: number, sides: number): Topology {
  const vertices: Vec3[] = [];
  for (const y of [-height / 2, height / 2]) {
    for (let index = 0; index < sides; index++) {
      const angle = Math.PI / 2 - index * Math.PI * 2 / sides;
      vertices.push([radius * Math.cos(angle), y, radius * Math.sin(angle)]);
    }
  }
  const bottom = Array.from({ length: sides }, (_, index) => sides - 1 - index);
  const top = Array.from({ length: sides }, (_, index) => sides + index);
  const faces = [bottom, top];
  for (let index = 0; index < sides; index++) faces.push([index, (index + 1) % sides, sides + (index + 1) % sides, sides + index]);
  return topology(vertices, faces);
}

function pyramidTopology(radius: number, height: number, sides: number): Topology {
  const vertices: Vec3[] = [];
  for (let index = 0; index < sides; index++) {
    const angle = Math.PI / 2 - index * Math.PI * 2 / sides;
    vertices.push([radius * Math.cos(angle), -height / 2, radius * Math.sin(angle)]);
  }
  vertices.push([0, height / 2, 0]);
  const apex = sides;
  const faces = [Array.from({ length: sides }, (_, index) => sides - 1 - index)];
  for (let index = 0; index < sides; index++) faces.push([index, (index + 1) % sides, apex]);
  return topology(vertices, faces);
}

function sphereTopology(radius: number, segments = 16, rings = 8): Topology {
  const vertices: Vec3[] = [[0, radius, 0]];
  for (let ring = 1; ring < rings; ring++) {
    const phi = Math.PI * ring / rings;
    for (let segment = 0; segment < segments; segment++) {
      const theta = Math.PI * 2 * segment / segments;
      vertices.push([radius * Math.sin(phi) * Math.cos(theta), radius * Math.cos(phi), radius * Math.sin(phi) * Math.sin(theta)]);
    }
  }
  const bottom = vertices.length;
  vertices.push([0, -radius, 0]);
  const faces: number[][] = [];
  for (let segment = 0; segment < segments; segment++) faces.push([0, 1 + segment, 1 + (segment + 1) % segments]);
  for (let ring = 0; ring < rings - 2; ring++) {
    const start = 1 + ring * segments;
    const next = start + segments;
    for (let segment = 0; segment < segments; segment++) {
      const after = (segment + 1) % segments;
      faces.push([start + segment, next + segment, next + after, start + after]);
    }
  }
  const last = 1 + (rings - 2) * segments;
  for (let segment = 0; segment < segments; segment++) faces.push([last + segment, bottom, last + (segment + 1) % segments]);
  return topology(vertices, faces);
}

export function solidTopology(object: SpatialObject): Topology | null {
  const geometry = object.geometry;
  switch (object.type) {
    case "cube": {
      const side = numberValue(geometry.sideLength, 4);
      return boxTopology(side, side, side);
    }
    case "cuboid":
      return boxTopology(numberValue(geometry.width, 5), numberValue(geometry.height, 3), numberValue(geometry.depth, 4));
    case "prism":
      return prismTopology(numberValue(geometry.radius, 2.2), numberValue(geometry.height, 4), integerValue(geometry.sides, 3, 3, 16));
    case "pyramid":
      return pyramidTopology(numberValue(geometry.radius, 2.5), numberValue(geometry.height, 4), integerValue(geometry.sides, 4, 3, 16));
    case "cylinder":
      return prismTopology(numberValue(geometry.radius, 2), numberValue(geometry.height, 4), integerValue(geometry.sides, 24, 8, 48));
    case "cone":
      return pyramidTopology(numberValue(geometry.radius, 2.2), numberValue(geometry.height, 4), integerValue(geometry.sides, 24, 8, 48));
    case "sphere":
      return sphereTopology(numberValue(geometry.radius, 2), 18, 9);
    default:
      return null;
  }
}

export function objectMatrix(object: SpatialObject) {
  const position = new Vector3(...object.transform.position);
  const quaternion = new Quaternion().setFromEuler(new Euler(...object.transform.rotation, "XYZ"));
  const scale = new Vector3(...object.transform.scale);
  return new Matrix4().compose(position, quaternion, scale);
}

export function transformPoint(point: Vec3, object: SpatialObject): Vec3 {
  const value = new Vector3(...point).applyMatrix4(objectMatrix(object));
  return [value.x, value.y, value.z];
}

export function worldTopology(object: SpatialObject): Topology | null {
  const local = object.type === "surface3d" ? surfaceTopology(object)
    : object.type === "planarGraph3d" ? planarGraphTopology(object)
      : solidTopology(object);
  if (!local) return null;
  return { ...local, vertices: local.vertices.map((point) => transformPoint(point, object)) };
}

export function edgeKey(edge: [number, number]) {
  return edge[0] < edge[1] ? `${edge[0]}-${edge[1]}` : `${edge[1]}-${edge[0]}`;
}

function faceNormal(face: number[], vertices: Vec3[], center: Vector3) {
  if (face.length < 3) return { normal: new Vector3(0, 1, 0), centroid: center.clone() };
  const a = new Vector3(...vertices[face[0]]);
  const b = new Vector3(...vertices[face[1]]);
  const c = new Vector3(...vertices[face[2]]);
  const normal = b.clone().sub(a).cross(c.clone().sub(a)).normalize();
  const centroid = face.reduce((sum, index) => sum.add(new Vector3(...vertices[index])), new Vector3()).multiplyScalar(1 / face.length);
  if (normal.dot(centroid.clone().sub(center)) < 0) normal.negate();
  return { normal, centroid };
}

export function classifyEdges(object: SpatialObject, cameraPosition: Vec3, projectionType: "orthographic" | "perspective", target: Vec3): ClassifiedEdge[] {
  const mesh = worldTopology(object);
  if (!mesh) return [];
  if (object.type === "surface3d" || object.type === "planarGraph3d") {
    if (object.geometry.wireframe === false) return [];
    return mesh.edges.map((edge) => ({ key: edgeKey(edge), from: mesh.vertices[edge[0]], to: mesh.vertices[edge[1]], display: "solid" }));
  }
  const center = mesh.vertices.reduce((sum, point) => sum.add(new Vector3(...point)), new Vector3()).multiplyScalar(1 / Math.max(1, mesh.vertices.length));
  const faceData = mesh.faces.map((face) => faceNormal(face, mesh.vertices, center));
  return mesh.edges.map((edge) => {
    const key = edgeKey(edge);
    const forced = object.style.edgeOverrides[key] ?? "auto";
    if (forced !== "auto") return { key, from: mesh.vertices[edge[0]], to: mesh.vertices[edge[1]], display: forced };
    const adjacent = mesh.faces.map((face, index) => face.includes(edge[0]) && face.includes(edge[1]) ? index : -1).filter((index) => index >= 0);
    const visible = adjacent.some((index) => {
      const data = faceData[index];
      const view = projectionType === "perspective"
        ? new Vector3(...cameraPosition).sub(data.centroid).normalize()
        : new Vector3(...cameraPosition).sub(new Vector3(...target)).normalize();
      return data.normal.dot(view) > 1e-7;
    });
    return { key, from: mesh.vertices[edge[0]], to: mesh.vertices[edge[1]], display: visible ? "solid" : "dashed" };
  });
}

export function objectVerticesWithNames(object: SpatialObject): Array<{ name: string; position: Vec3 }> {
  if (!Array.isArray(object.geometry.vertexNames)) return [];
  const mesh = worldTopology(object);
  if (!mesh) return [];
  const names = object.geometry.vertexNames.map(String);
  return mesh.vertices.map((position, index) => ({ name: names[index] || "", position })).filter((item) => item.name);
}

export function primitiveSegments(object: SpatialObject): Array<{ from: Vec3; to: Vec3; vector: boolean; dashed: boolean }> {
  if (object.type !== "segment3d" && object.type !== "vector3d") return [];
  const from: Vec3 = Array.isArray(object.geometry.from) && object.geometry.from.length === 3 ? object.geometry.from as Vec3 : [0, 0, 0];
  const to: Vec3 = Array.isArray(object.geometry.to) && object.geometry.to.length === 3 ? object.geometry.to as Vec3 : [1, 1, 1];
  return [{
    from: transformPoint(from, object),
    to: transformPoint(to, object),
    vector: object.type === "vector3d",
    dashed: object.geometry.lineType === "dashed",
  }];
}

export function primitivePoints(object: SpatialObject): Vec3[] {
  if (object.type !== "point3d") return [];
  const point: Vec3 = Array.isArray(object.geometry.point) && object.geometry.point.length === 3 ? object.geometry.point as Vec3 : [0, 0, 0];
  return [transformPoint(point, object)];
}

export function planeIntersection(document: SpatialGeometryDocument, plane: SpatialObject): Vec3[] {
  const pointRaw: Vec3 = Array.isArray(plane.geometry.point) && plane.geometry.point.length === 3 ? plane.geometry.point as Vec3 : [0, 0, 0];
  const normalRaw: Vec3 = Array.isArray(plane.geometry.normal) && plane.geometry.normal.length === 3 ? plane.geometry.normal as Vec3 : [0, 1, 0];
  const planePoint = new Vector3(...transformPoint(pointRaw, plane));
  const normal = new Vector3(...normalRaw).applyQuaternion(new Quaternion().setFromEuler(new Euler(...plane.transform.rotation))).normalize();
  const intersections: Vector3[] = [];
  for (const object of document.objects) {
    if (!object.visible || object.id === plane.id) continue;
    const mesh = worldTopology(object);
    if (!mesh) continue;
    for (const [aIndex, bIndex] of mesh.edges) {
      const a = new Vector3(...mesh.vertices[aIndex]);
      const b = new Vector3(...mesh.vertices[bIndex]);
      const da = normal.dot(a.clone().sub(planePoint));
      const db = normal.dot(b.clone().sub(planePoint));
      if (Math.abs(da) < 1e-7) intersections.push(a);
      if (da * db < -1e-9) intersections.push(a.clone().lerp(b, da / (da - db)));
    }
  }
  const unique: Vector3[] = [];
  for (const point of intersections) if (!unique.some((other) => other.distanceToSquared(point) < 1e-10)) unique.push(point);
  if (unique.length < 3) return unique.map((point) => [point.x, point.y, point.z]);
  const center = unique.reduce((sum, value) => sum.add(value), new Vector3()).multiplyScalar(1 / unique.length);
  const axisU = Math.abs(normal.y) < 0.9 ? normal.clone().cross(new Vector3(0, 1, 0)).normalize() : normal.clone().cross(new Vector3(1, 0, 0)).normalize();
  const axisV = normal.clone().cross(axisU).normalize();
  unique.sort((a, b) => Math.atan2(a.clone().sub(center).dot(axisV), a.clone().sub(center).dot(axisU)) - Math.atan2(b.clone().sub(center).dot(axisV), b.clone().sub(center).dot(axisU)));
  return unique.map((value) => [value.x, value.y, value.z]);
}
