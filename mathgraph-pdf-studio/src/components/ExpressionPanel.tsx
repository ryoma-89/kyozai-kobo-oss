import { useEffect, useMemo, useState } from "react";
import katex from "katex";
import {
  Plus,
  Trash2,
  Eye,
  EyeOff,
  GripVertical,
  MapPin,
  Sigma,
} from "lucide-react";
import type { ExprItem, FillStyle, MathLabel, PointItem } from "../types";
import { normalizeMathLabelLatex } from "../lib/mathlabel";
import type { RenderItem } from "../lib/buildSvg";
import { evalScalarInput, formatScalar } from "../lib/scalarInput";

interface Props {
  items: RenderItem[];
  points: PointItem[];
  labels: MathLabel[];
  selectedLabelId: string | null;
  mathReady: boolean;
  warnings: Map<string, string>;
  onAdd: () => void;
  onUpdate: (id: string, patch: Partial<ExprItem>) => void;
  onRemove: (id: string) => void;
  onReorder: (from: number, to: number) => void;
  onAddPoint: () => void;
  onUpdatePoint: (id: string, patch: Partial<PointItem>) => void;
  onRemovePoint: (id: string) => void;
  onAddLabel: () => void;
  onUpdateLabel: (id: string, patch: Partial<MathLabel>) => void;
  onRemoveLabel: (id: string) => void;
  onSelectLabel: (id: string | null) => void;
}

const FILL_STYLE_OPTIONS: Array<{ value: FillStyle; label: string }> = [
  { value: "solid", label: "ベタ塗り" },
  { value: "hatch", label: "斜線" },
  { value: "crosshatch", label: "網掛け" },
  { value: "dot", label: "点描" },
];

function toFillStyle(v: string): FillStyle {
  return v === "hatch" || v === "crosshatch" || v === "dot" ? v : "solid";
}

function ScalarField({
  label,
  value,
  onCommit,
}: {
  label: string;
  value: number;
  onCommit: (v: number) => void;
}) {
  const [draft, setDraft] = useState(formatScalar(value));
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    setDraft(formatScalar(value));
    setInvalid(false);
  }, [value]);

  const commit = () => {
    const parsed = evalScalarInput(draft);
    if (parsed == null) {
      setInvalid(true);
      setDraft(formatScalar(value));
      return;
    }
    setInvalid(false);
    setDraft(formatScalar(parsed));
    onCommit(parsed);
  };

  return (
    <label className="block min-w-0">
      <span className="label">{label}</span>
      <input
        className="input !py-1 text-[11.5px]"
        value={draft}
        inputMode="decimal"
        title={`${label}座標（例: 1/2, pi, pi/2, e, sqrt(2)）`}
        style={invalid ? { borderColor: "var(--danger)" } : undefined}
        onChange={(e) => {
          setDraft(e.target.value);
          setInvalid(false);
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        }}
      />
    </label>
  );
}

function KatexPreview({ latex, input }: { latex: string; input: string }) {
  const html = useMemo(() => {
    if (!latex) return "";
    try {
      return katex.renderToString(latex, {
        throwOnError: false,
        displayMode: false,
        strict: false,
      });
    } catch {
      return "";
    }
  }, [latex]);
  if (!input.trim()) {
    return (
      <div className="katex-preview" style={{ color: "var(--text-faint)" }}>
        式を入力するとここに数式が表示されます
      </div>
    );
  }
  if (!html) return null;
  return (
    <div className="katex-preview" dangerouslySetInnerHTML={{ __html: html }} />
  );
}

function ExprCard({
  ri,
  index,
  warning,
  onUpdate,
  onRemove,
  dragIndex,
  setDragIndex,
  dropIndex,
  setDropIndex,
  onReorder,
}: {
  ri: RenderItem;
  index: number;
  warning?: string;
  onUpdate: Props["onUpdate"];
  onRemove: Props["onRemove"];
  dragIndex: number | null;
  setDragIndex: (i: number | null) => void;
  dropIndex: number | null;
  setDropIndex: (i: number | null) => void;
  onReorder: Props["onReorder"];
}) {
  const { item, parsed } = ri;
  const isIneq = parsed.ok && parsed.isInequality;
  const isParametric = parsed.ok && parsed.kind === "parametric";

  return (
    <div
      className="expr-card"
      data-hidden={!item.visible}
      data-dragover={dropIndex === index && dragIndex !== null && dragIndex !== index}
      onDragOver={(e) => {
        if (dragIndex === null) return;
        e.preventDefault();
        setDropIndex(index);
      }}
      onDrop={(e) => {
        e.preventDefault();
        if (dragIndex !== null) onReorder(dragIndex, index);
        setDragIndex(null);
        setDropIndex(null);
      }}
    >
      {/* 1行目: ドラッグ・表示・入力・削除 */}
      <div className="flex items-center gap-1.5">
        <span
          draggable
          onDragStart={(e) => {
            setDragIndex(index);
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragEnd={() => {
            setDragIndex(null);
            setDropIndex(null);
          }}
          className="cursor-grab active:cursor-grabbing flex-none"
          style={{ color: "var(--text-faint)" }}
          title="ドラッグで並び替え"
        >
          <GripVertical size={15} />
        </span>
        <button
          className="btn-icon flex-none"
          title={item.visible ? "非表示にする" : "表示する"}
          onClick={() => onUpdate(item.id, { visible: !item.visible })}
          style={item.visible ? { color: "var(--accent)" } : undefined}
        >
          {item.visible ? <Eye size={15} /> : <EyeOff size={15} />}
        </button>
        <input
          className="input input-math flex-1"
          value={item.input}
          placeholder="例: y = x^2 - 4*x + 3"
          onChange={(e) => onUpdate(item.id, { input: e.target.value })}
          spellCheck={false}
        />
        <button
          className="btn-icon flex-none"
          title="削除"
          onClick={() => onRemove(item.id)}
          style={{ color: "var(--danger)" }}
        >
          <Trash2 size={14} />
        </button>
      </div>

      {/* 2行目: KaTeXプレビュー / エラー */}
      <div className="mt-1.5">
        {parsed.ok ? (
          <KatexPreview latex={parsed.latex} input={item.input} />
        ) : (
          <>
            <KatexPreview latex="" input="" />
            {item.input.trim() !== "" && (
              <div className="err-msg">{parsed.message}</div>
            )}
          </>
        )}
        {warning && <div className="warn-msg">{warning}</div>}
      </div>

      {/* 3行目: スタイル設定 */}
      <div className="mt-2 flex items-center gap-2 flex-wrap">
        <label className="flex items-center gap-1" title="線の色">
          <input
            type="color"
            value={item.color}
            onChange={(e) => onUpdate(item.id, { color: e.target.value })}
          />
        </label>
        <label
          className="flex items-center gap-1.5 flex-1 min-w-[90px]"
          title={`線の太さ: ${item.lineWidth}px`}
        >
          <span className="text-[10.5px]" style={{ color: "var(--text-dim)" }}>
            太さ
          </span>
          <input
            type="range"
            min={0.5}
            max={6}
            step={0.5}
            value={item.lineWidth}
            className="flex-1"
            onChange={(e) =>
              onUpdate(item.id, { lineWidth: Number(e.target.value) })
            }
          />
        </label>
        <select
          className="input !w-[68px] !py-1 text-[11.5px]"
          value={item.lineStyle}
          onChange={(e) =>
            onUpdate(item.id, {
              lineStyle: e.target.value === "dashed" ? "dashed" : "solid",
            })
          }
          title="線種"
        >
          <option value="solid">実線</option>
          <option value="dashed">破線</option>
        </select>
      </div>

      {isIneq && (
        <div className="mt-1.5 flex items-center gap-2 flex-wrap">
          <label className="flex items-center gap-1" title="塗りつぶし色">
            <input
              type="color"
              value={item.fillColor}
              onChange={(e) => onUpdate(item.id, { fillColor: e.target.value })}
            />
          </label>
          <label
            className="flex items-center gap-1.5 flex-1 min-w-[90px]"
            title={`塗りつぶし透明度: ${Math.round(item.fillOpacity * 100)}%`}
          >
            <span className="text-[10.5px]" style={{ color: "var(--text-dim)" }}>
              塗り {Math.round(item.fillOpacity * 100)}%
            </span>
            <input
              type="range"
              min={0}
              max={0.8}
              step={0.05}
              value={item.fillOpacity}
              className="flex-1"
              onChange={(e) =>
                onUpdate(item.id, { fillOpacity: Number(e.target.value) })
              }
            />
          </label>
          <select
            className="input !w-[76px] !py-1 text-[11.5px]"
            value={item.fillStyle}
            onChange={(e) => onUpdate(item.id, { fillStyle: toFillStyle(e.target.value) })}
            title="塗りつぶし方式"
          >
            {FILL_STYLE_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </div>
      )}

      {isParametric && (
        <div className="mt-1.5 flex items-center gap-1.5">
          <span className="text-[10.5px]" style={{ color: "var(--text-dim)" }}>
            t:
          </span>
          <input
            className="input !w-[64px] !py-1 text-[11.5px]"
            type="number"
            step="any"
            value={Math.round(item.tmin * 1000) / 1000}
            title="t 最小値"
            onChange={(e) => onUpdate(item.id, { tmin: Number(e.target.value) })}
          />
          <span className="text-[10.5px]" style={{ color: "var(--text-faint)" }}>
            〜
          </span>
          <input
            className="input !w-[64px] !py-1 text-[11.5px]"
            type="number"
            step="any"
            value={Math.round(item.tmax * 1000) / 1000}
            title="t 最大値"
            onChange={(e) => onUpdate(item.id, { tmax: Number(e.target.value) })}
          />
          <button
            className="btn !px-1.5 !py-1 text-[10.5px]"
            title="0〜2π に設定"
            onClick={() => onUpdate(item.id, { tmin: 0, tmax: Math.round(2 * Math.PI * 1000) / 1000 })}
          >
            2π
          </button>
        </div>
      )}

      <div className="mt-1.5">
        <input
          className="input !py-1 text-[11.5px]"
          value={item.name}
          placeholder="凡例名（空欄なら式を表示）"
          onChange={(e) => onUpdate(item.id, { name: e.target.value })}
        />
      </div>
    </div>
  );
}

/** 生の LaTeX を KaTeX で表示（ラベルのプレビュー用） */
function KatexRaw({ latex }: { latex: string }) {
  const html = useMemo(() => {
    const normalized = normalizeMathLabelLatex(latex);
    if (!normalized) return "";
    try {
      return katex.renderToString(normalized, {
        throwOnError: false,
        displayMode: false,
        strict: false,
      });
    } catch {
      return "";
    }
  }, [latex]);
  return (
    <div className="katex-preview !min-h-[26px]">
      {latex.trim() ? (
        <span dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <span style={{ color: "var(--text-faint)" }}>LaTeXを入力</span>
      )}
    </div>
  );
}

function LabelCard({
  label,
  selected,
  onUpdate,
  onRemove,
  onSelect,
}: {
  label: MathLabel;
  selected: boolean;
  onUpdate: Props["onUpdateLabel"];
  onRemove: Props["onRemoveLabel"];
  onSelect: Props["onSelectLabel"];
}) {
  return (
    <div
      className="expr-card !p-2"
      data-hidden={!label.visible}
      style={selected ? { borderColor: "var(--accent)", boxShadow: "0 0 0 1px var(--accent)" } : undefined}
      onMouseDown={() => onSelect(label.id)}
    >
      <div className="flex items-center gap-1.5">
        <button
          className="btn-icon flex-none"
          title={label.visible ? "非表示にする" : "表示する"}
          onClick={() => onUpdate(label.id, { visible: !label.visible })}
          style={label.visible ? { color: "var(--accent)" } : undefined}
        >
          {label.visible ? <Eye size={14} /> : <EyeOff size={14} />}
        </button>
        <input
          className="input input-math flex-1"
          value={label.latex}
          placeholder="例: b=(a+1)^2-1"
          spellCheck={false}
          onChange={(e) => onUpdate(label.id, { latex: e.target.value })}
        />
        <button
          className="btn-icon flex-none"
          title="削除"
          onClick={() => onRemove(label.id)}
          style={{ color: "var(--danger)" }}
        >
          <Trash2 size={13} />
        </button>
      </div>
      <div className="mt-1.5">
        <KatexRaw latex={label.latex} />
      </div>
      <div className="mt-1.5 flex items-center gap-2 flex-wrap">
        <input
          type="color"
          value={label.color}
          title="文字色"
          onChange={(e) => onUpdate(label.id, { color: e.target.value })}
        />
        <label
          className="flex items-center gap-1.5 flex-1 min-w-[90px]"
          title={`文字サイズ: ${label.fontSize}`}
        >
          <span className="text-[10.5px]" style={{ color: "var(--text-dim)" }}>
            大きさ
          </span>
          <input
            type="range"
            min={10}
            max={44}
            step={1}
            value={label.fontSize}
            className="flex-1"
            onChange={(e) => onUpdate(label.id, { fontSize: Number(e.target.value) })}
          />
        </label>
        <input
          className="input !w-[52px] !py-1 text-[11.5px]"
          type="number"
          value={Math.round(label.x * 100) / 100}
          step="any"
          title="x座標"
          onChange={(e) => onUpdate(label.id, { x: Number(e.target.value) || 0 })}
        />
        <input
          className="input !w-[52px] !py-1 text-[11.5px]"
          type="number"
          value={Math.round(label.y * 100) / 100}
          step="any"
          title="y座標"
          onChange={(e) => onUpdate(label.id, { y: Number(e.target.value) || 0 })}
        />
      </div>
    </div>
  );
}

export default function ExpressionPanel(props: Props) {
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);

  return (
    <div className="p-3 flex flex-col gap-3">
      <div className="section-title">式・オブジェクト</div>

      <button className="btn btn-primary w-full" onClick={props.onAdd}>
        <Plus size={15} /> 式を追加
      </button>

      <div className="flex flex-col gap-2">
        {props.items.map((ri, i) => (
          <ExprCard
            key={ri.item.id}
            ri={ri}
            index={i}
            warning={props.warnings.get(ri.item.id)}
            onUpdate={props.onUpdate}
            onRemove={props.onRemove}
            dragIndex={dragIndex}
            setDragIndex={setDragIndex}
            dropIndex={dropIndex}
            setDropIndex={setDropIndex}
            onReorder={props.onReorder}
          />
        ))}
        {props.items.length === 0 && (
          <div
            className="text-center py-6 text-[12px]"
            style={{ color: "var(--text-faint)" }}
          >
            「式を追加」から数式・不等式を入力してください
          </div>
        )}
      </div>

      <div className="section-title mt-2">点（交点・重要点）</div>
      <button className="btn w-full" onClick={props.onAddPoint}>
        <MapPin size={14} /> 点を追加
      </button>
      <div className="flex flex-col gap-1.5">
        {props.points.map((pt) => (
          <div key={pt.id} className="expr-card !p-2 flex flex-col gap-1.5">
            <div className="flex items-center gap-1.5">
              <button
                className="btn-icon flex-none"
                onClick={() => props.onUpdatePoint(pt.id, { visible: !pt.visible })}
                style={pt.visible ? { color: "var(--accent)" } : undefined}
              >
                {pt.visible ? <Eye size={14} /> : <EyeOff size={14} />}
              </button>
              <input
                type="color"
                value={pt.color}
                onChange={(e) => props.onUpdatePoint(pt.id, { color: e.target.value })}
              />
              <input
                className="input min-w-0 flex-1 !py-1 text-[11.5px]"
                value={pt.label}
                placeholder="ラベル"
                onChange={(e) => props.onUpdatePoint(pt.id, { label: e.target.value })}
              />
              <button
                className="btn-icon flex-none"
                onClick={() => props.onRemovePoint(pt.id)}
                style={{ color: "var(--danger)" }}
              >
                <Trash2 size={13} />
              </button>
            </div>
            <div className="grid grid-cols-2 gap-1.5">
              <ScalarField
                label="x"
                value={pt.x}
                onCommit={(x) => props.onUpdatePoint(pt.id, { x })}
              />
              <ScalarField
                label="y"
                value={pt.y}
                onCommit={(y) => props.onUpdatePoint(pt.id, { y })}
              />
            </div>
            <div className="grid grid-cols-2 gap-1.5 text-[11px]" style={{ color: "var(--text-dim)" }}>
              <label className="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={pt.showProjectionToXAxis}
                  onChange={(e) =>
                    props.onUpdatePoint(pt.id, { showProjectionToXAxis: e.target.checked })
                  }
                />
                x軸へ垂線
              </label>
              <label className="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  checked={pt.showProjectionToYAxis}
                  onChange={(e) =>
                    props.onUpdatePoint(pt.id, { showProjectionToYAxis: e.target.checked })
                  }
                />
                y軸へ垂線
              </label>
            </div>
          </div>
        ))}
      </div>

      <div className="section-title mt-2">数式ラベル</div>
      <button className="btn w-full" onClick={props.onAddLabel}>
        <Sigma size={14} /> 数式ラベルを追加
      </button>
      {!props.mathReady && (
        <div className="text-[10.5px]" style={{ color: "var(--text-faint)" }}>
          数式組版エンジンを読み込み中…
        </div>
      )}
      <div className="flex flex-col gap-1.5">
        {props.labels.map((lb) => (
          <LabelCard
            key={lb.id}
            label={lb}
            selected={props.selectedLabelId === lb.id}
            onUpdate={props.onUpdateLabel}
            onRemove={props.onRemoveLabel}
            onSelect={props.onSelectLabel}
          />
        ))}
      </div>

      <div
        className="text-[10.5px] leading-relaxed mt-1 pb-4"
        style={{ color: "var(--text-faint)" }}
      >
        対応例: <code>y = x^2 - 4*x + 3</code> / <code>y &gt;= x^2</code> /{" "}
        <code>x^2 + y^2 &lt;= 9</code> / <code>1 &lt;= x &lt;= 3</code> /{" "}
        <code>0 &lt;= y &lt;= x^3</code>
        <br />
        <code>x = y^2</code>（x=f(y)型）/ <code>y = [x]</code>（ガウス記号）/{" "}
        <code>x*y = 1</code>（陰関数 f(x,y)=0）/{" "}
        <code>(cos(t), sin(t))</code>（媒介変数表示、t範囲を指定可）にも対応
        <br />
        領域の結合: <code>and</code>（かつ）/ <code>or</code>（または）で不等式をつなぐと、
        場合分けの和集合や共通部分をひとつの領域として塗れます。例:{" "}
        <code>(x&gt;=0 and y&lt;=x^2) or (x&lt;0 and y&lt;=-x)</code>
        <br />
        曲線の切り取り: <code>y=x^2 and x&gt;=0</code> /{" "}
        <code>x^2+y^2=9 and y&gt;=0</code> のように、曲線1本を不等式条件で切り取れます。
        <br />
        数式ラベルはグラフ上をドラッグで自由に移動できます（LaTeX記法。例:{" "}
        <code>b=(a+1)^2-1</code>）。貼り付け時の <code>$...$</code> / <code>\(...\)</code> / <code>\[...\]</code> は自動的に外します。
        <br />
        点の座標には <code>1/2</code> / <code>pi</code> / <code>pi/2</code> /{" "}
        <code>e</code> / <code>sqrt(2)</code> なども入力できます。
        <br />
        LaTeX入力対応: <code>{"y=\\frac{x^2}{4}"}</code> /{" "}
        <code>{"y=\\sqrt{x+2}"}</code> /{" "}
        <code>{"x^2+y^2\\leq 9"}</code> のように貼り付けても解釈します。
        <br />
        関数: sqrt, abs, sin, cos, tan, log(自然対数), ln, exp, pi など
      </div>
    </div>
  );
}
