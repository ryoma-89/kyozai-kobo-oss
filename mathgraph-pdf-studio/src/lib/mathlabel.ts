// LaTeX を MathJax でベクター（SVGパス）に組版するモジュール。
// public/mathjax/tex-svg-full.js を同梱して完全オフラインで動作する。
// glyph はパスに変換されるので、プレビュー・PNG・SVG・PDF すべてで同一の
// 見た目になり、埋め込みフォント不要でベクター品質を保てる。

declare global {
  interface Window {
    MathJax?: {
      typesetPromise?: (els: HTMLElement[]) => Promise<void>;
      typesetClear?: (els: HTMLElement[]) => void;
      tex2svg?: (tex: string, opts?: { display?: boolean }) => HTMLElement;
      startup?: { promise?: Promise<void> };
      [k: string]: unknown;
    };
  }
}

let readyPromise: Promise<boolean> | null = null;
let isReady = false;

/** MathJax が使用可能か */
export function mathjaxReady(): boolean {
  return isReady;
}

/**
 * 既にロード済みのMathJaxの出力設定をパスインライン化（fontCache: none）へ寄せる。
 * ホストページ（教材工房のindex.html）が fontCache: "global" で先読みしていると、
 * tex2svg の出力glyphが document 側のグローバル<defs>への <use> 参照になり、
 * SVG文字列として切り出した時点で参照が切れてラベルが描画されなくなるため。
 */
function forceInlineFontCache(): void {
  try {
    const startup = window.MathJax?.startup as
      | { output?: { options?: { fontCache?: string } } }
      | undefined;
    const options = startup?.output?.options;
    if (options && options.fontCache !== "none") options.fontCache = "none";
  } catch {
    /* 内部構造が変わっていても致命ではない（renderMathToSvg側のuse展開で救済） */
  }
}

/**
 * MathJax（tex-svg）を一度だけ読み込む。
 * ホストページが独自にMathJaxを読み込む場合（教材工房）はそれを待って共用し、
 * 二重ロードや先読み設定（数式区切り等）の破壊をしない。
 * @returns 読み込み成功で true。失敗しても reject せず false を返す。
 */
export function initMathJax(): Promise<boolean> {
  if (readyPromise) return readyPromise;
  readyPromise = new Promise<boolean>((resolve) => {
    if (typeof window === "undefined") return resolve(false);
    const finish = () => {
      isReady = !!window.MathJax?.tex2svg;
      if (isReady) forceInlineFontCache();
      if (!isReady) readyPromise = null;
      resolve(isReady);
    };
    if (window.MathJax?.tex2svg) return finish();

    // ホストページ側でMathJaxを読み込み中（設定オブジェクトのみ、または初期化中）の場合
    const hostScript = document.querySelector<HTMLScriptElement>(
      'script[src*="mathjax"]',
    );
    if (window.MathJax || hostScript) {
      // まだ設定段階なら、出力をパスインライン化へ変更してから待つ
      const mj = window.MathJax as { svg?: { fontCache?: string }; startup?: { promise?: Promise<void> } } | undefined;
      if (mj && !mj.startup?.promise) mj.svg = { ...(mj.svg ?? {}), fontCache: "none" };
      const deadline = Date.now() + 15_000;
      const wait = () => {
        if (window.MathJax?.tex2svg) return finish();
        if (Date.now() > deadline) return finish();
        window.setTimeout(wait, 150);
      };
      return wait();
    }

    // 単体アプリ: 読み込み前に設定を注入（fontCache none で glyph をパスとしてインライン化）
    window.MathJax = {
      tex: { packages: { "[+]": ["ams", "color"] } },
      svg: { fontCache: "none" },
      startup: { typeset: false },
    } as Window["MathJax"];
    const script = document.createElement("script");
    script.src = `${import.meta.env.BASE_URL}mathjax/tex-svg-full.js`;
    script.async = true;
    script.onload = async () => {
      try {
        await window.MathJax?.startup?.promise;
      } catch {
        /* ignore */
      }
      finish();
    };
    script.onerror = () => {
      readyPromise = null;
      script.remove();
      resolve(false);
    };
    document.head.appendChild(script);
  });
  return readyPromise;
}

export interface RenderedMath {
  /** <svg> 内側の中身（<g>…</g>） */
  inner: string;
  /** viewBox = "minX minY width height"（MathJax 内部単位） */
  vbMinX: number;
  vbMinY: number;
  vbW: number;
  vbH: number;
  /** width, height（ex 単位） */
  exW: number;
  exH: number;
  error: boolean;
}

const cache = new Map<string, RenderedMath>();

const ERR: RenderedMath = {
  inner: "",
  vbMinX: 0,
  vbMinY: 0,
  vbW: 0,
  vbH: 0,
  exW: 0,
  exH: 0,
  error: true,
};

const exOf = (v: string | null): number => {
  if (!v) return 0;
  const m = /(-?[\d.]+)/.exec(v);
  return m ? parseFloat(m[1]) : 0;
};

/**
 * 入力欄へ貼り付けられやすい数式区切りを除き、MathJax/KaTeXへ同じ本文を渡す。
 * tex2svgは区切りなしのTeX本文を要求するため、$...$のままだとmerrorになり得る。
 */
export function normalizeMathLabelLatex(input: string): string {
  let value = input.trim().replace(/＼/g, "\\");
  const pairs: Array<[string, string]> = [
    ["$$", "$$"],
    ["\\[", "\\]"],
    ["\\(", "\\)"],
    ["$", "$"],
  ];
  for (const [start, end] of pairs) {
    if (value.startsWith(start) && value.endsWith(end) && value.length >= start.length + end.length) {
      value = value.slice(start.length, value.length - end.length).trim();
      break;
    }
  }
  return value;
}

/**
 * SVG内の <use> 参照をパス実体へ展開して自己完結化する。
 * fontCache: "global"（教材工房の先読み設定）ではglyph本体が document 側の
 * グローバル<defs>にしか存在せず、文字列として切り出すと参照が切れるため、
 * 参照先を複製してインライン化する。同一SVG内のdefsで解決できる参照
 * （fontCache: "local"）はそのまま残す。
 * @returns 全参照を解決できたら true。解決不能な参照が残れば false。
 */
function inlineUseReferences(svg: SVGElement): boolean {
  const uses = Array.from(svg.querySelectorAll("use"));
  for (const use of uses) {
    const href = use.getAttribute("href") ?? use.getAttribute("xlink:href") ?? "";
    if (!href.startsWith("#")) return false;
    const id = href.slice(1);
    let local = false;
    try {
      local = !!svg.querySelector(`#${CSS.escape(id)}`);
    } catch {
      local = false;
    }
    if (local) continue; // 同一SVG内のdefsで解決できる（切り出しても保たれる）
    const target = document.getElementById(id);
    if (!target) return false;
    // <use x y transform> の合成順（transform → translate(x,y)）を<g>で再現する
    const x = parseFloat(use.getAttribute("x") ?? "0") || 0;
    const y = parseFloat(use.getAttribute("y") ?? "0") || 0;
    const transforms: string[] = [];
    const tf = use.getAttribute("transform");
    if (tf) transforms.push(tf);
    if (x !== 0 || y !== 0) transforms.push(`translate(${x} ${y})`);
    const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
    if (transforms.length > 0) g.setAttribute("transform", transforms.join(" "));
    for (const attr of Array.from(use.attributes)) {
      if (
        attr.name === "href" || attr.name === "xlink:href" ||
        attr.name === "x" || attr.name === "y" ||
        attr.name === "transform" || attr.name === "width" || attr.name === "height"
      ) {
        continue;
      }
      g.setAttribute(attr.name, attr.value);
    }
    const clone = target.cloneNode(true) as Element;
    clone.removeAttribute("id");
    g.appendChild(clone);
    use.replaceWith(g);
  }
  return true;
}

/** LaTeX を組版して内部表現を返す（キャッシュ付き・例外を投げない） */
export function renderMathToSvg(latex: string): RenderedMath {
  const normalized = normalizeMathLabelLatex(latex);
  if (!normalized) return ERR;
  const hit = cache.get(normalized);
  if (hit) return hit;
  if (!isReady || !window.MathJax?.tex2svg) return ERR;

  let result: RenderedMath;
  try {
    const container = window.MathJax.tex2svg(normalized, { display: false });
    const svg = container.querySelector("svg");
    if (!svg) {
      result = ERR;
    } else {
      // glyphの<use>参照を実体パスへ展開して自己完結化する。
      // 解決できない参照が残る場合は成功扱いにせず、可視フォールバックへ回す。
      // （状況依存の失敗なのでキャッシュもしない）
      if (!inlineUseReferences(svg)) {
        return ERR;
      }
      // TeX 構文エラーは mjx-merror として表現される
      const hasErr = !!svg.querySelector('[data-mml-node="merror"]');
      const vb = (svg.getAttribute("viewBox") ?? "0 0 0 0").split(/\s+/).map(Number);
      let inner = "";
      svg.childNodes.forEach((n: ChildNode) => {
        if (n.nodeType === 1) inner += (n as Element).outerHTML;
      });
      result = {
        inner,
        vbMinX: vb[0] || 0,
        vbMinY: vb[1] || 0,
        vbW: vb[2] || 0,
        vbH: vb[3] || 0,
        exW: exOf(svg.getAttribute("width")),
        exH: exOf(svg.getAttribute("height")),
        error: hasErr,
      };
    }
  } catch {
    result = ERR;
  }
  if (cache.size > 500) cache.clear();
  cache.set(normalized, result);
  return result;
}

// 1ex ≈ 0.5em。fontSize(px) を em とみなすと表示高 px = exH * 0.5 * fontSize。
const EX_PER_EM = 0.5;

/** ラベルの表示サイズ（px）を返す（当たり判定・レイアウト用） */
export function measureMathLabel(
  latex: string,
  fontSize: number,
): { width: number; height: number; error: boolean } {
  const r = renderMathToSvg(latex);
  if (r.error || r.vbH === 0) return { width: 0, height: 0, error: r.error };
  const pxH = r.exH * EX_PER_EM * fontSize;
  const scale = pxH / r.vbH;
  return { width: r.vbW * scale, height: pxH, error: false };
}

/**
 * ラベルを SVG グループ文字列として組み立てる（左上を (cx,cy) に合わせる）。
 * @param idKey  glyph id 衝突回避用のユニーク文字列
 */
export function mathLabelSvg(
  latex: string,
  cx: number,
  cy: number,
  fontSize: number,
  color: string,
  idKey: string,
): { svg: string; width: number; height: number; error: boolean } {
  const r = renderMathToSvg(latex);
  if (r.error || r.vbH === 0) return { svg: "", width: 0, height: 0, error: r.error };

  const pxH = r.exH * EX_PER_EM * fontSize;
  const scale = pxH / r.vbH;
  const w = r.vbW * scale;

  // 複数ラベルでの id 衝突（use/defs）を避けるため接頭辞を付与し、
  // currentColor を実際の色へ置換（標準SVG文字列やPDFで色が固定されるように）
  const uid = `ml${idKey.replace(/[^A-Za-z0-9_-]/g, "")}`;
  const body = r.inner
    .replace(/id="([^"]+)"/g, `id="${uid}-$1"`)
    .replace(/xlink:href="#([^"]+)"/g, `xlink:href="#${uid}-$1"`)
    .replace(/(^|\s)href="#([^"]+)"/g, `$1href="#${uid}-$2"`)
    .replace(/currentColor/g, color);

  // MathJax の出力は内部に scale(1,-1) を含む。外側でも手動で
  // viewBox の移動・拡大を行うと、Safari/WebKit では変換順の差により
  // 数式全体が描画領域外へずれることがある。ネストした SVG の標準
  // viewBox 変換に任せれば、画面・PNG・PDF で同じ座標になる。
  const svg =
    `<svg x="${cx.toFixed(2)}" y="${cy.toFixed(2)}" ` +
    `width="${w.toFixed(2)}" height="${pxH.toFixed(2)}" ` +
    `viewBox="${r.vbMinX} ${r.vbMinY} ${r.vbW} ${r.vbH}" ` +
    `preserveAspectRatio="xMinYMin meet" overflow="visible" ` +
    `color="${color}" fill="${color}" stroke="none">${body}</svg>`;
  return { svg, width: w, height: pxH, error: false };
}
