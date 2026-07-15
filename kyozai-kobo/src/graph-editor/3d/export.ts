import { jsPDF } from "jspdf";
import "svg2pdf.js";
import { OrthographicCamera, PerspectiveCamera, Vector3 } from "three";
import fontUrl from "../../../../mathgraph-pdf-studio/src/assets/fonts/ipaexg.ttf?url";
import { mathLabelSvg, normalizeMathLabelLatex } from "../../../../mathgraph-pdf-studio/src/lib/mathlabel";
import type { SpatialGeometryDocument, Vec3 } from "./types";
import {
  classifyEdges,
  framedCameraPosition,
  mathToWorld,
  objectVerticesWithNames,
  planeIntersection,
  primitivePoints,
  primitiveSegments,
  transformPoint,
  worldTopology,
} from "./geometry";

interface ProjectedPoint { x: number; y: number; z: number }
interface ProjectedLine { from: ProjectedPoint; to: ProjectedPoint; dashed: boolean; color: string; width: number; arrow?: boolean }
interface ProjectedFace { points: ProjectedPoint[]; color: string; opacity: number; depth: number }
interface ProjectedLabel { point: ProjectedPoint; text: string; color: string; fontSize: number; background: boolean; border: boolean; centered?: boolean }

function cameraFor(document: SpatialGeometryDocument, width: number, height: number) {
  const aspect = width / height;
  const projection = document.projection;
  const halfHeight = Math.max(0.005, projection.viewHeight / 2);
  const far = Math.max(10_000, projection.viewHeight * 1_000);
  const camera = projection.type === "orthographic"
    ? new OrthographicCamera(-halfHeight * aspect, halfHeight * aspect, halfHeight, -halfHeight, 0.01, far)
    : new PerspectiveCamera(projection.fov, aspect, 0.01, far);
  camera.position.set(...framedCameraPosition(projection));
  camera.up.set(...projection.up);
  camera.lookAt(new Vector3(...projection.target));
  camera.zoom = projection.zoom;
  camera.updateProjectionMatrix();
  camera.updateMatrixWorld(true);
  return camera;
}

function projected(point: Vec3, camera: OrthographicCamera | PerspectiveCamera, width: number, height: number): ProjectedPoint {
  const value = new Vector3(...point).project(camera);
  return { x: (value.x + 1) * width / 2, y: (1 - value.y) * height / 2, z: value.z };
}

function escapeXml(value: string) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");
}

function mathLikeLabel(text: string) {
  return !!text.trim() && !/[\u3040-\u30ff\u3400-\u9fff]/.test(text)
    && /^[A-Za-z0-9\s+\-*/=<>^_{}()[\].,|\\]+$/.test(text);
}

function projectedScene(document: SpatialGeometryDocument, width: number, height: number) {
  const camera = cameraFor(document, width, height);
  const faces: ProjectedFace[] = [];
  const lines: ProjectedLine[] = [];
  const points: Array<{ point: ProjectedPoint; color: string; size: number }> = [];
  const labels: ProjectedLabel[] = [];

  for (const object of document.objects) {
    if (!object.visible) continue;
    const mesh = worldTopology(object);
    if (mesh) {
      for (const face of mesh.faces) {
        const facePoints = face.map((index) => projected(mesh.vertices[index], camera, width, height));
        faces.push({
          points: facePoints,
          color: object.style.faceColor,
          opacity: object.style.faceOpacity,
          depth: facePoints.reduce((sum, point) => sum + point.z, 0) / facePoints.length,
        });
      }
      for (const edge of classifyEdges(object, document.projection.cameraPosition, document.projection.type, document.projection.target)) {
        if (edge.display === "hidden" || edge.display === "dashed" && !document.scene.showHiddenEdges) continue;
        const dashed = edge.display === "dashed";
        lines.push({
          from: projected(edge.from, camera, width, height),
          to: projected(edge.to, camera, width, height),
          dashed,
          color: dashed ? object.style.hiddenLineColor : object.style.lineColor,
          width: dashed ? object.style.hiddenLineWidth : object.style.lineWidth,
        });
      }
      for (const vertex of objectVerticesWithNames(object)) {
        points.push({ point: projected(vertex.position, camera, width, height), color: object.style.pointColor, size: object.style.pointSize });
        labels.push({ point: projected(vertex.position, camera, width, height), text: vertex.name, color: object.style.labelColor, fontSize: object.style.labelFontSize, background: object.style.labelBackground === "white", border: false });
      }
    }
    for (const point of primitivePoints(object)) points.push({ point: projected(point, camera, width, height), color: object.style.pointColor, size: object.style.pointSize });
    for (const segment of primitiveSegments(object)) lines.push({
      from: projected(segment.from, camera, width, height), to: projected(segment.to, camera, width, height), dashed: segment.dashed,
      color: segment.dashed ? object.style.hiddenLineColor : object.style.lineColor,
      width: (segment.dashed ? object.style.hiddenLineWidth : object.style.lineWidth) + (segment.vector ? 0.5 : 0),
    });
    if (object.type === "label3d") {
      const position: Vec3 = Array.isArray(object.geometry.position) && object.geometry.position.length === 3 ? object.geometry.position as Vec3 : [0, 0, 0];
      labels.push({ point: projected(transformPoint(position, object), camera, width, height), text: String(object.geometry.text || object.name), color: object.style.labelColor, fontSize: object.style.labelFontSize, background: object.style.labelBackground === "white", border: false });
    }
    for (const label of object.labels) labels.push({
      point: projected(label.position, camera, width, height), text: label.text, color: label.color,
      fontSize: label.fontSize, background: object.style.labelBackground === "white", border: label.border,
    });
    if (object.type === "sectionPlane") {
      const polygon = planeIntersection(document, object).map((point) => projected(point, camera, width, height));
      if (polygon.length >= 3) faces.push({ points: polygon, color: "#f59e0b", opacity: 0.28, depth: polygon.reduce((sum, point) => sum + point.z, 0) / polygon.length });
      for (let index = 0; index < polygon.length; index++) lines.push({ from: polygon[index], to: polygon[(index + 1) % polygon.length], dashed: false, color: "#b45309", width: 3 });
    }
  }
  if (document.scene.showAxes) {
    const axisExtent = Math.max(0.005, document.projection.viewHeight / 2);
    const projectedOrigin = projected([0, 0, 0], camera, width, height);
    for (const [from, to] of [[[-axisExtent, 0, 0], [0, 0, 0]], [[0, -axisExtent, 0], [0, 0, 0]], [[0, 0, axisExtent], [0, 0, 0]]] as Array<[Vec3, Vec3]>) {
      lines.push({ from: projected(from, camera, width, height), to: projected(to, camera, width, height), dashed: false, color: document.scene.axesColor, width: 1.5 });
    }
    for (const [from, to] of [[[0, 0, 0], [axisExtent, 0, 0]], [[0, 0, 0], [0, axisExtent, 0]], [[0, 0, 0], [0, 0, -axisExtent]]] as Array<[Vec3, Vec3]>) {
      lines.push({ from: projected(from, camera, width, height), to: projected(to, camera, width, height), dashed: false, color: document.scene.axesColor, width: 1.5, arrow: true });
    }
    for (const [text, position] of [[document.scene.axesLabels.x, [axisExtent, 0, 0]], [document.scene.axesLabels.z, [0, axisExtent, 0]], [document.scene.axesLabels.y, [0, 0, -axisExtent]]] as Array<[string, Vec3]>) {
      if (!text.trim()) continue;
      const endpoint = projected(position, camera, width, height);
      const dx = endpoint.x - projectedOrigin.x, dy = endpoint.y - projectedOrigin.y;
      const length = Math.max(1e-6, Math.hypot(dx, dy));
      const distance = document.scene.axesLabelGap + document.scene.axesLabelSize * 0.42;
      labels.push({ point: { ...endpoint, x: endpoint.x + dx / length * distance, y: endpoint.y + dy / length * distance }, text, color: document.scene.axesColor, fontSize: document.scene.axesLabelSize, background: document.scene.axesLabelBackground === "white", border: false, centered: true });
    }
    if (document.scene.showOriginLabel && document.scene.originLabel.trim()) {
      labels.push({ point: projected(mathToWorld(document.scene.originLabelPosition), camera, width, height), text: document.scene.originLabel, color: document.scene.axesColor, fontSize: document.scene.axesLabelSize, background: document.scene.axesLabelBackground === "white", border: false, centered: true });
    }
  }
  faces.sort((a, b) => b.depth - a.depth);
  return { faces, lines, points, labels };
}

export function buildSpatialSvg(document: SpatialGeometryDocument, width = document.output.pixelWidth, height = Math.max(1, Math.round(width * document.output.heightMm / document.output.widthMm))) {
  const scene = projectedScene(document, width, height);
  const definitions = document.scene.showAxes ? `<defs><marker id="spatial-axis-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0 0L8 4L0 8Z" fill="${document.scene.axesColor}"/></marker></defs>` : "";
  const background = document.scene.background === "transparent" ? "" : `<rect width="${width}" height="${height}" fill="#ffffff"/>`;
  const faces = scene.faces.map((face) => `<polygon points="${face.points.map((point) => `${point.x.toFixed(2)},${point.y.toFixed(2)}`).join(" ")}" fill="${face.color}" fill-opacity="${face.opacity.toFixed(3)}" stroke="none"/>`).join("");
  const hidden = scene.lines.filter((line) => line.dashed).map((line) => `<line x1="${line.from.x.toFixed(2)}" y1="${line.from.y.toFixed(2)}" x2="${line.to.x.toFixed(2)}" y2="${line.to.y.toFixed(2)}" stroke="${line.color}" stroke-width="${line.width}" stroke-dasharray="9 7" stroke-linecap="round"/>`).join("");
  const visible = scene.lines.filter((line) => !line.dashed).map((line) => `<line x1="${line.from.x.toFixed(2)}" y1="${line.from.y.toFixed(2)}" x2="${line.to.x.toFixed(2)}" y2="${line.to.y.toFixed(2)}" stroke="${line.color}" stroke-width="${line.width}" stroke-linecap="round"${line.arrow ? ' marker-end="url(#spatial-axis-arrow)"' : ""}/>`).join("");
  const points = scene.points.map((item) => `<circle cx="${item.point.x.toFixed(2)}" cy="${item.point.y.toFixed(2)}" r="${Math.max(2, item.size * 40).toFixed(2)}" fill="${item.color}"/>`).join("");
  const labels = scene.labels.map((label, index) => {
    const probe = mathLikeLabel(label.text) ? mathLabelSvg(label.text, 0, 0, label.fontSize, label.color, `spatial${index}`) : null;
    const widthEstimate = probe?.svg ? probe.width : Math.max(label.fontSize * 0.9, [...label.text].length * label.fontSize * 0.72);
    const heightEstimate = probe?.svg ? probe.height : label.fontSize * 1.1;
    const x = label.centered ? label.point.x - widthEstimate / 2 : label.point.x + 10;
    const top = label.centered ? label.point.y - heightEstimate / 2 : label.point.y - 10 - label.fontSize;
    const math = probe?.svg ? mathLabelSvg(label.text, x, top, label.fontSize, label.color, `spatial${index}`) : null;
    const backgroundRect = label.background ? `<rect x="${(x - 4).toFixed(2)}" y="${(top - 4).toFixed(2)}" width="${(widthEstimate + 8).toFixed(2)}" height="${(heightEstimate + 8).toFixed(2)}" rx="3" fill="#ffffff" fill-opacity="0.9"${label.border ? ' stroke="#94a3b8"' : ""}/>` : "";
    if (math?.svg) return `<g>${backgroundRect}${math.svg}</g>`;
    const fallback = escapeXml(normalizeMathLabelLatex(label.text));
    return `<g>${backgroundRect}<text x="${x.toFixed(2)}" y="${(top + label.fontSize).toFixed(2)}" font-family="${mathLikeLabel(label.text) ? "Cambria Math, Times New Roman, serif" : "IPAexGothic, sans-serif"}" font-size="${label.fontSize}"${mathLikeLabel(label.text) ? ' font-style="italic"' : ""} fill="${label.color}">${fallback}</text></g>`;
  }).join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">${definitions}${background}<g>${faces}${hidden}${visible}${points}${labels}</g></svg>`;
}

export async function spatialSvgToPngBytes(svg: string, scale = 2): Promise<Uint8Array> {
  const parsed = new DOMParser().parseFromString(svg, "image/svg+xml").documentElement;
  const width = Number(parsed.getAttribute("width")) || 1600;
  const height = Number(parsed.getAttribute("height")) || 1100;
  const image = new Image();
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    await new Promise<void>((resolve, reject) => { image.onload = () => resolve(); image.onerror = () => reject(new Error("SVG画像を読み込めません")); image.src = url; });
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(width * scale); canvas.height = Math.round(height * scale);
    const context = canvas.getContext("2d");
    if (!context) throw new Error("Canvasを利用できません");
    context.scale(scale, scale); context.drawImage(image, 0, 0, width, height);
    const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob((value) => value ? resolve(value) : reject(new Error("PNGを生成できません")), "image/png"));
    return new Uint8Array(await blob.arrayBuffer());
  } finally { URL.revokeObjectURL(url); }
}

let fontPromise: Promise<string> | null = null;
async function japaneseFontBase64() {
  fontPromise ??= (async () => {
    const bytes = new Uint8Array(await (await fetch(fontUrl)).arrayBuffer());
    let binary = "";
    for (let index = 0; index < bytes.length; index += 0x8000) binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
    return btoa(binary);
  })();
  return fontPromise;
}

export async function spatialSvgToPdfBytes(svg: string, widthMm: number, heightMm: number): Promise<Uint8Array> {
  const doc = new jsPDF({ orientation: widthMm >= heightMm ? "landscape" : "portrait", unit: "mm", format: [widthMm, heightMm], compress: true });
  doc.addFileToVFS("ipaexg.ttf", await japaneseFontBase64());
  doc.addFont("ipaexg.ttf", "IPAexGothic", "normal");
  doc.setFont("IPAexGothic");
  const holder = document.createElement("div");
  holder.style.position = "fixed"; holder.style.left = "-100000px"; holder.innerHTML = svg;
  document.body.appendChild(holder);
  try {
    const element = holder.querySelector("svg");
    if (!element) throw new Error("SVGを生成できません");
    await doc.svg(element, { x: 0, y: 0, width: widthMm, height: heightMm });
  } finally { holder.remove(); }
  return new Uint8Array(doc.output("arraybuffer"));
}

export function buildSpatialTikz(document: SpatialGeometryDocument) {
  const width = 140, height = width * document.output.heightMm / document.output.widthMm;
  const scene = projectedScene(document, width, height);
  const lines = scene.lines.map((line) => {
    const style = `${line.dashed ? "dashed, gray" : "solid, black"}${line.arrow ? ", ->" : ""}`;
    return `  \\draw[${style}, line width=${Math.max(0.3, line.width * 0.25).toFixed(2)}pt] (${(line.from.x / 10).toFixed(3)},${((height - line.from.y) / 10).toFixed(3)}) -- (${(line.to.x / 10).toFixed(3)},${((height - line.to.y) / 10).toFixed(3)});`;
  });
  const labels = scene.labels.map((label) => `  \\node[fill=white, inner sep=1pt] at (${(label.point.x / 10).toFixed(3)},${((height - label.point.y) / 10).toFixed(3)}) {${label.text.replace(/[{}%#&_]/g, "\\$&")}};`);
  return ["% Generated by 教材工房 spatial geometry", "\\begin{tikzpicture}[x=1cm,y=1cm]", ...lines, ...labels, "\\end{tikzpicture}", ""].join("\n");
}
