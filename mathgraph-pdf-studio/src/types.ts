/** 関係演算子 */
export type RelOp = "=" | "<" | "<=" | ">" | ">=";

/** 線種 */
export type LineStyle = "solid" | "dashed";

/** 塗りつぶし方式（ベタ塗り・斜線・網掛け・点描） */
export type FillStyle = "solid" | "hatch" | "crosshatch" | "dot";

/** 不等式領域の描画モード（重ね塗り / 共通部分のみ） */
export type RegionMode = "overlay" | "intersection";

/** グラフ描画領域の縦横比モード */
export type AspectMode = "range" | "custom";

/** 式アイテム（1つの関数・方程式・不等式） */
export interface ExprItem {
  id: string;
  /** ユーザーが入力した式テキスト */
  input: string;
  /** 凡例名（空なら式そのものを表示） */
  name: string;
  visible: boolean;
  /** 線の色 (#rrggbb) */
  color: string;
  /** 線の太さ (px) */
  lineWidth: number;
  lineStyle: LineStyle;
  /** 不等式領域の塗りつぶし色 (#rrggbb) */
  fillColor: string;
  /** 塗りつぶし透明度 0〜1 */
  fillOpacity: number;
  /** 塗りつぶし方式 */
  fillStyle: FillStyle;
  /** 媒介変数表示のときの t 最小値 */
  tmin: number;
  /** 媒介変数表示のときの t 最大値 */
  tmax: number;
}

/** 座標平面上の点（交点・重要点などの将来拡張用） */
export interface PointItem {
  id: string;
  x: number;
  y: number;
  label: string;
  color: string;
  visible: boolean;
  /** 点から x 軸へ垂線を描く */
  showProjectionToXAxis: boolean;
  /** 点から y 軸へ垂線を描く */
  showProjectionToYAxis: boolean;
}

/**
 * グラフ上に自由配置できる数式ラベル。
 * LaTeX を MathJax でベクター組版し、任意のグラフ座標に置ける。
 */
export interface MathLabel {
  id: string;
  /** 表示する LaTeX（`$` は不要） */
  latex: string;
  /** グラフ座標（データ座標）での位置 */
  x: number;
  y: number;
  /** 文字の高さ（グラフ座標での大きさではなく、表示 pt 相当の相対サイズ） */
  fontSize: number;
  color: string;
  visible: boolean;
}

/** グラフ表示範囲と目盛り */
export interface ViewRange {
  xmin: number;
  xmax: number;
  ymin: number;
  ymax: number;
  /** x軸目盛り間隔 */
  xstep: number;
  /** y軸目盛り間隔 */
  ystep: number;
}

export type Orientation = "portrait" | "landscape";

/** グラフのみPDFの用紙比率 */
export type PdfAspectMode = "graph" | "custom";

/** 用紙・レイアウト設定 */
export interface PaperSettings {
  orientation: Orientation;
  title: string;
  subtitle: string;
  /** 問題番号（例: 問1） */
  problemNumber: string;
  /** グラフ下の説明文 */
  caption: string;
  showAxes: boolean;
  /** x軸の名前（LaTeX。数式フォントで組版される） */
  axisLabelX: string;
  /** y軸の名前 */
  axisLabelY: string;
  /** 原点の名前 */
  axisLabelO: string;
  /** 軸ラベルの文字サイズ（基準幅700pxでの相対値） */
  axisLabelSize: number;
  /** 軸の目盛り（目盛り線と数値）を表示するか */
  showTicks: boolean;
  showGrid: boolean;
  /** 凡例を表示するか */
  showLegend: boolean;
  /** 凡例の文字サイズ（基準幅700pxでの相対値） */
  legendFontSize: number;
  /** 曲線どうしの交点を自動検出して表示するか */
  showIntersections: boolean;
  /** 交点に座標ラベルを付けるか */
  showIntersectionCoords: boolean;
  /** 不等式領域の描画モード */
  regionMode: RegionMode;
  /** 共通部分の塗りつぶし色（regionMode = intersection のとき） */
  intersectionColor: string;
  /** 共通部分の塗りつぶし透明度 */
  intersectionOpacity: number;
  /** 共通部分の塗りつぶし方式 */
  intersectionStyle: FillStyle;
  /** 旧プロジェクト互換用。新規設定では aspectMode を使う */
  lockAspect: boolean;
  /** グラフ描画領域の縦横比設定 */
  aspectMode: AspectMode;
  /** aspectMode = custom のときの横÷縦 */
  customAspectRatio: number;
  /** PDF余白 (mm) */
  marginMm: number;
  /**
   * PDFをグラフ部分だけにする（タイトル・説明文などを付けず、
   * グラフの縦横比ぴったりの用紙に端から端まで描く。LaTeX 埋め込み向け）。
   */
  pdfGraphOnly: boolean;
  /** グラフのみPDFの幅 (mm)。高さは縦横比から決まる */
  pdfGraphWidthMm: number;
  /** グラフのみPDFの用紙比率。graph はグラフにぴったり合わせる */
  pdfAspectMode: PdfAspectMode;
  /** pdfAspectMode = custom のときの用紙の横÷縦 */
  pdfCustomAspectRatio: number;
}

/** プロジェクトファイル (.mathgraph.json) の中身 */
export interface Project {
  /** スキーマバージョン（将来拡張用） */
  version: 1;
  appName: "MathGraph PDF Studio";
  expressions: ExprItem[];
  points: PointItem[];
  /** グラフ上に自由配置する数式ラベル */
  labels: MathLabel[];
  range: ViewRange;
  paper: PaperSettings;
}

/** 解析済みの式の種別 */
export type ParsedKind =
  | "explicit-y" // y = f(x) / y REL f(x)
  | "explicit-x" // x = c / x REL c（縦線・半平面）
  | "explicit-x-fn" // x = f(y) / x REL f(y)（y方向にサンプリング）
  | "parametric" // (x(t), y(t))（媒介変数表示）
  | "implicit"; // F(x,y) REL 0（円・楕円など）

/** 数式解析結果（成功） */
export interface ParsedExpr {
  ok: true;
  kind: ParsedKind;
  rel: RelOp;
  /** KaTeX 表示用 LaTeX */
  latex: string;
  /** kind === "explicit-y": f(x) を評価 */
  fx?: (x: number) => number;
  /** kind === "explicit-x": 定数 c */
  xconst?: number;
  /** kind === "explicit-x-fn": f(y) を評価 */
  fy?: (y: number) => number;
  /** kind === "parametric": x(t) を評価 */
  xt?: (t: number) => number;
  /** kind === "parametric": y(t) を評価 */
  yt?: (t: number) => number;
  /** kind === "implicit": G(x,y)。G<=0 が不等式の内側（rel が等号なら G=0 が曲線） */
  gxy?: (x: number, y: number) => number;
  /**
   * `or`（または）で結合された領域の各枝の G 関数。
   * 塗りつぶし時に枝ごとに評価して重ね合わせると、場合分けの継ぎ目に
   * 描画アーティファクト（切れ込み）が出ない。
   */
  orBranches?: Array<(x: number, y: number) => number>;
  /** 不等式かどうか */
  isInequality: boolean;
  /** 等式曲線を条件で切り取るための制約。clipGxy <= 0 の部分だけ描く */
  clipGxy?: (x: number, y: number) => number;
}

/** 数式解析結果（失敗） */
export interface ParseError {
  ok: false;
  /** 日本語のエラーメッセージ */
  message: string;
}

export type ParseResult = ParsedExpr | ParseError;
