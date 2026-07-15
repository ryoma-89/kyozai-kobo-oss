import { useEffect, useState } from "react";
import {
  FileDown,
  Image,
  FileCode,
  Save,
  FolderOpen,
  FilePlus2,
} from "lucide-react";
import type { PaperSettings, ViewRange } from "../types";
import { fmtCoord, type Intersection } from "../lib/intersections";
import { clampAspectRatio } from "../lib/aspect";

interface Props {
  range: ViewRange;
  paper: PaperSettings;
  intersections: Intersection[];
  busy: boolean;
  onRangeChange: (patch: Partial<ViewRange>) => void;
  onPaperChange: (patch: Partial<PaperSettings>) => void;
  onPdf: () => void;
  onPng: () => void;
  onSvg: () => void;
  onSaveProject: () => void;
  onOpenProject: () => void;
  onNewProject: () => void;
}

/** blur / Enter で確定する数値入力 */
function NumField({
  label,
  value,
  onCommit,
}: {
  label: string;
  value: number;
  onCommit: (v: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => {
    setDraft(String(parseFloat(value.toFixed(6))));
  }, [value]);
  const commit = () => {
    const n = parseFloat(draft);
    if (Number.isFinite(n)) onCommit(n);
    else setDraft(String(value));
  };
  return (
    <label className="block">
      <span className="label">{label}</span>
      <input
        className="input"
        value={draft}
        inputMode="decimal"
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        }}
      />
    </label>
  );
}

function ToggleRow({
  label,
  on,
  onChange,
}: {
  label: string;
  on: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      className="flex items-center justify-between w-full py-1 cursor-pointer bg-transparent border-0 text-left"
      style={{ color: "var(--text)" }}
      onClick={() => onChange(!on)}
    >
      <span className="text-[12px]">{label}</span>
      <span className="toggle" data-on={on} />
    </button>
  );
}

export default function SettingsPanel(props: Props) {
  const { range, paper } = props;
  return (
    <div className="p-3 flex flex-col gap-4 pb-6">
      {/* 範囲設定 */}
      <div>
        <div className="section-title mb-2">グラフ範囲</div>
        <div className="grid grid-cols-2 gap-2">
          <NumField label="x 最小" value={range.xmin} onCommit={(v) => props.onRangeChange({ xmin: v })} />
          <NumField label="x 最大" value={range.xmax} onCommit={(v) => props.onRangeChange({ xmax: v })} />
          <NumField label="y 最小" value={range.ymin} onCommit={(v) => props.onRangeChange({ ymin: v })} />
          <NumField label="y 最大" value={range.ymax} onCommit={(v) => props.onRangeChange({ ymax: v })} />
          <NumField label="x 目盛り間隔" value={range.xstep} onCommit={(v) => props.onRangeChange({ xstep: Math.abs(v) })} />
          <NumField label="y 目盛り間隔" value={range.ystep} onCommit={(v) => props.onRangeChange({ ystep: Math.abs(v) })} />
        </div>
        {(range.xmin >= range.xmax || range.ymin >= range.ymax) && (
          <div className="err-msg">最小値は最大値より小さくしてください。</div>
        )}
      </div>

      {/* 表示設定 */}
      <div>
        <div className="section-title mb-1">表示</div>
        <ToggleRow label="座標軸を表示" on={paper.showAxes} onChange={(v) => props.onPaperChange({ showAxes: v })} />
        {paper.showAxes && (
          <div className="mb-1.5 mt-0.5">
            <div className="flex items-center gap-1.5">
              <label className="flex-1">
                <span className="label">x軸名</span>
                <input
                  className="input input-math !py-1 text-[12px]"
                  value={paper.axisLabelX}
                  placeholder="x"
                  onChange={(e) => props.onPaperChange({ axisLabelX: e.target.value })}
                />
              </label>
              <label className="flex-1">
                <span className="label">y軸名</span>
                <input
                  className="input input-math !py-1 text-[12px]"
                  value={paper.axisLabelY}
                  placeholder="y"
                  onChange={(e) => props.onPaperChange({ axisLabelY: e.target.value })}
                />
              </label>
              <label className="w-[56px]">
                <span className="label">原点</span>
                <input
                  className="input input-math !py-1 text-[12px]"
                  value={paper.axisLabelO}
                  placeholder="O"
                  onChange={(e) => props.onPaperChange({ axisLabelO: e.target.value })}
                />
              </label>
            </div>
            <label className="block mt-1">
              <span className="label">軸ラベルの大きさ: {paper.axisLabelSize}</span>
              <input
                type="range"
                min={10}
                max={34}
                step={1}
                className="w-full"
                value={paper.axisLabelSize}
                onChange={(e) => props.onPaperChange({ axisLabelSize: Number(e.target.value) })}
              />
            </label>
            <div className="text-[10.5px] leading-snug" style={{ color: "var(--text-faint)" }}>
              軸名は数式フォントで組版されます（LaTeX記法。例: <code>\theta</code>）。
            </div>
          </div>
        )}
        <ToggleRow label="軸の目盛りを表示" on={paper.showTicks} onChange={(v) => props.onPaperChange({ showTicks: v })} />
        <ToggleRow label="方眼を表示" on={paper.showGrid} onChange={(v) => props.onPaperChange({ showGrid: v })} />
        <ToggleRow label="凡例を表示" on={paper.showLegend} onChange={(v) => props.onPaperChange({ showLegend: v })} />
        {paper.showLegend && (
          <label className="block mt-1 mb-1">
            <span className="label">凡例の文字サイズ: {paper.legendFontSize}</span>
            <input
              type="range"
              min={9}
              max={28}
              step={1}
              className="w-full"
              value={paper.legendFontSize}
              onChange={(e) => props.onPaperChange({ legendFontSize: Number(e.target.value) })}
            />
          </label>
        )}
        <div className="mt-1.5 mb-1.5">
          <span className="label">グラフの縦横比</span>
          <div className="grid grid-cols-2 gap-1.5">
            {(
              [
                ["range", "座標単位を正方形"],
                ["custom", "指定比率"],
              ] as const
            ).map(([mode, label]) => (
              <button
                key={mode}
                className="btn"
                style={
                  paper.aspectMode === mode
                    ? {
                        borderColor: "var(--accent)",
                        color: "var(--accent)",
                        background: "#0e2a3355",
                      }
                    : undefined
                }
                onClick={() => props.onPaperChange({ aspectMode: mode, lockAspect: true })}
              >
                {label}
              </button>
            ))}
          </div>
          {paper.aspectMode === "custom" && (
            <div className="mt-2 flex flex-col gap-1.5">
              <NumField
                label={`横÷縦: ${paper.customAspectRatio.toFixed(2)}`}
                value={paper.customAspectRatio}
                onCommit={(v) => props.onPaperChange({ customAspectRatio: clampAspectRatio(v) })}
              />
              <div className="grid grid-cols-4 gap-1">
                {[
                  ["1:1", 1],
                  ["4:3", 4 / 3],
                  ["16:9", 16 / 9],
                  ["3:4", 3 / 4],
                ].map(([label, ratio]) => (
                  <button
                    key={label}
                    className="btn !py-1 text-[11px]"
                    onClick={() => props.onPaperChange({ customAspectRatio: ratio as number })}
                  >
                    {label}
                  </button>
                ))}
              </div>
              <div className="text-[10.5px] leading-snug" style={{ color: "var(--text-faint)" }}>
                指定比率では、グラフを引き延ばさずに表示範囲を左右または上下へ広げます。画面表示と出力に反映されます。
              </div>
            </div>
          )}
        </div>
        <ToggleRow
          label="交点を自動検出して表示"
          on={paper.showIntersections}
          onChange={(v) => props.onPaperChange({ showIntersections: v })}
        />
        {paper.showIntersections && (
          <div className="mt-1 mb-1">
            <ToggleRow
              label="交点に座標を表示"
              on={paper.showIntersectionCoords}
              onChange={(v) => props.onPaperChange({ showIntersectionCoords: v })}
            />
            <div className="mt-1 text-[11px]" style={{ color: "var(--text-dim)" }}>
              {props.intersections.length === 0 ? (
                <span style={{ color: "var(--text-faint)" }}>
                  表示中の曲線どうしの交点はありません。
                </span>
              ) : (
                <>
                  <span style={{ color: "var(--accent)" }}>
                    {props.intersections.length} 個の交点
                  </span>
                  <div
                    className="mt-1 flex flex-col gap-0.5 max-h-[120px] overflow-y-auto rounded-md p-1.5"
                    style={{ background: "#0c0f16", border: "1px solid var(--border)" }}
                  >
                    {props.intersections.map((p, i) => (
                      <span key={i} className="font-mono text-[11px]" style={{ color: "var(--text-dim)" }}>
                        ( {fmtCoord(p.x)} , {fmtCoord(p.y)} )
                      </span>
                    ))}
                  </div>
                </>
              )}
            </div>
          </div>
        )}
      </div>

      {/* 不等式領域 */}
      <div>
        <div className="section-title mb-1">不等式領域</div>
        <ToggleRow
          label="共通部分のみ塗りつぶす"
          on={paper.regionMode === "intersection"}
          onChange={(v) => props.onPaperChange({ regionMode: v ? "intersection" : "overlay" })}
        />
        {paper.regionMode === "intersection" ? (
          <div className="mt-1.5 flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <input
                type="color"
                value={paper.intersectionColor}
                title="共通部分の色"
                onChange={(e) => props.onPaperChange({ intersectionColor: e.target.value })}
              />
              <select
                className="input flex-1 !py-1 text-[11.5px]"
                value={paper.intersectionStyle}
                title="共通部分の塗りつぶし方式"
                onChange={(e) => {
                  const v = e.target.value;
                  props.onPaperChange({
                    intersectionStyle:
                      v === "solid" || v === "crosshatch" || v === "dot" ? v : "hatch",
                  });
                }}
              >
                <option value="solid">ベタ塗り</option>
                <option value="hatch">斜線</option>
                <option value="crosshatch">網掛け</option>
                <option value="dot">点描</option>
              </select>
            </div>
            <label>
              <span className="label">
                濃さ: {Math.round(paper.intersectionOpacity * 100)}%
              </span>
              <input
                type="range"
                min={0.05}
                max={0.8}
                step={0.05}
                className="w-full"
                value={paper.intersectionOpacity}
                onChange={(e) =>
                  props.onPaperChange({ intersectionOpacity: Number(e.target.value) })
                }
              />
            </label>
            <div className="text-[10.5px] leading-relaxed" style={{ color: "var(--text-faint)" }}>
              表示中のすべての不等式を同時に満たす領域だけを塗ります。境界線は式ごとの設定で描かれます。
            </div>
          </div>
        ) : (
          <div className="text-[10.5px] leading-relaxed mt-1" style={{ color: "var(--text-faint)" }}>
            オフのときは各不等式の領域を半透明で重ねて表示します。
          </div>
        )}
      </div>

      {/* 用紙・教材情報 */}
      <div>
        <div className="section-title mb-2">用紙・教材情報</div>
        <ToggleRow
          label="PDFはグラフのみ出力"
          on={paper.pdfGraphOnly}
          onChange={(v) => props.onPaperChange({ pdfGraphOnly: v })}
        />
        <div className="text-[10.5px] leading-snug mb-2" style={{ color: "var(--text-faint)" }}>
          {paper.pdfGraphOnly
            ? "タイトルなしのグラフ用PDFを出力します。用紙比率を指定してもグラフ自体は伸縮しません。"
            : "A4 にタイトル・問題番号・説明文を付けた教材レイアウトで出力します。"}
        </div>

        {paper.pdfGraphOnly ? (
          <div className="flex flex-col gap-2">
            <label>
              <span className="label">グラフの幅: {paper.pdfGraphWidthMm} mm（高さは自動）</span>
              <input
                type="range"
                min={40}
                max={200}
                step={5}
                className="w-full"
                value={paper.pdfGraphWidthMm}
                onChange={(e) => props.onPaperChange({ pdfGraphWidthMm: Number(e.target.value) })}
              />
            </label>
            <div>
              <span className="label">PDF用紙比率</span>
              <div className="grid grid-cols-2 gap-1.5">
                {(
                  [
                    ["graph", "グラフに合わせる"],
                    ["custom", "指定比率"],
                  ] as const
                ).map(([mode, label]) => (
                  <button
                    key={mode}
                    className="btn"
                    style={
                      paper.pdfAspectMode === mode
                        ? {
                            borderColor: "var(--accent)",
                            color: "var(--accent)",
                            background: "#0e2a3355",
                          }
                        : undefined
                    }
                    onClick={() => props.onPaperChange({ pdfAspectMode: mode })}
                  >
                    {label}
                  </button>
                ))}
              </div>
              {paper.pdfAspectMode === "custom" && (
                <div className="mt-2 flex flex-col gap-1.5">
                  <NumField
                    label={`PDF 横÷縦: ${paper.pdfCustomAspectRatio.toFixed(2)}`}
                    value={paper.pdfCustomAspectRatio}
                    onCommit={(v) => props.onPaperChange({ pdfCustomAspectRatio: clampAspectRatio(v) })}
                  />
                  <div className="grid grid-cols-4 gap-1">
                    {[
                      ["1:1", 1],
                      ["4:3", 4 / 3],
                      ["16:9", 16 / 9],
                      ["3:4", 3 / 4],
                    ].map(([label, ratio]) => (
                      <button
                        key={label}
                        className="btn !py-1 text-[11px]"
                        onClick={() => props.onPaperChange({ pdfCustomAspectRatio: ratio as number })}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                  <div className="text-[10.5px] leading-snug" style={{ color: "var(--text-faint)" }}>
                    グラフ自体は伸縮せず、用紙側に余白を足して指定比率にします。
                  </div>
                </div>
              )}
            </div>
            <div
              className="text-[10.5px] leading-relaxed rounded-md p-2"
              style={{ background: "#0c0f16", border: "1px solid var(--border)", color: "var(--text-dim)" }}
            >
              LaTeX 例（余白なくそのまま貼れます）:
              <br />
              <code style={{ color: "var(--accent)" }}>
                \includegraphics[width=.6\linewidth]{"{"}図.pdf{"}"}
              </code>
              <br />
              ベクターPDFなので拡大しても劣化しません。
            </div>
          </div>
        ) : (
          <>
            <div className="flex gap-1.5 mb-2">
              {(
                [
                  ["portrait", "A4 縦"],
                  ["landscape", "A4 横"],
                ] as const
              ).map(([v, lb]) => (
                <button
                  key={v}
                  className="btn flex-1"
                  style={
                    paper.orientation === v
                      ? {
                          borderColor: "var(--accent)",
                          color: "var(--accent)",
                          background: "#0e2a3355",
                        }
                      : undefined
                  }
                  onClick={() => props.onPaperChange({ orientation: v })}
                >
                  {lb}
                </button>
              ))}
            </div>
            <div className="flex flex-col gap-2">
              <label>
                <span className="label">タイトル</span>
                <input
                  className="input"
                  value={paper.title}
                  placeholder="例: 二次関数と不等式の領域"
                  onChange={(e) => props.onPaperChange({ title: e.target.value })}
                />
              </label>
              <label>
                <span className="label">サブタイトル</span>
                <input
                  className="input"
                  value={paper.subtitle}
                  placeholder="例: 数学Ⅱ・図形と方程式"
                  onChange={(e) => props.onPaperChange({ subtitle: e.target.value })}
                />
              </label>
              <label>
                <span className="label">問題番号（右上）</span>
                <input
                  className="input"
                  value={paper.problemNumber}
                  placeholder="例: 問 3"
                  onChange={(e) => props.onPaperChange({ problemNumber: e.target.value })}
                />
              </label>
              <label>
                <span className="label">グラフ下の説明文</span>
                <textarea
                  className="input"
                  value={paper.caption}
                  placeholder="例: 連立不等式の表す領域を図示せよ。"
                  onChange={(e) => props.onPaperChange({ caption: e.target.value })}
                />
              </label>
              <label>
                <span className="label">余白: {paper.marginMm} mm</span>
                <input
                  type="range"
                  min={5}
                  max={40}
                  step={1}
                  className="w-full"
                  value={paper.marginMm}
                  onChange={(e) => props.onPaperChange({ marginMm: Number(e.target.value) })}
                />
              </label>
            </div>
          </>
        )}
      </div>

      {/* 出力 */}
      <div>
        <div className="section-title mb-2">出力</div>
        <div className="flex flex-col gap-1.5">
          <button className="btn btn-primary w-full" onClick={props.onPdf} disabled={props.busy}>
            <FileDown size={15} /> PDF出力（プレビュー）
          </button>
          <div className="flex gap-1.5">
            <button className="btn flex-1" onClick={props.onPng} disabled={props.busy}>
              <Image size={14} /> PNG
            </button>
            <button className="btn flex-1" onClick={props.onSvg} disabled={props.busy}>
              <FileCode size={14} /> SVG
            </button>
          </div>
        </div>
      </div>

      {/* プロジェクト */}
      <div>
        <div className="section-title mb-2">プロジェクト</div>
        <div className="flex flex-col gap-1.5">
          <button className="btn w-full" onClick={props.onSaveProject}>
            <Save size={14} /> プロジェクト保存 (.mathgraph.json)
          </button>
          <button className="btn w-full" onClick={props.onOpenProject}>
            <FolderOpen size={14} /> プロジェクトを開く
          </button>
          <button className="btn w-full" onClick={props.onNewProject}>
            <FilePlus2 size={14} /> 新規プロジェクト
          </button>
        </div>
      </div>
    </div>
  );
}
