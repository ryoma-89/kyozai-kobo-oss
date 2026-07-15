import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  aiCancelJob,
  aiCreateJob,
  aiGetJob,
  aiStoreInputImage,
  completeGraphWebSession,
  insertGraphToProject,
  listGraphVersions,
  restoreGraphVersion,
  saveGraphExports,
  updateGraph,
} from "../../api";
import { ConflictError, graphFileUrl, isTauri } from "../../transport";
import { useApp } from "../../store";
import type { AiJob, AiSpatialStructuredResult, CompleteGraphWebSessionResult, GraphVersionSummary, GraphWebSession, ProjectSummary, StoredGraph } from "../../types";
import { Modal } from "../../components/ui";
import { PdfCanvasViewer } from "../../components/PdfCanvasViewer";
import { initMathJax } from "../../../../mathgraph-pdf-studio/src/lib/mathlabel";
import { buildSpatialSvg, buildSpatialTikz, spatialSvgToPdfBytes, spatialSvgToPngBytes } from "./export";
import { mathToWorld, objectVerticesWithNames, solidTopology, spatialDocumentBounds, worldToMath } from "./geometry";
import {
  createSpatialObject,
  objectTypeLabel,
  parseSpatialDocument,
  serializeSpatialDocument,
  spatialId,
  VIEW_PRESETS,
} from "./model";
import { SpatialScene } from "./SpatialScene";
import { compilePlanarExpression, planarAxes, planarPlane } from "./planarGraph";
import { compileSurfaceExpression } from "./surface";
import type { EdgeDisplay, SpatialCameraState, SpatialGeometryDocument, SpatialObject, SpatialObjectType, Vec3 } from "./types";
import "./spatial.css";

interface SpatialGeometryEditorProps {
  stored: StoredGraph;
  projects: ProjectSummary[];
  integration?: {
    session: GraphWebSession;
    onComplete: (result: CompleteGraphWebSessionResult) => void | Promise<void>;
    onCancel: () => void;
  };
  onClose: () => void | Promise<void>;
  onReload: () => void | Promise<void>;
}

type SpatialAiMode = "spatial-geometry-from-text" | "spatial-geometry-from-image" | "spatial-geometry-from-problem";
const isSpatialResult = (value: AiJob["structuredResult"]): value is AiSpatialStructuredResult => !!value && "kind" in value && value.kind === "spatial-geometry";

function bytesToBase64(bytes: Uint8Array) {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  return btoa(binary);
}

function downloadBytes(name: string, bytes: Uint8Array, type: string) {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
  const anchor = document.createElement("a"); anchor.href = url; anchor.download = name; anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 2_000);
}

function downloadText(name: string, value: string, type: string) {
  downloadBytes(name, new TextEncoder().encode(value), type);
}

function safeBase(value: string) {
  return (value.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").replace(/[. ]+$/g, "") || "spatial-geometry").slice(0, 80);
}

async function withTimeout<T>(promise: Promise<T>, milliseconds: number, label: string): Promise<T> {
  let timer = 0;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => { timer = window.setTimeout(() => reject(new Error(`${label}がタイムアウトしました`)), milliseconds); }),
    ]);
  } finally {
    window.clearTimeout(timer);
  }
}

function zipStore(files: Array<{ name: string; bytes: Uint8Array }>) {
  const u16 = (value: number) => [value & 255, value >>> 8 & 255];
  const u32 = (value: number) => [value & 255, value >>> 8 & 255, value >>> 16 & 255, value >>> 24 & 255];
  const crc32 = (bytes: Uint8Array) => {
    let crc = 0xffffffff;
    for (const byte of bytes) { crc ^= byte; for (let bit = 0; bit < 8; bit++) crc = crc >>> 1 ^ (0xedb88320 & -(crc & 1)); }
    return (crc ^ 0xffffffff) >>> 0;
  };
  const encoder = new TextEncoder(); const local: Uint8Array[] = []; const central: Uint8Array[] = []; let offset = 0;
  for (const file of files) {
    const name = encoder.encode(file.name), crc = crc32(file.bytes);
    const header = new Uint8Array([...u32(0x04034b50), ...u16(20), ...u16(0), ...u16(0), ...u16(0), ...u16(0), ...u32(crc), ...u32(file.bytes.length), ...u32(file.bytes.length), ...u16(name.length), ...u16(0), ...name]);
    local.push(header, file.bytes);
    central.push(new Uint8Array([...u32(0x02014b50), ...u16(20), ...u16(20), ...u16(0), ...u16(0), ...u16(0), ...u16(0), ...u32(crc), ...u32(file.bytes.length), ...u32(file.bytes.length), ...u16(name.length), ...u16(0), ...u16(0), ...u16(0), ...u16(0), ...u32(0), ...u32(offset), ...name]));
    offset += header.length + file.bytes.length;
  }
  const centralSize = central.reduce((sum, value) => sum + value.length, 0);
  const end = new Uint8Array([...u32(0x06054b50), ...u16(0), ...u16(0), ...u16(files.length), ...u16(files.length), ...u32(centralSize), ...u32(offset), ...u16(0)]);
  const output = new Uint8Array(offset + centralSize + end.length); let cursor = 0;
  for (const part of [...local, ...central, end]) { output.set(part, cursor); cursor += part.length; }
  return output;
}

function NumberInput({ value, onChange, step = 0.1, min, max, label }: { value: number; onChange: (value: number) => void; step?: number; min?: number; max?: number; label: string }) {
  return <label className="spatial-number"><span>{label}</span><input className="input" type="number" value={Number.isFinite(value) ? value : 0} step={step} min={min} max={max} onChange={(event) => { const next = Number(event.target.value); if (Number.isFinite(next)) onChange(next); }} /></label>;
}

function Vec3Input({ value, onChange, label }: { value: Vec3; onChange: (value: Vec3) => void; label: string }) {
  return <fieldset className="spatial-vector"><legend>{label}</legend>{(["x", "y", "z"] as const).map((axis, index) => <NumberInput key={axis} label={axis} value={value[index]} onChange={(next) => { const copy = [...value] as Vec3; copy[index] = next; onChange(copy); }} />)}</fieldset>;
}

function CoordinateInput({ value, onChange, label }: { value: Vec3; onChange: (value: Vec3) => void; label: string }) {
  return <Vec3Input label={label} value={worldToMath(value)} onChange={(next) => onChange(mathToWorld(next))} />;
}

function ScaleInput({ value, onChange }: { value: Vec3; onChange: (value: Vec3) => void }) {
  return <Vec3Input label="拡大縮小" value={[value[0], value[2], value[1]]} onChange={(next) => onChange([next[0], next[2], next[1]])} />;
}

const SOLID_BUTTONS: Array<[SpatialObjectType, string]> = [
  ["cube", "立方体"], ["cuboid", "直方体"], ["prism", "角柱"], ["pyramid", "角錐"],
  ["cylinder", "円柱"], ["cone", "円錐"], ["sphere", "球"],
  ["surface3d", "3D関数曲面"],
];

export function SpatialGeometryEditor({ stored, projects, integration, onClose, onReload }: SpatialGeometryEditorProps) {
  const { showToast, setDirty, confirm } = useApp();
  const parsed = useMemo(() => parseSpatialDocument(stored.graphJson), [stored.graphJson]);
  const [documentValue, setDocumentValue] = useState<SpatialGeometryDocument>(() => parsed.ok ? parsed.document : (() => { throw new Error(parsed.message); })());
  const documentRef = useRef(documentValue); documentRef.current = documentValue;
  const [record, setRecord] = useState({ id: stored.id, version: stored.version, sourceType: stored.sourceType });
  const recordRef = useRef(record); recordRef.current = record;
  const [selectedId, setSelectedId] = useState<string | null>(documentValue.objects[0]?.id ?? null);
  const [navigationMode, setNavigationMode] = useState<"rotate" | "pan">("rotate");
  const [saveState, setSaveState] = useState<"saved" | "saving" | "dirty" | "error">("saved");
  const [busy, setBusy] = useState(false);
  const [selectedProjectId, setSelectedProjectId] = useState<number | "">(integration?.session.projectId ?? "");
  const [pdfPreview, setPdfPreview] = useState<string | null>(null);
  const [versions, setVersions] = useState<GraphVersionSummary[] | null>(null);
  const [aiOpen, setAiOpen] = useState(false);
  const [aiMode, setAiMode] = useState<SpatialAiMode>("spatial-geometry-from-text");
  const [aiText, setAiText] = useState("");
  const [aiImage, setAiImage] = useState<File | null>(null);
  const [aiJob, setAiJob] = useState<AiJob | null>(null);
  const importRef = useRef<HTMLInputElement>(null);
  const undoRef = useRef<SpatialGeometryDocument[]>([]), redoRef = useRef<SpatialGeometryDocument[]>([]);
  const editRevisionRef = useRef(0);
  const savePromiseRef = useRef<Promise<number | null> | null>(null);
  const selected = documentValue.objects.find((object) => object.id === selectedId) ?? null;

  const commit = useCallback((updater: (current: SpatialGeometryDocument) => SpatialGeometryDocument) => {
    setDocumentValue((current) => {
      const next = updater(current); if (next === current) return current;
      undoRef.current = [...undoRef.current.slice(-79), current]; redoRef.current = [];
      editRevisionRef.current += 1;
      return { ...next, updatedAt: new Date().toISOString() };
    });
    setSaveState("dirty"); setDirty(true);
  }, [setDirty]);

  const save = useCallback(async (download = false) => {
    if (!savePromiseRef.current) {
      const task = (async () => {
        setSaveState("saving");
        try {
          while (true) {
            const revision = editRevisionRef.current;
            const documentAtStart = documentRef.current;
            const currentRecord = recordRef.current;
            const graphJson = serializeSpatialDocument(documentAtStart);
            const version = await updateGraph({ id: currentRecord.id, title: documentAtStart.title || "無題の空間図形", graphJson, graphType: "spatial_geometry", sourceType: currentRecord.sourceType, expectedVersion: currentRecord.version });
            const nextRecord = { ...currentRecord, version };
            recordRef.current = nextRecord; setRecord(nextRecord);
            if (revision === editRevisionRef.current) {
              setSaveState("saved"); setDirty(false); void onReload(); return version;
            }
            setSaveState("saving");
          }
        } catch (error) {
          setSaveState("error");
          showToast(error instanceof ConflictError ? "別の端末で更新されています。一覧へ戻り、最新版を開き直してください。" : `保存に失敗しました: ${String(error)}`, "error");
          return null;
        }
      })();
      savePromiseRef.current = task;
      void task.finally(() => { if (savePromiseRef.current === task) savePromiseRef.current = null; });
    }
    const version = await savePromiseRef.current;
    if (download && version != null) downloadText(`${safeBase(documentRef.current.title)}.spatial-graph.json`, serializeSpatialDocument(documentRef.current), "application/json");
    return version;
  }, [onReload, setDirty, showToast]);

  useEffect(() => {
    if (saveState !== "dirty") return;
    const timer = window.setTimeout(() => void save(false), 1_200);
    return () => window.clearTimeout(timer);
  }, [documentValue, save, saveState]);

  useEffect(() => {
    if (!aiJob || !["queued", "preprocessing", "waiting_for_codex", "converting", "validating"].includes(aiJob.status)) return;
    const timer = window.setTimeout(() => void aiGetJob(aiJob.id).then(setAiJob).catch((error) => showToast(String(error), "error")), 1_000);
    return () => window.clearTimeout(timer);
  }, [aiJob, showToast]);

  const undo = useCallback(() => { const value = undoRef.current.pop(); if (!value) return; redoRef.current.push(documentRef.current); editRevisionRef.current += 1; setDocumentValue(value); setSaveState("dirty"); setDirty(true); }, [setDirty]);
  const redo = useCallback(() => { const value = redoRef.current.pop(); if (!value) return; undoRef.current.push(documentRef.current); editRevisionRef.current += 1; setDocumentValue(value); setSaveState("dirty"); setDirty(true); }, [setDirty]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      if (event.key.toLowerCase() === "s") { event.preventDefault(); void save(false); }
      else if (event.key.toLowerCase() === "z") { event.preventDefault(); event.shiftKey ? redo() : undo(); }
      else if (event.key.toLowerCase() === "y") { event.preventDefault(); redo(); }
    };
    window.addEventListener("keydown", key); return () => window.removeEventListener("keydown", key);
  }, [redo, save, undo]);

  const updateObject = (id: string, updater: (object: SpatialObject) => SpatialObject, allowLocked = false) => {
    const target = documentRef.current.objects.find((object) => object.id === id);
    if (!target || target.locked && !allowLocked) return;
    commit((current) => ({ ...current, objects: current.objects.map((object) => object.id === id ? updater(object) : object) }));
  };
  const addObject = (type: SpatialObjectType) => {
    const object = createSpatialObject(type);
    commit((current) => ({ ...current, objects: [...current.objects, object] })); setSelectedId(object.id);
  };
  const addDiagonal = () => {
    const cube = documentValue.objects.find((object) => object.type === "cube" || object.type === "cuboid");
    if (!cube) { showToast("先に立方体または直方体を追加してください", "error"); return; }
    const vertices = objectVerticesWithNames(cube); const a = vertices.find((value) => value.name === "A"), g = vertices.find((value) => value.name === "G");
    if (!a || !g) return;
    const segment = createSpatialObject("segment3d", "対角線AG"); segment.geometry = { from: a.position, to: g.position, lineType: "solid" }; segment.style.lineWidth = 3;
    commit((current) => ({ ...current, objects: [...current.objects, segment] })); setSelectedId(segment.id);
  };
  const duplicateSelected = () => {
    if (!selected) return;
    const copy: SpatialObject = JSON.parse(JSON.stringify(selected)); copy.id = spatialId("object"); copy.name = `${copy.name}のコピー`; copy.transform.position = [copy.transform.position[0] + 0.5, copy.transform.position[1], copy.transform.position[2]];
    commit((current) => ({ ...current, objects: [...current.objects, copy] })); setSelectedId(copy.id);
  };
  const deleteSelected = async () => {
    if (!selected || selected.locked || !(await confirm(`「${selected.name}」を削除しますか？`))) return;
    commit((current) => ({ ...current, objects: current.objects.filter((object) => object.id !== selected.id) })); setSelectedId(null);
  };
  const cameraChanged = useCallback((camera: SpatialCameraState) => {
    const current = documentRef.current.projection;
    const difference = [...camera.position, ...camera.target, camera.zoom].some((value, index) => Math.abs(value - [...current.cameraPosition, ...current.target, current.zoom][index]) > 1e-5);
    if (!difference) return;
    commit((value) => ({ ...value, projection: { ...value.projection, cameraPosition: camera.position, target: camera.target, up: camera.up, zoom: camera.zoom, preset: "custom" } }));
  }, [commit]);

  const fitToObjects = () => {
    const bounds = spatialDocumentBounds(documentRef.current);
    if (!bounds) { showToast("表示対象の図形がありません", "error"); return; }
    const projection = documentRef.current.projection;
    const direction = projection.cameraPosition.map((value, index) => value - projection.target[index]) as Vec3;
    let length = Math.hypot(...direction);
    if (length < 1e-9) { direction[0] = 6; direction[1] = 5; direction[2] = 7; length = Math.hypot(...direction); }
    const distance = Math.max(10, bounds.viewHeight * 1.8);
    const position = direction.map((value, index) => bounds.center[index] + value / length * distance) as Vec3;
    commit((current) => ({ ...current, projection: { ...current.projection, target: bounds.center, cameraPosition: position, viewHeight: bounds.viewHeight, zoom: 1, preset: "fit" } }));
  };

  const exportArtifacts = async () => {
    const version = await save(false); if (version == null) throw new Error("保存できませんでした");
    await initMathJax();
    const svg = buildSpatialSvg(documentRef.current);
    const png = await withTimeout(spatialSvgToPngBytes(svg, documentRef.current.scene.quality === "high" ? 2.5 : 2), 30_000, "PNG生成");
    const pdf = await withTimeout(spatialSvgToPdfBytes(svg, documentRef.current.output.widthMm, documentRef.current.output.heightMm), 30_000, "PDF生成"); const tex = buildSpatialTikz(documentRef.current); const json = serializeSpatialDocument(documentRef.current);
    await saveGraphExports(record.id, { pdf: bytesToBase64(pdf), png: bytesToBase64(png), svg: bytesToBase64(new TextEncoder().encode(svg)), tex: bytesToBase64(new TextEncoder().encode(tex)) });
    return { version, svg, png, pdf, tex, json, base: safeBase(documentRef.current.title) };
  };
  const runExport = async (format: "pdf" | "png" | "svg" | "tex" | "json" | "zip") => {
    setBusy(true);
    try {
      if (format === "json") { await save(true); return; }
      const files = await exportArtifacts();
      if (format === "pdf") {
        const url = isTauri ? URL.createObjectURL(new Blob([files.pdf as BlobPart], { type: "application/pdf" })) : `${graphFileUrl(record.id, "pdf")}?t=${Date.now()}`;
        setPdfPreview((old) => { if (old?.startsWith("blob:")) URL.revokeObjectURL(old); return url; });
      } else if (format === "png") isTauri ? downloadBytes(`${files.base}.png`, files.png, "image/png") : window.open(graphFileUrl(record.id, "png", true));
      else if (format === "svg") isTauri ? downloadText(`${files.base}.svg`, files.svg, "image/svg+xml") : window.open(graphFileUrl(record.id, "svg", true));
      else if (format === "tex") isTauri ? downloadText(`${files.base}.tex`, files.tex, "text/plain;charset=utf-8") : window.open(graphFileUrl(record.id, "tex", true));
      else if (isTauri) downloadBytes(`${files.base}.zip`, zipStore([{ name: "graph.pdf", bytes: files.pdf }, { name: "graph.png", bytes: files.png }, { name: "graph.svg", bytes: new TextEncoder().encode(files.svg) }, { name: "graph.tex", bytes: new TextEncoder().encode(files.tex) }, { name: "graph.json", bytes: new TextEncoder().encode(files.json) }]), "application/zip");
      else window.open(graphFileUrl(record.id, "zip", true));
      showToast("現在の視点で書き出しました"); void onReload();
    } catch (error) { showToast(`書き出しに失敗しました: ${String(error)}`, "error"); }
    finally { setBusy(false); }
  };

  const importJson = async (file: File) => {
    try {
      const result = parseSpatialDocument(await file.text()); if (!result.ok) throw new Error(result.message);
      undoRef.current.push(documentRef.current); redoRef.current = []; editRevisionRef.current += 1; setDocumentValue(result.document); setSelectedId(result.document.objects[0]?.id ?? null); setSaveState("dirty"); setDirty(true);
    } catch (error) { showToast(`読み込みに失敗しました: ${String(error)}`, "error"); }
  };

  const startSpatialAi = async () => {
    if (aiMode === "spatial-geometry-from-image" && !aiImage) { showToast("画像を選択してください", "error"); return; }
    if (aiMode !== "spatial-geometry-from-image" && !aiText.trim()) { showToast("空間図形の条件または問題文を入力してください", "error"); return; }
    setBusy(true);
    try {
      const inputNames: string[] = [];
      if (aiImage) {
        if (aiImage.size > 12 * 1024 * 1024) throw new Error("画像は12MBまでです");
        const uploaded = await aiStoreInputImage(bytesToBase64(new Uint8Array(await aiImage.arrayBuffer())), aiImage.name);
        inputNames.push(uploaded.name);
      }
      const job = await aiCreateJob({ sourceType: aiMode === "spatial-geometry-from-image" ? "image" : "text", conversionMode: aiMode, inputText: aiText.trim(), inputNames, options: { spatialGeometryOutput: true, requireUserConfirmation: true } });
      setAiJob(job);
    } catch (error) { showToast(`AI下書きを開始できません: ${String(error)}`, "error"); }
    finally { setBusy(false); }
  };

  const acceptSpatialAi = () => {
    if (!aiJob || !isSpatialResult(aiJob.structuredResult)) return;
    const result = parseSpatialDocument(JSON.stringify(aiJob.structuredResult.spatialDocument));
    if (!result.ok) { showToast(`AI下書きを読み込めません: ${result.message}`, "error"); return; }
    commit(() => ({ ...result.document, id: documentRef.current.id, createdAt: documentRef.current.createdAt }));
    setSelectedId(result.document.objects[0]?.id ?? null); setAiOpen(false); setAiJob(null);
    showToast("AI結果を下書きとして読み込みました。確認してから保存・書き出ししてください");
  };

  const geometryNumber = (key: string, fallback: number) => typeof selected?.geometry[key] === "number" ? selected.geometry[key] as number : fallback;
  const updateGeometry = (key: string, value: unknown) => selected && updateObject(selected.id, (object) => ({ ...object, geometry: { ...object.geometry, [key]: value } }));
  const topology = selected ? solidTopology(selected) : null;
  const editableVertexCount = selected && ["cube", "cuboid", "prism", "pyramid"].includes(selected.type) ? topology?.vertices.length ?? 0 : 0;
  const hasEditablePoints = selected?.type === "point3d" || editableVertexCount > 0;
  const selectedPlanarPlane = planarPlane(selected?.geometry.plane);
  const selectedPlanarAxes = planarAxes(selectedPlanarPlane);
  const surfaceError = useMemo(() => {
    if (selected?.type !== "surface3d") return "";
    const result = compileSurfaceExpression(String(selected.geometry.expression ?? ""));
    return result.ok ? "" : result.message;
  }, [selected]);
  const planarError = useMemo(() => {
    if (selected?.type !== "planarGraph3d") return "";
    const result = compilePlanarExpression(String(selected.geometry.expression ?? ""), selectedPlanarPlane);
    return result.ok ? "" : result.message;
  }, [selected, selectedPlanarPlane]);

  return <div className="spatial-editor">
    <header className="spatial-toolbar">
      <button className="btn btn-sm" onClick={async () => { if (saveState === "dirty" || saveState === "saving") await save(false); await onClose(); }}>{integration ? "キャンセル" : "← 一覧"}</button>
      <input className="input spatial-title" value={documentValue.title} onChange={(event) => commit((current) => ({ ...current, title: event.target.value }))} />
      <span className={`save-indicator save-${saveState}`}>{saveState === "saved" ? "保存済み" : saveState === "saving" ? "保存中…" : saveState === "error" ? "保存失敗" : "未保存"}</span>
      <button className="btn btn-sm" disabled={!undoRef.current.length} onClick={undo}>Undo</button>
      <button className="btn btn-sm" disabled={!redoRef.current.length} onClick={redo}>Redo</button>
      <button className="btn btn-sm" onClick={async () => { try { setVersions(await listGraphVersions(record.id)); } catch (error) { showToast(String(error), "error"); } }}>履歴</button>
      <button className="btn btn-sm" onClick={() => { setAiOpen(true); setAiJob(null); }}>AI下書き</button>
      <button className="btn btn-sm" disabled={busy} onClick={() => void runExport("zip")}>一式ZIP</button>
      {integration ? <button className="btn btn-sm btn-primary" disabled={busy} onClick={async () => { setBusy(true); try { const files = await exportArtifacts(); const result = await completeGraphWebSession(integration.session.sessionId, record.id, files.version); await integration.onComplete(result); } catch (error) { showToast(`${integration.session.problemId ? "問題" : "教材"}へ挿入できません: ${String(error)}`, "error"); } finally { setBusy(false); } }}>{integration.session.problemId ? "問題へ挿入して戻る" : "教材へ挿入して戻る"}</button>
        : <><select className="input spatial-project-select" value={selectedProjectId} onChange={(event) => setSelectedProjectId(event.target.value ? Number(event.target.value) : "")}><option value="">教材を選択…</option>{projects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}</select><button className="btn btn-sm btn-primary" disabled={busy || selectedProjectId === ""} onClick={async () => { if (selectedProjectId === "") return; setBusy(true); try { await exportArtifacts(); const target = projects.find((value) => value.id === selectedProjectId); await insertGraphToProject(record.id, selectedProjectId, target?.version); showToast("教材へ空間図形のスナップショットを挿入しました"); void onReload(); } catch (error) { showToast(`教材へ挿入できません: ${String(error)}`, "error"); } finally { setBusy(false); } }}>教材へ挿入</button></>}
    </header>

    <div className="spatial-layout">
      <aside className="spatial-left panel">
        <section><h3>オブジェクト</h3><div className="spatial-add-grid">{SOLID_BUTTONS.map(([type, label]) => <button className="btn btn-sm" key={type} onClick={() => addObject(type)}>＋ {label}</button>)}</div></section>
        <section><h3>教材用要素</h3><div className="spatial-add-grid"><button className="btn btn-sm" onClick={() => addObject("planarGraph3d")}>＋ 2D式をXY平面へ</button><button className="btn btn-sm" onClick={() => addObject("point3d")}>＋ 点</button><button className="btn btn-sm" onClick={() => addObject("segment3d")}>＋ 線分</button><button className="btn btn-sm" onClick={() => addObject("vector3d")}>＋ ベクトル</button><button className="btn btn-sm" onClick={addDiagonal}>＋ 対角線AG</button><button className="btn btn-sm" onClick={() => addObject("sectionPlane")}>＋ 切断面</button><button className="btn btn-sm" onClick={() => addObject("label3d")}>＋ ラベル</button></div></section>
        <div className="spatial-object-list">{documentValue.objects.map((object) => <button key={object.id} className={`spatial-object-row ${object.id === selectedId ? "selected" : ""}`} onClick={() => setSelectedId(object.id)}><span>{object.visible ? "◉" : "○"}</span><span className="truncate">{object.name}</span><small>{objectTypeLabel(object.type)}</small>{object.locked && <span>🔒</span>}</button>)}</div>
      </aside>

      <main className="spatial-stage">
        <SpatialScene document={documentValue} selectedId={selectedId} onSelect={setSelectedId} onCameraChange={cameraChanged} navigationMode={navigationMode} />
        <div className="spatial-view-controls"><span>操作</span><button className={`btn btn-sm ${navigationMode === "rotate" ? "btn-outline" : "btn-ghost"}`} onClick={() => setNavigationMode("rotate")}>回転</button><button className={`btn btn-sm ${navigationMode === "pan" ? "btn-outline" : "btn-ghost"}`} onClick={() => setNavigationMode("pan")}>パン</button><button className="btn btn-sm btn-outline" onClick={fitToObjects}>全体を枠に合わせる</button><span>視点</span>{Object.entries(VIEW_PRESETS).map(([key, preset]) => <button className="btn btn-sm" key={key} onClick={() => commit((current) => ({ ...current, projection: { ...current.projection, cameraPosition: preset.position, up: preset.up ?? [0, 1, 0], target: [0, 0, 0], zoom: 1, preset: key } }))}>{preset.label}</button>)}</div>
        <div className="spatial-stage-hint">{navigationMode === "pan" ? "ドラッグ/1本指: 移動" : "ドラッグ/1本指: 回転"}　ホイール/2本指: ズーム　タップ: 選択</div>
      </main>

      <aside className="spatial-right panel">
        <section><h3>投影と表示</h3><div className="spatial-segmented"><button className={`btn btn-sm ${documentValue.projection.type === "orthographic" ? "btn-outline" : "btn-ghost"}`} onClick={() => commit((current) => ({ ...current, projection: { ...current.projection, type: "orthographic" } }))}>平行投影</button><button className={`btn btn-sm ${documentValue.projection.type === "perspective" ? "btn-outline" : "btn-ghost"}`} onClick={() => commit((current) => ({ ...current, projection: { ...current.projection, type: "perspective" } }))}>透視投影</button></div><NumberInput label="表示枠の高さ（座標）" value={documentValue.projection.viewHeight} min={0.01} max={5_000_000} step={1} onChange={(value) => commit((current) => ({ ...current, projection: { ...current.projection, viewHeight: value } }))} />
          <label className="check-row"><input type="checkbox" checked={documentValue.scene.showHiddenEdges} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, showHiddenEdges: event.target.checked } }))} />隠線を破線表示</label>
          <label className="check-row"><input type="checkbox" checked={documentValue.scene.showAxes} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, showAxes: event.target.checked } }))} />座標軸（画像上の矢印）</label>
          <label className="spatial-color"><span>座標軸の色</span><input type="color" value={documentValue.scene.axesColor} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, axesColor: event.target.value } }))} /></label>
          <NumberInput label="軸ラベルの大きさ" value={documentValue.scene.axesLabelSize} min={8} max={72} step={1} onChange={(value) => commit((current) => ({ ...current, scene: { ...current.scene, axesLabelSize: value } }))} />
          <fieldset className="spatial-vector"><legend>軸ラベル文字（空欄で非表示）</legend>{(["x", "y", "z"] as const).map((axis) => <label className="spatial-number" key={axis}><span>{axis}</span><input className="input" maxLength={30} value={documentValue.scene.axesLabels[axis]} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, axesLabels: { ...current.scene.axesLabels, [axis]: event.target.value } } }))} /></label>)}</fieldset>
          <NumberInput label="軸端とxyzの間隔（px）" value={documentValue.scene.axesLabelGap} min={0} max={200} step={1} onChange={(value) => commit((current) => ({ ...current, scene: { ...current.scene, axesLabelGap: value } }))} />
          <label className="spatial-quality">軸ラベル背景<select className="input" value={documentValue.scene.axesLabelBackground} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, axesLabelBackground: event.target.value as "transparent" | "white" } }))}><option value="transparent">透明</option><option value="white">白</option></select></label>
          <label className="check-row"><input type="checkbox" checked={documentValue.scene.showOriginLabel} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, showOriginLabel: event.target.checked } }))} />原点ラベル</label>
          <label className="spatial-quality">原点名<input className="input" maxLength={30} value={documentValue.scene.originLabel} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, originLabel: event.target.value } }))} /></label>
          <CoordinateInput label="原点ラベル位置（数学xyz）" value={documentValue.scene.originLabelPosition} onChange={(value) => commit((current) => ({ ...current, scene: { ...current.scene, originLabelPosition: value } }))} />
          <label className="check-row"><input type="checkbox" checked={documentValue.scene.showGrid} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, showGrid: event.target.checked } }))} />グリッド</label>
          <label className="spatial-quality">出力背景<select className="input" value={documentValue.scene.background} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, background: event.target.value as SpatialGeometryDocument["scene"]["background"] } }))}><option value="white">白</option><option value="transparent">透明</option></select></label>
        </section>
        {selected ? <>
          <section><h3>選択中</h3><input className="input" disabled={selected.locked} value={selected.name} onChange={(event) => updateObject(selected.id, (object) => ({ ...object, name: event.target.value }))} /><div className="spatial-inline-actions"><button className="btn btn-sm" disabled={selected.locked} onClick={() => updateObject(selected.id, (object) => ({ ...object, visible: !object.visible }))}>{selected.visible ? "非表示" : "表示"}</button><button className="btn btn-sm" onClick={() => updateObject(selected.id, (object) => ({ ...object, locked: !object.locked }), true)}>{selected.locked ? "ロック解除" : "ロック"}</button><button className="btn btn-sm" disabled={selected.locked} onClick={duplicateSelected}>複製</button><button className="btn btn-sm btn-ghost" disabled={selected.locked} onClick={() => void deleteSelected()}>削除</button></div></section>
          <section><CoordinateInput label="座標（数学xyz）" value={selected.transform.position} onChange={(value) => updateObject(selected.id, (object) => ({ ...object, transform: { ...object.transform, position: value } }))} /><CoordinateInput label="回転（数学xyz・rad）" value={selected.transform.rotation} onChange={(value) => updateObject(selected.id, (object) => ({ ...object, transform: { ...object.transform, rotation: value } }))} /><ScaleInput value={selected.transform.scale} onChange={(value) => updateObject(selected.id, (object) => ({ ...object, transform: { ...object.transform, scale: value } }))} /></section>
          {selected.type === "cube" && <section><h3>サイズ</h3><NumberInput label="一辺" value={geometryNumber("sideLength", 4)} min={0.01} onChange={(value) => updateGeometry("sideLength", value)} /></section>}
          {selected.type === "cuboid" && <section><h3>サイズ</h3><NumberInput label="幅" value={geometryNumber("width", 5)} min={0.01} onChange={(value) => updateGeometry("width", value)} /><NumberInput label="高さ" value={geometryNumber("height", 3)} min={0.01} onChange={(value) => updateGeometry("height", value)} /><NumberInput label="奥行" value={geometryNumber("depth", 4)} min={0.01} onChange={(value) => updateGeometry("depth", value)} /></section>}
          {["prism", "pyramid", "cylinder", "cone"].includes(selected.type) && <section><h3>サイズ</h3><NumberInput label="半径" value={geometryNumber("radius", 2)} min={0.01} onChange={(value) => updateGeometry("radius", value)} /><NumberInput label="高さ" value={geometryNumber("height", 4)} min={0.01} onChange={(value) => updateGeometry("height", value)} /><NumberInput label="辺数" value={geometryNumber("sides", 4)} step={1} min={3} max={48} onChange={(value) => updateGeometry("sides", Math.round(value))} /></section>}
          {selected.type === "sphere" && <section><h3>サイズ</h3><NumberInput label="半径" value={geometryNumber("radius", 2)} min={0.01} onChange={(value) => updateGeometry("radius", value)} /></section>}
          {selected.type === "surface3d" && <section><h3>3D関数曲面 z=f(x,y)</h3><input className="input spatial-expression" value={String(selected.geometry.expression ?? "")} placeholder="z = sin(x) + cos(y)" onChange={(event) => updateGeometry("expression", event.target.value)} />{surfaceError && <div className="err-msg mt-1">{surfaceError}</div>}<div className="spatial-range-grid"><NumberInput label="x最小" value={geometryNumber("xMin", -3)} onChange={(value) => updateGeometry("xMin", value)} /><NumberInput label="x最大" value={geometryNumber("xMax", 3)} onChange={(value) => updateGeometry("xMax", value)} /><NumberInput label="y最小" value={geometryNumber("yMin", -3)} onChange={(value) => updateGeometry("yMin", value)} /><NumberInput label="y最大" value={geometryNumber("yMax", 3)} onChange={(value) => updateGeometry("yMax", value)} /></div><NumberInput label="メッシュ分割" value={geometryNumber("resolution", 28)} min={4} max={160} step={1} onChange={(value) => updateGeometry("resolution", Math.round(value))} /><label className="check-row"><input type="checkbox" checked={selected.geometry.wireframe !== false} onChange={(event) => updateGeometry("wireframe", event.target.checked)} />ワイヤーフレームを表示</label></section>}
          {selected.type === "planarGraph3d" && <section><h3>2D式を{selectedPlanarPlane.toUpperCase()}平面に描画</h3><label className="spatial-quality">配置平面<select className="input" value={selectedPlanarPlane} onChange={(event) => updateGeometry("plane", event.target.value)}><option value="xy">XY平面（x, y）</option><option value="xz">XZ平面（x, z）</option><option value="yz">YZ平面（y, z）</option></select></label><input className="input spatial-expression" value={String(selected.geometry.expression ?? "")} placeholder={selectedPlanarPlane === "xy" ? "x^2+y^2<=4" : selectedPlanarPlane === "xz" ? "x^2+z^2<=4" : "y^2+z^2<=4"} onChange={(event) => updateGeometry("expression", event.target.value)} />{planarError && <div className="err-msg mt-1">{planarError}</div>}<div className="spatial-range-grid"><NumberInput label={`${selectedPlanarAxes[0]}最小`} value={geometryNumber("xMin", -4)} onChange={(value) => updateGeometry("xMin", value)} /><NumberInput label={`${selectedPlanarAxes[0]}最大`} value={geometryNumber("xMax", 4)} onChange={(value) => updateGeometry("xMax", value)} /><NumberInput label={`${selectedPlanarAxes[1]}最小`} value={geometryNumber("yMin", -4)} onChange={(value) => updateGeometry("yMin", value)} /><NumberInput label={`${selectedPlanarAxes[1]}最大`} value={geometryNumber("yMax", 4)} onChange={(value) => updateGeometry("yMax", value)} /></div><NumberInput label="描画精度" value={geometryNumber("resolution", 64)} min={12} max={240} step={1} onChange={(value) => updateGeometry("resolution", Math.round(value))} /><label className="check-row"><input type="checkbox" checked={selected.geometry.fill !== false} onChange={(event) => updateGeometry("fill", event.target.checked)} />不等式の領域を平面で塗りつぶす</label><div className="spatial-range-grid"><NumberInput label="媒介変数 t 最小" value={geometryNumber("tMin", 0)} onChange={(value) => updateGeometry("tMin", value)} /><NumberInput label="媒介変数 t 最大" value={geometryNumber("tMax", Math.PI * 2)} onChange={(value) => updateGeometry("tMax", value)} /></div></section>}
          {(selected.type === "point3d") && <section><CoordinateInput label="点（数学xyz）" value={(selected.geometry.point as Vec3) ?? [0, 0, 0]} onChange={(value) => updateGeometry("point", value)} /></section>}
          {(selected.type === "segment3d" || selected.type === "vector3d") && <section><CoordinateInput label="始点（数学xyz）" value={(selected.geometry.from as Vec3) ?? [0, 0, 0]} onChange={(value) => updateGeometry("from", value)} /><CoordinateInput label="終点（数学xyz）" value={(selected.geometry.to as Vec3) ?? [1, 1, 1]} onChange={(value) => updateGeometry("to", value)} /></section>}
          {(selected.type === "segment3d" || selected.type === "vector3d") && <section><h3>線種</h3><select className="input" value={selected.geometry.lineType === "dashed" ? "dashed" : "solid"} onChange={(event) => updateGeometry("lineType", event.target.value)}><option value="solid">実線</option><option value="dashed">破線（補助線）</option></select></section>}
          {(selected.type === "sectionPlane" || selected.type === "plane3d") && <section><CoordinateInput label="平面上の点（数学xyz）" value={(selected.geometry.point as Vec3) ?? [0, 0, 0]} onChange={(value) => updateGeometry("point", value)} /><CoordinateInput label="法線（数学xyz）" value={(selected.geometry.normal as Vec3) ?? [0, 1, 0]} onChange={(value) => updateGeometry("normal", value)} /></section>}
          {selected.type === "label3d" && <section><h3>ラベル</h3><input className="input" value={String(selected.geometry.text ?? "")} onChange={(event) => updateGeometry("text", event.target.value)} /><CoordinateInput label="位置（数学xyz）" value={(selected.geometry.position as Vec3) ?? [0, 0, 0]} onChange={(value) => updateGeometry("position", value)} /></section>}
          {editableVertexCount > 0 && <section><h3>頂点名</h3><div className="spatial-vertex-list">{Array.from({ length: editableVertexCount }, (_, index) => { const names = Array.isArray(selected.geometry.vertexNames) ? selected.geometry.vertexNames.map(String) : []; return <label key={index}><span>{index + 1}</span><input className="input" maxLength={30} value={names[index] ?? ""} onChange={(event) => { const next = [...names]; next[index] = event.target.value; updateGeometry("vertexNames", next); }} /></label>; })}</div></section>}
          <section><h3>線・面・ラベル</h3><label className="spatial-color"><span>実線</span><input type="color" value={selected.style.lineColor} onChange={(event) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, lineColor: event.target.value } }))} /></label><NumberInput label="線幅" value={selected.style.lineWidth} min={0.25} max={12} onChange={(value) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, lineWidth: value } }))} /><label className="spatial-color"><span>面</span><input type="color" value={selected.style.faceColor} onChange={(event) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, faceColor: event.target.value } }))} /></label><NumberInput label="面透明度" value={selected.style.faceOpacity} step={0.05} min={0} max={1} onChange={(value) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, faceOpacity: value } }))} /><label className="spatial-color"><span>ラベル色</span><input type="color" value={selected.style.labelColor} onChange={(event) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, labelColor: event.target.value } }))} /></label><NumberInput label="初期ラベルの大きさ" value={selected.style.labelFontSize} min={8} max={72} step={1} onChange={(value) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, labelFontSize: value } }))} /><label className="spatial-quality">ラベル背景<select className="input" value={selected.style.labelBackground} onChange={(event) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, labelBackground: event.target.value as "transparent" | "white" } }))}><option value="transparent">透明</option><option value="white">白</option></select></label>{hasEditablePoints && <><label className="spatial-color"><span>点・頂点の色</span><input type="color" value={selected.style.pointColor} onChange={(event) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, pointColor: event.target.value } }))} /></label><NumberInput label="点・頂点の大きさ" value={selected.style.pointSize} min={0.03} max={1} step={0.01} onChange={(value) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, pointSize: value } }))} /></>}</section>
          {topology && <section><h3>辺ごとの指定</h3><div className="spatial-edge-list">{topology.edges.map((edge) => { const key = `${Math.min(...edge)}-${Math.max(...edge)}`; return <label key={key}><span>{key}</span><select className="input" value={selected.style.edgeOverrides[key] ?? "auto"} onChange={(event) => updateObject(selected.id, (object) => ({ ...object, style: { ...object.style, edgeOverrides: { ...object.style.edgeOverrides, [key]: event.target.value as EdgeDisplay } } }))}><option value="auto">自動</option><option value="solid">実線</option><option value="dashed">破線</option><option value="hidden">非表示</option></select></label>; })}</div></section>}
        </> : <div className="empty-state">オブジェクトをタップして編集します</div>}
        <section><h3>書き出し枠とPDF用紙</h3><div className="spatial-inline-actions"><button className="btn btn-sm" onClick={() => commit((current) => ({ ...current, output: { ...current.output, widthMm: 297, heightMm: 210 } }))}>A4横</button><button className="btn btn-sm" onClick={() => commit((current) => ({ ...current, output: { ...current.output, widthMm: 210, heightMm: 297 } }))}>A4縦</button><button className="btn btn-sm" onClick={() => commit((current) => ({ ...current, output: { ...current.output, widthMm: 150, heightMm: 150 } }))}>正方形</button></div><NumberInput label="枠の幅（mm）" value={documentValue.output.widthMm} min={10} max={1_000} step={1} onChange={(value) => commit((current) => ({ ...current, output: { ...current.output, widthMm: value } }))} /><NumberInput label="枠の高さ（mm）" value={documentValue.output.heightMm} min={10} max={1_000} step={1} onChange={(value) => commit((current) => ({ ...current, output: { ...current.output, heightMm: value } }))} /><NumberInput label="SVG横画素" value={documentValue.output.pixelWidth} min={400} max={8_000} step={100} onChange={(value) => commit((current) => ({ ...current, output: { ...current.output, pixelWidth: Math.round(value) } }))} /><div className="spatial-export-grid"><button className="btn btn-sm" disabled={busy} onClick={() => void runExport("png")}>PNG</button><button className="btn btn-sm" disabled={busy} onClick={() => void runExport("svg")}>SVG</button><button className="btn btn-sm" disabled={busy} onClick={() => void runExport("pdf")}>PDF</button><button className="btn btn-sm" disabled={busy} onClick={() => void runExport("tex")}>TikZ</button><button className="btn btn-sm" disabled={busy} onClick={() => void runExport("json")}>編集JSON</button><button className="btn btn-sm" onClick={() => importRef.current?.click()}>JSON読込</button></div><label className="spatial-quality">描画品質<select className="input" value={documentValue.scene.quality} onChange={(event) => commit((current) => ({ ...current, scene: { ...current.scene, quality: event.target.value as SpatialGeometryDocument["scene"]["quality"] } }))}><option value="low">低負荷</option><option value="standard">標準</option><option value="high">高品質</option></select></label></section>
      </aside>
    </div>
    <input ref={importRef} type="file" hidden accept=".json,.spatial-graph.json,application/json" onChange={(event) => { const file = event.target.files?.[0]; event.currentTarget.value = ""; if (file) void importJson(file); }} />
    {pdfPreview && <Modal title="空間図形PDFプレビュー" wide onClose={() => { if (pdfPreview.startsWith("blob:")) URL.revokeObjectURL(pdfPreview); setPdfPreview(null); }}><div className="h-[70vh] min-h-[360px]"><PdfCanvasViewer src={pdfPreview} zoom={100} /></div>{isTauri && <div className="mt-3 flex justify-end"><button className="btn btn-primary" onClick={async () => downloadBytes(`${safeBase(documentValue.title)}.pdf`, new Uint8Array(await (await fetch(pdfPreview)).arrayBuffer()), "application/pdf")}>PDFをこの端末へ保存</button></div>}</Modal>}
    {versions && <Modal title="空間図形の更新履歴" onClose={() => setVersions(null)}>{versions.length ? <div className="max-h-[60vh] space-y-2 overflow-y-auto">{versions.map((version) => <div className="card flex items-center gap-3 p-3" key={version.id}><div className="min-w-0 flex-1"><strong>v{version.version}・{version.title}</strong><div className="text-xs">{version.savedAt}</div></div><button className="btn btn-sm" onClick={async () => { if (!(await confirm(`v${version.version}を復元しますか？`))) return; try { await restoreGraphVersion(version.id, record.version); setVersions(null); await onClose(); await onReload(); showToast("履歴を復元しました。もう一度開いてください"); } catch (error) { showToast(String(error), "error"); } }}>復元</button></div>)}</div> : <p>過去版はありません。</p>}</Modal>}
    {aiOpen && <Modal title="Codexで空間図形の下書きを作成" wide onClose={() => { if (aiJob && ["queued", "preprocessing", "waiting_for_codex", "converting", "validating"].includes(aiJob.status)) void aiCancelJob(aiJob.id); setAiOpen(false); }}>
      <div className="grid gap-4 md:grid-cols-[minmax(260px,0.8fr)_minmax(320px,1.2fr)]">
        <div className="flex flex-col gap-3"><div className="grid grid-cols-3 gap-1">{([ ["spatial-geometry-from-text", "テキスト"], ["spatial-geometry-from-image", "画像"], ["spatial-geometry-from-problem", "問題文"] ] as const).map(([mode, label]) => <button className={`btn ${aiMode === mode ? "btn-outline" : "btn-ghost"}`} key={mode} onClick={() => { setAiMode(mode); setAiJob(null); }}>{label}</button>)}</div>
          {aiMode === "spatial-geometry-from-image" ? <label className="rounded border border-dashed p-3 text-xs"><span className="mb-2 block">図・プリント・手書き空間図形の画像</span><input type="file" accept="image/png,image/jpeg,image/webp" onChange={(event) => setAiImage(event.target.files?.[0] ?? null)} />{aiImage && <span className="mt-2 block">{aiImage.name}</span>}</label> : <label><span className="mb-1 block text-xs">{aiMode === "spatial-geometry-from-problem" ? "空間図形を必要とする問題文" : "立体・頂点名・追加する辺・投影方式"}</span><textarea className="input min-h-44 resize-y text-sm" value={aiText} placeholder="例: 一辺4の立方体ABCD-EFGH。頂点AとGを結び、見えない辺は破線。平行投影。" onChange={(event) => setAiText(event.target.value)} /></label>}
          {aiMode === "spatial-geometry-from-image" && <textarea className="input min-h-20 text-sm" value={aiText} placeholder="補足（任意）" onChange={(event) => setAiText(event.target.value)} />}
          <div className="rounded p-2 text-[11px]" style={{ background: "var(--panel-2)", color: "var(--muted)" }}>AIは編集可能な下書きだけを返します。不明な頂点名や矛盾は警告として残し、確認するまで保存へ確定しません。</div>
          <button className="btn btn-primary" disabled={busy || !!aiJob && ["queued", "preprocessing", "waiting_for_codex", "converting", "validating"].includes(aiJob.status)} onClick={() => void startSpatialAi()}>{aiJob && !["completed", "failed", "cancelled"].includes(aiJob.status) ? aiJob.progressMessage || "生成中…" : "空間図形の下書きを生成"}</button>{aiJob?.status === "failed" && <div className="err-msg">{aiJob.errorMessage || "生成に失敗しました"}</div>}
        </div>
        <div className="min-h-[360px] rounded border p-3" style={{ borderColor: "var(--border)", background: "#eef2f7" }}>{!aiJob || !isSpatialResult(aiJob.structuredResult) ? <div className="flex min-h-[330px] items-center justify-center text-xs" style={{ color: "var(--muted)" }}>{aiJob ? aiJob.progressMessage || aiJob.status : "生成結果の構成と警告がここに表示されます"}</div> : <div className="flex h-full flex-col gap-3"><h3 className="font-bold">{aiJob.structuredResult.spatialSpec.title}</h3><div className="grid grid-cols-2 gap-2 text-sm"><span>立体 {aiJob.structuredResult.spatialSpec.solids.length}件</span><span>線分 {aiJob.structuredResult.spatialSpec.segments.length}件</span><span>点 {aiJob.structuredResult.spatialSpec.points.length}件</span><span>投影 {aiJob.structuredResult.spatialSpec.projection.type === "orthographic" ? "平行" : "透視"}</span></div>{aiJob.warnings.length > 0 && <div className="warn-msg">{aiJob.warnings.map((warning) => warning.message).join("\n")}</div>}{aiJob.uncertainFragments.length > 0 && <div className="warn-msg">要確認: {aiJob.uncertainFragments.map((item) => item.description).join(" / ")}</div>}<div className="mt-auto flex justify-end gap-2"><button className="btn" onClick={() => { setAiJob(null); void startSpatialAi(); }}>再解析</button><button className="btn btn-primary" onClick={acceptSpatialAi}>確認して編集画面へ読み込む</button></div></div>}</div>
      </div>
    </Modal>}
  </div>;
}
