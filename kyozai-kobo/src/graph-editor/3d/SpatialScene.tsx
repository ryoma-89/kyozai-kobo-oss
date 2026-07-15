import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { initMathJax, renderMathToSvg } from "../../../../mathgraph-pdf-studio/src/lib/mathlabel";
import type { SpatialCameraState, SpatialGeometryDocument, SpatialObject, Vec3 } from "./types";
import { buildSpatialSvg } from "./export";
import {
  classifyEdges,
  framedCameraPosition,
  objectVerticesWithNames,
  planeIntersection,
  primitivePoints,
  primitiveSegments,
  transformPoint,
  worldTopology,
} from "./geometry";

interface SpatialSceneProps {
  document: SpatialGeometryDocument;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onCameraChange: (camera: SpatialCameraState) => void;
  navigationMode: "rotate" | "pan";
}

function pointsGeometry(lines: Array<{ from: Vec3; to: Vec3 }>) {
  const values: THREE.Vector3[] = [];
  for (const line of lines) values.push(new THREE.Vector3(...line.from), new THREE.Vector3(...line.to));
  return new THREE.BufferGeometry().setFromPoints(values);
}

function mathLikeLabel(text: string) {
  return !!text.trim() && !/[\u3040-\u30ff\u3400-\u9fff]/.test(text)
    && /^[A-Za-z0-9\s+\-*/=<>^_{}()[\].,|\\]+$/.test(text);
}

function spriteLabel(
  text: string,
  color: string,
  fontSize = 18,
  background: "transparent" | "white" = "transparent",
  onTextureReady: () => void = () => undefined,
  scaleFactor = 1,
) {
  const rendered = mathLikeLabel(text) ? renderMathToSvg(text) : null;
  const hasMath = !!rendered && !rendered.error && rendered.vbW > 0 && rendered.vbH > 0;
  const canvas = document.createElement("canvas");
  const ratio = Math.min(2, window.devicePixelRatio || 1);
  const measure = canvas.getContext("2d")!;
  const fontFamily = mathLikeLabel(text) ? "Cambria Math, Times New Roman, serif" : "sans-serif";
  measure.font = `${mathLikeLabel(text) ? "italic " : ""}600 ${fontSize}px ${fontFamily}`;
  const height = fontSize + 24;
  const width = Math.max(28, Math.min(512, hasMath ? height * rendered.vbW / rendered.vbH + 16 : measure.measureText(text).width + 20));
  canvas.width = Math.ceil(width * ratio); canvas.height = Math.ceil(height * ratio);
  const context = canvas.getContext("2d")!;
  context.scale(ratio, ratio);
  context.font = `${mathLikeLabel(text) ? "italic " : ""}600 ${fontSize}px ${fontFamily}`;
  if (background === "white") {
    context.fillStyle = "rgba(255,255,255,0.9)";
    context.fillRect(1, 4, width - 2, height - 8);
  }
  context.fillStyle = color;
  context.textBaseline = "top";
  context.fillText(text, 10, 13);
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  if (hasMath) {
    const padding = rendered.vbH * 0.08;
    const minX = rendered.vbMinX - padding, minY = rendered.vbMinY - padding;
    const vbWidth = rendered.vbW + padding * 2, vbHeight = rendered.vbH + padding * 2;
    const body = rendered.inner.replace(/currentColor/g, color);
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${minX} ${minY} ${vbWidth} ${vbHeight}" color="${color}" fill="${color}">${body}</svg>`;
    const image = new Image();
    image.onload = () => {
      context.clearRect(0, 0, width, height);
      if (background === "white") {
        context.fillStyle = "rgba(255,255,255,0.9)";
        context.fillRect(1, 4, width - 2, height - 8);
      }
      context.drawImage(image, 8, 7, width - 16, height - 14);
      texture.needsUpdate = true;
      onTextureReady();
    };
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  }
  const material = new THREE.SpriteMaterial({ map: texture, transparent: true, depthTest: false });
  const sprite = new THREE.Sprite(material);
  const worldHeight = 0.62 * height / 72 * scaleFactor;
  sprite.scale.set(worldHeight * width / height, worldHeight, 1);
  sprite.renderOrder = 20;
  return sprite;
}

function flatArrowSprite(color: string, size: number) {
  const canvas = document.createElement("canvas");
  canvas.width = 64; canvas.height = 64;
  const context = canvas.getContext("2d")!;
  context.fillStyle = color;
  context.beginPath(); context.moveTo(58, 32); context.lineTo(8, 9); context.lineTo(8, 55); context.closePath(); context.fill();
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: texture, transparent: true, depthTest: false }));
  sprite.scale.set(size, size, 1);
  sprite.renderOrder = 12;
  return sprite;
}

function solidGroup(object: SpatialObject, documentValue: SpatialGeometryDocument, requestRender: () => void) {
  const topology = worldTopology(object);
  if (!topology) return null;
  const group = new THREE.Group();
  group.userData.objectId = object.id;
  const positions = topology.vertices.flat();
  const indices: number[] = [];
  for (const face of topology.faces) for (let index = 1; index < face.length - 1; index++) indices.push(face[0], face[index], face[index + 1]);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setIndex(indices); geometry.computeVertexNormals();
  const material = new THREE.MeshBasicMaterial({
    color: object.style.faceColor,
    transparent: true,
    opacity: object.style.faceOpacity,
    side: THREE.DoubleSide,
    depthWrite: object.style.faceOpacity > 0.65,
    polygonOffset: true,
    polygonOffsetFactor: 1,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.userData.objectId = object.id;
  group.add(mesh);
  const classified = classifyEdges(object, documentValue.projection.cameraPosition, documentValue.projection.type, documentValue.projection.target);
  const solid = classified.filter((edge) => edge.display === "solid");
  const dashed = classified.filter((edge) => edge.display === "dashed" && documentValue.scene.showHiddenEdges);
  if (dashed.length) {
    const hidden = new THREE.LineSegments(pointsGeometry(dashed), new THREE.LineDashedMaterial({ color: object.style.hiddenLineColor, dashSize: 0.18, gapSize: 0.13, linewidth: object.style.hiddenLineWidth, depthTest: false, transparent: true, opacity: 0.9 }));
    hidden.computeLineDistances(); hidden.renderOrder = 2; hidden.userData.objectId = object.id; group.add(hidden);
  }
  if (solid.length) {
    const visible = new THREE.LineSegments(pointsGeometry(solid), new THREE.LineBasicMaterial({ color: object.id === documentValue.objects.find((value) => value.id === object.id)?.id ? object.style.lineColor : object.style.lineColor, linewidth: object.style.lineWidth }));
    visible.renderOrder = 4; visible.userData.objectId = object.id; group.add(visible);
  }
  for (const vertex of objectVerticesWithNames(object)) {
    const marker = new THREE.Mesh(new THREE.SphereGeometry(object.style.pointSize, 14, 10), new THREE.MeshBasicMaterial({ color: object.style.pointColor }));
    marker.position.set(...vertex.position); marker.userData.objectId = object.id; group.add(marker);
    const label = spriteLabel(vertex.name, object.style.labelColor, object.style.labelFontSize, object.style.labelBackground, requestRender, documentValue.projection.viewHeight / 12);
    label.position.set(...vertex.position); label.userData.objectId = object.id; group.add(label);
  }
  return group;
}

function disposeObject(root: THREE.Object3D) {
  root.traverse((child) => {
    const value = child as THREE.Mesh;
    value.geometry?.dispose?.();
    const materials = Array.isArray(value.material) ? value.material : value.material ? [value.material] : [];
    for (const material of materials) {
      const map = (material as THREE.SpriteMaterial).map;
      map?.dispose(); material.dispose();
    }
  });
}

export function SpatialScene({ document: documentValue, selectedId, onSelect, onCameraChange, navigationMode }: SpatialSceneProps) {
  const frameRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState("");
  const [mathReady, setMathReady] = useState(false);

  useEffect(() => {
    let active = true;
    void initMathJax().then((ready) => { if (active) setMathReady(ready); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    const frame = frameRef.current;
    const host = hostRef.current;
    const preview = previewRef.current;
    if (!frame || !host || !preview) return;
    const stage = frame.parentElement;
    const outputAspect = Math.max(0.01, documentValue.output.widthMm / documentValue.output.heightMm);
    const fitFrame = () => {
      if (!stage) return;
      const availableWidth = Math.max(1, stage.clientWidth);
      const availableHeight = Math.max(1, stage.clientHeight);
      const width = Math.min(availableWidth, availableHeight * outputAspect);
      frame.style.width = `${width}px`;
      frame.style.height = `${width / outputAspect}px`;
    };
    fitFrame();
    setError("");
    preview.innerHTML = buildSpatialSvg(documentValue);
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: documentValue.scene.quality !== "low", alpha: true, preserveDrawingBuffer: false });
    } catch (reason) {
      setError(`WebGLを開始できません: ${String(reason)}`);
      return;
    }
    renderer.setPixelRatio(Math.min(documentValue.scene.quality === "high" ? 2 : 1.5, window.devicePixelRatio || 1));
    renderer.setClearColor(0xffffff, documentValue.scene.background === "transparent" ? 0 : 1);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.domElement.className = "spatial-interaction-canvas";
    renderer.domElement.setAttribute("aria-hidden", "true");
    host.appendChild(renderer.domElement);
    const scene = new THREE.Scene();
    const aspect = Math.max(0.1, host.clientWidth / Math.max(1, host.clientHeight));
    const projection = documentValue.projection;
    const halfHeight = Math.max(0.005, projection.viewHeight / 2);
    const far = Math.max(10_000, projection.viewHeight * 1_000);
    const camera: THREE.OrthographicCamera | THREE.PerspectiveCamera = projection.type === "orthographic"
      ? new THREE.OrthographicCamera(-halfHeight * aspect, halfHeight * aspect, halfHeight, -halfHeight, 0.01, far)
      : new THREE.PerspectiveCamera(projection.fov, aspect, 0.01, far);
    camera.position.set(...framedCameraPosition(projection)); camera.up.set(...projection.up); camera.zoom = projection.zoom;
    camera.lookAt(new THREE.Vector3(...projection.target)); camera.updateProjectionMatrix();
    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(...projection.target);
    controls.enableDamping = false;
    controls.enablePan = true; controls.screenSpacePanning = true;
    controls.mouseButtons.LEFT = navigationMode === "pan" ? THREE.MOUSE.PAN : THREE.MOUSE.ROTATE;
    controls.mouseButtons.RIGHT = THREE.MOUSE.PAN;
    controls.touches.ONE = navigationMode === "pan" ? THREE.TOUCH.PAN : THREE.TOUCH.ROTATE;
    controls.touches.TWO = THREE.TOUCH.DOLLY_PAN;
    controls.update();
    if (camera instanceof THREE.PerspectiveCamera) controls.minDistance = camera.position.distanceTo(controls.target);

    if (documentValue.scene.showGrid) {
      const grid = new THREE.GridHelper(20, 20, 0x94a3b8, 0xdbe3ee);
      grid.position.y = -2.5; scene.add(grid);
    }
    let requestRender: () => void = () => undefined;
    const flatAxisArrows: Array<{ sprite: THREE.Sprite; endpoint: Vec3 }> = [];
    if (documentValue.scene.showAxes) {
      const axisExtent = halfHeight;
      const labelExtent = axisExtent + Math.max(0.2, projection.viewHeight / 15);
      const originOffset = Math.max(0.08, projection.viewHeight * 0.0233);
      const axes = new THREE.LineSegments(
        pointsGeometry([
          { from: [-axisExtent, 0, 0], to: [axisExtent, 0, 0] },
          { from: [0, -axisExtent, 0], to: [0, axisExtent, 0] },
          { from: [0, 0, -axisExtent], to: [0, 0, axisExtent] },
        ]),
        new THREE.LineBasicMaterial({ color: documentValue.scene.axesColor }),
      );
      scene.add(axes);
      for (const endpoint of [[axisExtent, 0, 0], [0, axisExtent, 0], [0, 0, -axisExtent]] as Vec3[]) {
        const sprite = flatArrowSprite(documentValue.scene.axesColor, Math.max(0.1, projection.viewHeight * 0.035));
        sprite.position.set(...endpoint); scene.add(sprite); flatAxisArrows.push({ sprite, endpoint });
      }
      for (const [text, position] of [[documentValue.scene.axesLabels.x, [labelExtent, 0, 0]], [documentValue.scene.axesLabels.z, [0, labelExtent, 0]], [documentValue.scene.axesLabels.y, [0, 0, -labelExtent]]] as Array<[string, Vec3]>) {
        if (!text.trim()) continue;
        const label = spriteLabel(text, documentValue.scene.axesColor, documentValue.scene.axesLabelSize, documentValue.scene.axesLabelBackground, () => requestRender(), projection.viewHeight / 12);
        label.position.set(...position); scene.add(label);
      }
      if (documentValue.scene.showOriginLabel && documentValue.scene.originLabel.trim()) {
        const label = spriteLabel(documentValue.scene.originLabel, documentValue.scene.axesColor, documentValue.scene.axesLabelSize, documentValue.scene.axesLabelBackground, () => requestRender(), projection.viewHeight / 12);
        label.position.set(-originOffset, 0, originOffset); scene.add(label);
      }
    }
    for (const object of documentValue.objects) {
      if (!object.visible) continue;
      const group = solidGroup(object, documentValue, () => requestRender());
      if (group) {
        if (object.id === selectedId) {
          group.traverse((value) => {
            if ((value as THREE.Line).isLine || (value as THREE.LineSegments).isLineSegments) {
              const material = (value as THREE.Line).material as THREE.LineBasicMaterial;
              material.color.set("#7c3aed");
            }
          });
        }
        scene.add(group);
      }
      for (const point of primitivePoints(object)) {
        const mesh = new THREE.Mesh(new THREE.SphereGeometry(object.style.pointSize, 18, 12), new THREE.MeshBasicMaterial({ color: object.style.pointColor }));
        mesh.position.set(...point); mesh.userData.objectId = object.id; scene.add(mesh);
      }
      for (const segment of primitiveSegments(object)) {
        const direction = new THREE.Vector3(...segment.to).sub(new THREE.Vector3(...segment.from));
        if (segment.vector) {
          const arrow = new THREE.ArrowHelper(direction.clone().normalize(), new THREE.Vector3(...segment.from), direction.length(), object.style.lineColor, 0.35, 0.18);
          arrow.userData.objectId = object.id; scene.add(arrow);
        } else {
          const material = segment.dashed
            ? new THREE.LineDashedMaterial({ color: object.style.hiddenLineColor, dashSize: 0.18, gapSize: 0.13, linewidth: object.style.hiddenLineWidth })
            : new THREE.LineBasicMaterial({ color: object.style.lineColor, linewidth: object.style.lineWidth });
          const line = new THREE.Line(pointsGeometry([segment]), material);
          if (segment.dashed) line.computeLineDistances();
          line.userData.objectId = object.id; scene.add(line);
        }
      }
      if (object.type === "label3d") {
        const position: Vec3 = Array.isArray(object.geometry.position) && object.geometry.position.length === 3 ? object.geometry.position as Vec3 : [0, 0, 0];
        const label = spriteLabel(String(object.geometry.text || object.name), object.style.labelColor, object.style.labelFontSize, object.style.labelBackground, () => requestRender(), projection.viewHeight / 12);
        label.position.set(...transformPoint(position, object)); label.userData.objectId = object.id; scene.add(label);
      }
      for (const labelValue of object.labels) {
        const label = spriteLabel(labelValue.text, labelValue.color, labelValue.fontSize, object.style.labelBackground, () => requestRender(), projection.viewHeight / 12);
        label.position.set(...labelValue.position); label.userData.objectId = object.id; scene.add(label);
      }
      if (object.type === "sectionPlane") {
        const polygon = planeIntersection(documentValue, object);
        if (polygon.length >= 3) {
          const geometry = new THREE.BufferGeometry();
          geometry.setAttribute("position", new THREE.Float32BufferAttribute(polygon.flat(), 3));
          const indices: number[] = []; for (let index = 1; index < polygon.length - 1; index++) indices.push(0, index, index + 1);
          geometry.setIndex(indices);
          const mesh = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ color: 0xf59e0b, transparent: true, opacity: 0.35, side: THREE.DoubleSide, depthTest: false }));
          mesh.renderOrder = 8; mesh.userData.objectId = object.id; scene.add(mesh);
          const outline = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(polygon.map((point) => new THREE.Vector3(...point))), new THREE.LineBasicMaterial({ color: 0xb45309, depthTest: false }));
          outline.renderOrder = 9; outline.userData.objectId = object.id; scene.add(outline);
        }
      }
    }

    let frameRequest = 0;
    let previewTimer = 0;
    let previewPending = false;
    const currentCameraState = (): SpatialCameraState => ({
      position: [camera.position.x, camera.position.y, camera.position.z],
      target: [controls.target.x, controls.target.y, controls.target.z],
      up: [camera.up.x, camera.up.y, camera.up.z],
      zoom: camera.zoom,
    });
    const updateOutputPreview = () => {
      previewPending = false;
      const current = currentCameraState();
      const previewDocument: SpatialGeometryDocument = {
        ...documentValue,
        projection: {
          ...documentValue.projection,
          cameraPosition: current.position,
          target: current.target,
          up: current.up,
          zoom: current.zoom,
        },
      };
      preview.innerHTML = buildSpatialSvg(previewDocument);
    };
    const scheduleOutputPreview = () => {
      previewPending = true;
      if (previewTimer) return;
      previewTimer = window.setTimeout(() => {
        previewTimer = 0;
        if (previewPending) updateOutputPreview();
      }, 40);
    };
    const render = () => {
      if (frameRequest) return;
      frameRequest = requestAnimationFrame(() => {
        frameRequest = 0;
        const origin = new THREE.Vector3(0, 0, 0).project(camera);
        for (const { sprite, endpoint } of flatAxisArrows) {
          const end = new THREE.Vector3(...endpoint).project(camera);
          (sprite.material as THREE.SpriteMaterial).rotation = Math.atan2(end.y - origin.y, end.x - origin.x);
        }
        renderer.render(scene, camera);
      });
    };
    const controlsChanged = () => { render(); scheduleOutputPreview(); };
    const controlsEnded = () => {
      if (previewTimer) { window.clearTimeout(previewTimer); previewTimer = 0; }
      updateOutputPreview();
      onCameraChange(currentCameraState());
    };
    requestRender = render;
    const resize = () => {
      fitFrame();
      const width = Math.max(1, host.clientWidth), height = Math.max(1, host.clientHeight);
      renderer.setSize(width, height, false);
      const nextAspect = width / height;
      if (camera instanceof THREE.PerspectiveCamera) camera.aspect = nextAspect;
      else { camera.left = -halfHeight * nextAspect; camera.right = halfHeight * nextAspect; camera.top = halfHeight; camera.bottom = -halfHeight; }
      camera.updateProjectionMatrix();
      render();
    };
    const observer = new ResizeObserver(resize); observer.observe(stage ?? frame); resize();
    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();
    let down: { x: number; y: number } | null = null;
    const pointerDown = (event: PointerEvent) => { down = { x: event.clientX, y: event.clientY }; };
    const pointerUp = (event: PointerEvent) => {
      if (!down || Math.hypot(event.clientX - down.x, event.clientY - down.y) > 8) return;
      const rect = renderer.domElement.getBoundingClientRect();
      pointer.set((event.clientX - rect.left) / rect.width * 2 - 1, -(event.clientY - rect.top) / rect.height * 2 + 1);
      raycaster.setFromCamera(pointer, camera);
      const hit = raycaster.intersectObjects(scene.children, true).find((value) => {
        let current: THREE.Object3D | null = value.object;
        while (current) { if (current.userData.objectId) return true; current = current.parent; }
        return false;
      });
      let current: THREE.Object3D | null = hit?.object ?? null;
      while (current && !current.userData.objectId) current = current.parent;
      onSelect(current?.userData.objectId ?? null);
    };
    renderer.domElement.addEventListener("pointerdown", pointerDown);
    renderer.domElement.addEventListener("pointerup", pointerUp);
    controls.addEventListener("change", controlsChanged);
    controls.addEventListener("end", controlsEnded);
    const contextLost = (event: Event) => { event.preventDefault(); setError("WebGLコンテキストが失われました。復旧を待っています。"); };
    const contextRestored = () => { setError(""); render(); };
    renderer.domElement.addEventListener("webglcontextlost", contextLost);
    renderer.domElement.addEventListener("webglcontextrestored", contextRestored);
    updateOutputPreview();
    render();
    return () => {
      cancelAnimationFrame(frameRequest); if (previewTimer) window.clearTimeout(previewTimer); observer.disconnect();
      controls.removeEventListener("change", controlsChanged); controls.removeEventListener("end", controlsEnded); controls.dispose();
      renderer.domElement.removeEventListener("pointerdown", pointerDown); renderer.domElement.removeEventListener("pointerup", pointerUp);
      renderer.domElement.removeEventListener("webglcontextlost", contextLost); renderer.domElement.removeEventListener("webglcontextrestored", contextRestored);
      disposeObject(scene); renderer.dispose(); renderer.domElement.remove();
    };
  }, [documentValue, mathReady, navigationMode, onCameraChange, onSelect, selectedId]);

  return <div ref={frameRef} className="spatial-canvas" role="application" aria-label="空間図形3Dプレビュー">
    <div ref={previewRef} className="spatial-output-preview" aria-label="PDF出力プレビュー" />
    <div ref={hostRef} className="spatial-interaction-layer" />
    {error && <div className="spatial-webgl-error"><strong>3D表示を利用できません</strong><span>{error}</span><span>JSON・SVG・PDF出力と数値編集は引き続き利用できます。</span></div>}
  </div>;
}
