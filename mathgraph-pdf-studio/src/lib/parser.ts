import { create, all, isSymbolNode, isFunctionNode, isParenthesisNode } from "mathjs";
import type { MathNode } from "mathjs";
import type { ParsedExpr, ParseError, ParseResult, RelOp } from "../types";

// predictable: true → sqrt(-1) などが複素数ではなく NaN になる（グラフ用途に適する）
const math = create(all, { predictable: true });

// 教材で頻出する別名・区分関数を安全な純粋関数として追加する。
math.import({
  ln: (x: number) => Math.log(x),
  logb: (x: number, base: number) => Math.log(x) / Math.log(base),
  sinc: (x: number) => x === 0 ? 1 : Math.sin(x) / x,
  heaviside: (x: number) => x < 0 ? 0 : x > 0 ? 1 : 0.5,
  sgn: (x: number) => Math.sign(x),
  where: (condition: boolean, whenTrue: number, whenFalse: number) => condition ? whenTrue : whenFalse,
  sqrt2: Math.SQRT2,
  sqrt3: Math.sqrt(3),
}, { override: false });

/** 使用を許可する関数名 */
const ALLOWED_FUNCS = new Set([
  "sqrt", "abs", "sin", "cos", "tan", "asin", "acos", "atan", "atan2",
  "sec", "csc", "cot", "asec", "acsc", "acot",
  "sinh", "cosh", "tanh", "asinh", "acosh", "atanh",
  "sech", "csch", "coth", "asech", "acsch", "acoth",
  "log", "ln", "log10", "log2", "log1p", "logb", "exp", "expm1",
  "pow", "cbrt", "nthRoot", "hypot", "mod", "gamma", "erf",
  "floor", "ceil", "fix", "round", "sign", "sgn", "min", "max", "clamp",
  "sinc", "heaviside", "where",
]);

/** 使用を許可する記号（変数・定数） */
const ALLOWED_SYMBOLS = new Set(["x", "y", "pi", "PI", "e", "E", "tau", "phi", "sqrt2", "sqrt3"]);

/** `{` の対応する `}` の位置を返す（なければ -1） */
function matchBrace(s: string, open: number): number {
  let depth = 0;
  for (let i = open; i < s.length; i++) {
    if (s[i] === "{") depth++;
    else if (s[i] === "}") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** \frac{}{} と \sqrt[]{} を再帰的に通常の式へ変換する */
function replaceFracSqrt(s: string): string {
  let out = "";
  let i = 0;
  const skipWs = (k: number) => {
    while (s[k] === " ") k++;
    return k;
  };
  while (i < s.length) {
    const isFrac =
      s.startsWith("\\frac", i) ||
      s.startsWith("\\dfrac", i) ||
      s.startsWith("\\tfrac", i);
    if (isFrac) {
      const cmdLen = s.startsWith("\\frac", i) ? 5 : 6;
      let j = skipWs(i + cmdLen);
      if (s[j] === "{") {
        const e1 = matchBrace(s, j);
        if (e1 > 0) {
          let k = skipWs(e1 + 1);
          if (s[k] === "{") {
            const e2 = matchBrace(s, k);
            if (e2 > 0) {
              const num = replaceFracSqrt(s.slice(j + 1, e1));
              const den = replaceFracSqrt(s.slice(k + 1, e2));
              out += `((${num})/(${den}))`;
              i = e2 + 1;
              continue;
            }
          }
        }
      }
      out += s[i];
      i++;
    } else if (s.startsWith("\\sqrt", i)) {
      let j = skipWs(i + 5);
      let root: string | null = null;
      if (s[j] === "[") {
        const e = s.indexOf("]", j);
        if (e >= 0) {
          root = s.slice(j + 1, e);
          j = skipWs(e + 1);
        }
      }
      if (s[j] === "{") {
        const e = matchBrace(s, j);
        if (e > 0) {
          const arg = replaceFracSqrt(s.slice(j + 1, e));
          out +=
            root != null
              ? `nthRoot((${arg}),(${replaceFracSqrt(root)}))`
              : `sqrt(${arg})`;
          i = e + 1;
          continue;
        }
      }
      out += s[i];
      i++;
    } else {
      out += s[i];
      i++;
    }
  }
  return out;
}

/** LaTeX 記法を mathjs が解釈できる式へ変換する（対応する範囲で） */
export function latexToExpr(src: string): string {
  let s = src;
  // 装飾・スペース系コマンドを除去
  s = s.replace(/\\left|\\right|\\!|\\,|\\;|\\:|\\quad|\\qquad|\\displaystyle/g, " ");
  // \frac, \sqrt を先に処理（波括弧を消費する）
  s = replaceFracSqrt(s);
  // 絶対値 |...| → abs(...)
  s = s.replace(/\\lvert|\\rvert|\\vert|\\mid/g, "|");
  s = s.replace(/\|([^|]+)\|/g, "abs($1)");
  // 残った波括弧（主に指数 x^{2}）は丸括弧へ
  s = s.replace(/\{/g, "(").replace(/\}/g, ")");
  // コマンドを置換（長いキーを先に）
  const repl: Array<[string, string]> = [
    ["\\leq", "<="], ["\\geq", ">="], ["\\le", "<="], ["\\ge", ">="],
    ["\\lt", "<"], ["\\gt", ">"], ["\\neq", "="], ["\\ne", "="],
    ["\\cdot", "*"], ["\\times", "*"], ["\\div", "/"],
    ["\\land", " and "], ["\\wedge", " and "], ["\\lor", " or "], ["\\vee", " or "],
    ["\\arcsin", "asin"], ["\\arccos", "acos"], ["\\arctan", "atan"],
    ["\\sinh", "sinh"], ["\\cosh", "cosh"], ["\\tanh", "tanh"],
    ["\\sin", "sin"], ["\\cos", "cos"], ["\\tan", "tan"],
    ["\\ln", "ln"], ["\\log", "log"], ["\\exp", "exp"], ["\\sqrt", "sqrt"],
    ["\\pi", " pi "], ["\\cdot", "*"],
  ];
  for (const [k, v] of repl) s = s.split(k).join(v);
  // 未対応コマンドはバックスラッシュを外す（後段の検証でエラーにする）
  s = s.replace(/\\([a-zA-Z]+)/g, "$1");
  s = s.replace(/\\/g, "");
  return s;
}

/** √ の直後にくる「かたまり」を読み取る（数・変数・関数呼び出し・括弧・入れ子の√） */
function readRadicalFactor(s: string, i: number): { expr: string; next: number } {
  while (s[i] === " ") i++;
  if (i >= s.length) return { expr: "", next: i };
  const c = s[i];
  if (c === "√") {
    const inner = readRadicalFactor(s, i + 1);
    return { expr: `sqrt(${inner.expr})`, next: inner.next };
  }
  if (c === "(") {
    const e = matchParen(s, i);
    if (e < 0) return { expr: s.slice(i), next: s.length };
    return { expr: s.slice(i, e + 1), next: e + 1 };
  }
  if (/[A-Za-z_]/.test(c)) {
    let j = i;
    while (j < s.length && /[A-Za-z0-9_]/.test(s[j])) j++;
    if (s[j] === "(") {
      const e = matchParen(s, j);
      if (e >= 0) return { expr: s.slice(i, e + 1), next: e + 1 };
    }
    return { expr: s.slice(i, j), next: j };
  }
  if (/[0-9.]/.test(c)) {
    let j = i;
    while (j < s.length && /[0-9.]/.test(s[j])) j++;
    return { expr: s.slice(i, j), next: j };
  }
  return { expr: "", next: i };
}

/** 根号 √<かたまり> を sqrt(<かたまり>) に変換する（√x, √√x, √(x+1), √sin(x) など） */
function replaceRadical(s: string): string {
  let out = "";
  let i = 0;
  while (i < s.length) {
    if (s[i] === "√") {
      const f = readRadicalFactor(s, i + 1);
      if (f.expr) {
        out += `sqrt(${f.expr})`;
        i = f.next;
        continue;
      }
      i++; // 直後にかたまりが無い √ は捨てる
      continue;
    }
    out += s[i];
    i++;
  }
  return out;
}

/** 全角文字・数学記号を半角の式に正規化する */
function replaceSpacedLogicalCarets(value: string): string {
  let depth = 0;
  let output = "";
  const hasRelation = (part: string) => /(?:<=|>=|==|=|<|>)/.test(part);
  for (let index = 0; index < value.length; index++) {
    const character = value[index];
    if (character === "(") depth += 1;
    else if (character === ")") depth = Math.max(0, depth - 1);
    if (character === "^" && depth === 0 && /\s/.test(value[index - 1] ?? "") && /\s/.test(value[index + 1] ?? "")
      && hasRelation(value.slice(0, index)) && hasRelation(value.slice(index + 1))) {
      output += " and ";
    } else {
      output += character;
    }
  }
  return output;
}

export function normalizeInput(raw: string): string {
  const map: Record<string, string> = {
    "０": "0", "１": "1", "２": "2", "３": "3", "４": "4",
    "５": "5", "６": "6", "７": "7", "８": "8", "９": "9",
    "ｘ": "x", "ｙ": "y", "Ｘ": "x", "Ｙ": "y",
    "＋": "+", "－": "-", "−": "-", "ー": "-",
    "×": "*", "＊": "*", "・": "*",
    "÷": "/", "／": "/",
    "＾": "^", "（": "(", "）": ")", "，": ",", "．": ".",
    "＝": "=", "＜": "<", "＞": ">",
    "≦": "<=", "≧": ">=", "≤": "<=", "≥": ">=", "≠": "=",
    "　": " ", "π": "pi",
    // ガウス記号・天井関数
    "⌊": "floor(", "⌋": ")", "⌈": "ceil(", "⌉": ")",
  };
  let s = "";
  for (const ch of raw) s += map[ch] ?? ch;
  // 2D教材で使われてきた「不等式 ^ 不等式」は論理積として扱う。
  // 空白のない x^2 などの累乗記号はそのまま保持する。
  s = replaceSpacedLogicalCarets(s);
  // LaTeX 記法（\frac, \sqrt, ^{}, \le, |...| など）を通常の式へ変換
  if (/[\\{}|]/.test(s)) s = latexToExpr(s);
  // ガウス記号 [x] → floor(x)（入れ子や [x+1] も対応。乗算の取りこぼしを防ぐため * を補う）
  if (s.includes("[")) {
    s = s.replace(/([0-9a-zA-Z_.)])\s*\[/g, "$1*[");
    s = s.replace(/\[/g, "floor(").replace(/\]/g, ")");
  }
  // 根号 √ を sqrt(...) へ。括弧なし √x や入れ子 √√x にも対応
  if (s.includes("√")) s = replaceRadical(s);
  // 「=<」「=>」の並びも「<=」「>=」として解釈する
  s = s.replace(/=</g, "<=").replace(/=>/g, ">=");
  // 論理結合の表記ゆれを and / or に統一
  s = s
    .replace(/かつ/g, " and ")
    .replace(/または/g, " or ")
    .replace(/&&/g, " and ")
    .replace(/\|\|/g, " or ")
    .replace(/∧/g, " and ")
    .replace(/∨/g, " or ");
  return s.trim();
}

/** 関数引数内の比較（where(x<0,...)等）を除き、式の最上位にある関係演算子だけを返す。 */
function topLevelRelations(input: string): RegExpMatchArray[] {
  const matches: RegExpMatchArray[] = [];
  let depth = 0;
  for (let index = 0; index < input.length; index++) {
    const character = input[index];
    if (character === "(") { depth += 1; continue; }
    if (character === ")") { depth = Math.max(0, depth - 1); continue; }
    if (depth !== 0) continue;
    const operator = ["<=", ">=", "==", "=", "<", ">"].find((value) => input.startsWith(value, index));
    if (!operator) continue;
    const match = [operator] as unknown as RegExpMatchArray;
    match.index = index; match.input = input;
    matches.push(match);
    index += operator.length - 1;
  }
  return matches;
}

/** 括弧ノードを剥がす */
function unwrap(node: MathNode): MathNode {
  let n = node;
  while (isParenthesisNode(n)) n = n.content;
  return n;
}

/** 式に含まれる変数と関数を検査し、問題があれば日本語エラーを返す */
function validateNode(node: MathNode): { vars: Set<string>; error?: string } {
  const vars = new Set<string>();
  let error: string | undefined;
  node.traverse((n: MathNode, _path: string, parent: MathNode | null) => {
    if (error) return;
    if (isFunctionNode(n)) {
      const fname = n.fn.name;
      if (!ALLOWED_FUNCS.has(fname)) {
        error = `未対応の関数「${fname}()」が含まれています。使用できる関数: sqrt, abs, sin, cos, tan, log, ln, exp など`;
      }
    } else if (isSymbolNode(n)) {
      // 関数呼び出しの関数名部分は変数として扱わない
      if (parent && isFunctionNode(parent) && parent.fn === n) return;
      if (!ALLOWED_SYMBOLS.has(n.name)) {
        error = `未対応の変数「${n.name}」が含まれています。使用できる変数は x と y です。`;
      } else if (n.name === "x" || n.name === "y") {
        vars.add(n.name);
      }
    }
  });
  return { vars, error };
}

export type BivariateCompileResult =
  | { ok: true; normalized: string; evaluate: (x: number, y: number) => number }
  | { ok: false; message: string };

const ALLOWED_BIVARIATE_NODE_TYPES = new Set(["OperatorNode", "ConstantNode", "SymbolNode", "FunctionNode", "ParenthesisNode"]);

/** x・yだけを変数に持つ実数式を、安全な有限値関数としてコンパイルする。 */
export function compileBivariateExpression(raw: string): BivariateCompileResult {
  if ([...raw].length > 500) return { ok: false, message: "数式は500文字までです" };
  const normalized = normalizeInput(raw);
  if (!normalized) return { ok: false, message: "数式を入力してください" };
  if (topLevelRelations(normalized).length) return { ok: false, message: "最上位に等号や不等号を含まない f(x,y) を入力してください（区分関数内の比較は使用できます）" };
  let node: MathNode;
  try { node = math.parse(normalized); } catch { return { ok: false, message: "数式を解釈できません。`*` や括弧の対応を確認してください。" }; }
  let unsupported = "";
  node.traverse((value: MathNode) => {
    if (!unsupported && !ALLOWED_BIVARIATE_NODE_TYPES.has(value.type)) unsupported = `未対応の数式要素「${value.type}」が含まれています`;
  });
  if (unsupported) return { ok: false, message: unsupported };
  const validation = validateNode(node);
  if (validation.error) return { ok: false, message: validation.error };
  const compiled = node.compile();
  const scope: Record<string, number> = { x: 0, y: 0 };
  return {
    ok: true,
    normalized,
    evaluate: (x: number, y: number) => {
      scope.x = x; scope.y = y;
      try {
        const value = compiled.evaluate(scope);
        return typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 1_000_000_000 ? value : NaN;
      } catch { return NaN; }
    },
  };
}

const REL_TEX: Record<RelOp, string> = {
  "=": "=",
  "<": "<",
  "<=": "\\le",
  ">": ">",
  ">=": "\\ge",
};

function toTexSafe(node: MathNode): string {
  try {
    return node.toTex({ parenthesis: "auto" });
  } catch {
    return "";
  }
}

const PARSE_ERROR_MSG =
  "数式を解釈できません。`*` や括弧の対応を確認してください。";

/** 解析済みの式（不等式）を「G(x,y) <= 0 が内側」の関数に変換する */
export function parsedToG(p: ParsedExpr): (x: number, y: number) => number {
  if (p.kind === "explicit-y" && p.fx) {
    const fx = p.fx;
    return p.rel === ">" || p.rel === ">="
      ? (x, y) => fx(x) - y
      : (x, y) => y - fx(x);
  }
  if (p.kind === "explicit-x" && p.xconst !== undefined) {
    const c = p.xconst;
    return p.rel === ">" || p.rel === ">=" ? (x) => c - x : (x) => x - c;
  }
  return p.gxy!;
}

// ---------------------------------------------------------------------------
// and / or による論理結合（かつ・または）
// ---------------------------------------------------------------------------

type GFn = (x: number, y: number) => number;
type BoolResult =
  | { ok: true; g: GFn; latex: string; multi: boolean; branches?: GFn[] }
  | ParseError;

/** 括弧の外側（深さ0）にある and / or 単語で分割する */
function splitTopLevel(s: string, word: "and" | "or"): string[] {
  const isWordChar = (c: string) => /[A-Za-z0-9_]/.test(c);
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch === "(") depth++;
    else if (ch === ")") depth--;
    else if (depth === 0 && s.startsWith(word, i)) {
      const before = i === 0 ? "" : s[i - 1];
      const after = s[i + word.length] ?? "";
      if (!isWordChar(before) && !isWordChar(after)) {
        parts.push(s.slice(start, i));
        start = i + word.length;
        i = start - 1;
      }
    }
  }
  parts.push(s.slice(start));
  return parts;
}

/** 文字列全体が対応の取れた括弧で包まれているか */
function fullyWrapped(s: string): boolean {
  if (s.length < 2 || s[0] !== "(" || s[s.length - 1] !== ")") return false;
  let depth = 0;
  for (let i = 0; i < s.length; i++) {
    if (s[i] === "(") depth++;
    else if (s[i] === ")") {
      depth--;
      if (depth === 0) return i === s.length - 1;
    }
  }
  return false;
}

function parseOrExpr(s: string): BoolResult {
  const parts = splitTopLevel(s, "or");
  if (parts.length === 1) return parseAndExpr(s);
  const children: Array<{ g: GFn; latex: string; multi: boolean }> = [];
  for (const p of parts) {
    const r = parseAndExpr(p);
    if (!r.ok) return r;
    children.push(r);
  }
  const gs = children.map((c) => c.g);
  // または: どれか1つでも満たせば内側（NaN の子は無視する）
  const g: GFn = (x, y) => {
    let m = Infinity;
    let any = false;
    for (const f of gs) {
      const v = f(x, y);
      if (Number.isNaN(v)) continue;
      any = true;
      if (v < m) m = v;
    }
    return any ? m : NaN;
  };
  const latex = children
    .map((c) => (c.multi ? `\\left(${c.latex}\\right)` : c.latex))
    .join(" \\;\\lor\\; ");
  return { ok: true, g, latex, multi: true, branches: gs };
}

function parseAndExpr(s: string): BoolResult {
  const parts = splitTopLevel(s, "and");
  if (parts.length === 1) return parseBoolAtom(s);
  const children: Array<{ g: GFn; latex: string; multi: boolean }> = [];
  for (const p of parts) {
    const r = parseBoolAtom(p);
    if (!r.ok) return r;
    children.push(r);
  }
  const gs = children.map((c) => c.g);
  // かつ: すべて満たせば内側（NaN はその点で定義されない → 外側）
  const g: GFn = (x, y) => {
    let m = -Infinity;
    for (const f of gs) {
      const v = f(x, y);
      if (Number.isNaN(v)) return NaN;
      if (v > m) m = v;
    }
    return m;
  };
  const latex = children.map((c) => c.latex).join(" \\;\\land\\; ");
  return { ok: true, g, latex, multi: true };
}

function parseBoolAtom(s: string): BoolResult {
  const st = s.trim();
  if (!st) {
    return {
      ok: false,
      message: "and / or の前後に不等式を書いてください（例: y >= 0 and y <= x^2）。",
    };
  }
  // 全体が括弧で包まれていれば中身を再帰的に解釈
  if (fullyWrapped(st)) {
    const r = parseOrExpr(st.slice(1, -1));
    if (!r.ok) return r;
    return { ok: true, g: r.g, latex: `\\left(${r.latex}\\right)`, multi: false };
  }
  const p = parseExpression(st);
  if (!p.ok) return p;
  if (!p.isInequality) {
    return {
      ok: false,
      message: "and / or で結合できるのは不等式だけです（= の式は結合できません）。",
    };
  }
  return { ok: true, g: parsedToG(p), latex: p.latex, multi: false };
}

function tryParseCurveClipExpr(input: string): ParseResult | null {
  if (fullyWrapped(input)) return tryParseCurveClipExpr(input.slice(1, -1));
  if (splitTopLevel(input, "or").length > 1) return null;
  const parts = splitTopLevel(input, "and");
  if (parts.length < 2) return null;

  let curve: ParsedExpr | null = null;
  const constraints: GFn[] = [];
  const latexParts: string[] = [];

  for (const part of parts) {
    const st = part.trim();
    if (!st) {
      return {
        ok: false,
        message: "and の前後に式を書いてください（例: y=x^2 and x>=0）。",
      };
    }
    const parsed = parseExpression(st);
    if (!parsed.ok) return parsed;
    latexParts.push(parsed.latex);
    if (parsed.isInequality) {
      constraints.push(parsedToG(parsed));
      continue;
    }
    if (curve) {
      return {
        ok: false,
        message: "and で切り取れる曲線は1つまでです。不等式条件を追加してください。",
      };
    }
    curve = parsed;
  }

  if (!curve || constraints.length === 0) return null;
  const clipGxy: GFn = (x, y) => {
    let m = -Infinity;
    for (const g of constraints) {
      const v = g(x, y);
      if (Number.isNaN(v)) return NaN;
      if (v > m) m = v;
    }
    return m;
  };
  return {
    ...curve,
    latex: latexParts.join(" \\;\\land\\; "),
    clipGxy,
  };
}

/**
 * 連鎖不等式 `A <= B <= C`（例: 1 <= x <= 3, 0 <= y <= x^3）を解析する。
 * 2つの制約の共通部分（A REL B かつ B REL C）を表す陰関数領域に変換する。
 */
function parseChained(
  input: string,
  matches: RegExpMatchArray[],
): ParseResult {
  const [m1, m2] = matches;
  const op1 = (m1[0] === "==" ? "=" : m1[0]) as RelOp;
  const op2 = (m2[0] === "==" ? "=" : m2[0]) as RelOp;
  const isLt = (o: RelOp) => o === "<" || o === "<=";
  const isGt = (o: RelOp) => o === ">" || o === ">=";
  if (op1 === "=" || op2 === "=") {
    return {
      ok: false,
      message:
        "範囲指定は不等号を2つ並べた形で入力してください（例: 1 <= x <= 3）。",
    };
  }
  if (!((isLt(op1) && isLt(op2)) || (isGt(op1) && isGt(op2)))) {
    return {
      ok: false,
      message:
        "不等号の向きを揃えてください（例: 1 <= x <= 3 または 3 >= x >= 1）。",
    };
  }

  const s1 = input.slice(0, m1.index).trim();
  const s2 = input.slice(m1.index! + m1[0].length, m2.index).trim();
  const s3 = input.slice(m2.index! + m2[0].length).trim();
  if (!s1 || !s2 || !s3) {
    return { ok: false, message: PARSE_ERROR_MSG };
  }

  let n1: MathNode;
  let n2: MathNode;
  let n3: MathNode;
  try {
    n1 = math.parse(s1);
    n2 = math.parse(s2);
    n3 = math.parse(s3);
  } catch {
    return { ok: false, message: PARSE_ERROR_MSG };
  }
  const vars = new Set<string>();
  for (const n of [n1, n2, n3]) {
    const v = validateNode(n);
    if (v.error) return { ok: false, message: v.error };
    for (const name of v.vars) vars.add(name);
  }
  if (vars.size === 0) {
    return { ok: false, message: "この式には変数 x または y が必要です。" };
  }

  const latex = `${toTexSafe(n1)} ${REL_TEX[op1]} ${toTexSafe(n2)} ${REL_TEX[op2]} ${toTexSafe(n3)}`;

  // 各制約を「G <= 0 が内側」に正規化して max で合成（共通部分）
  const gAStr = isLt(op1) ? `(${s1}) - (${s2})` : `(${s2}) - (${s1})`;
  const gBStr = isLt(op2) ? `(${s2}) - (${s3})` : `(${s3}) - (${s2})`;
  let cA: ReturnType<MathNode["compile"]>;
  let cB: ReturnType<MathNode["compile"]>;
  try {
    cA = math.parse(gAStr).compile();
    cB = math.parse(gBStr).compile();
  } catch {
    return { ok: false, message: PARSE_ERROR_MSG };
  }
  const scope: Record<string, number> = { x: 0, y: 0 };
  const gxy = (x: number, y: number): number => {
    scope.x = x;
    scope.y = y;
    try {
      const a = cA.evaluate(scope);
      const b = cB.evaluate(scope);
      if (
        typeof a !== "number" || !Number.isFinite(a) ||
        typeof b !== "number" || !Number.isFinite(b)
      ) {
        return NaN;
      }
      return Math.max(a, b);
    } catch {
      return NaN;
    }
  };
  return { ok: true, kind: "implicit", rel: op2, latex, gxy, isInequality: true };
}

/** `(` の対応する `)` の位置を返す（なければ -1） */
function matchParen(s: string, open: number): number {
  let depth = 0;
  for (let i = open; i < s.length; i++) {
    if (s[i] === "(") depth++;
    else if (s[i] === ")") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** 媒介変数（t）用の変数検査。t・定数のみ許可する */
function validateParamNode(node: MathNode): { hasT: boolean; error?: string } {
  let hasT = false;
  let error: string | undefined;
  node.traverse((n: MathNode, _path: string, parent: MathNode | null) => {
    if (error) return;
    if (isFunctionNode(n)) {
      if (!ALLOWED_FUNCS.has(n.fn.name)) {
        error = `未対応の関数「${n.fn.name}()」が含まれています。`;
      }
    } else if (isSymbolNode(n)) {
      if (parent && isFunctionNode(parent) && parent.fn === n) return;
      if (n.name === "t") hasT = true;
      else if (n.name === "x" || n.name === "y") {
        error = "媒介変数表示では変数 t を使ってください（x, y は使えません）。";
      } else if (!ALLOWED_SYMBOLS.has(n.name)) {
        error = `未対応の変数「${n.name}」が含まれています。媒介変数は t です。`;
      }
    }
  });
  return { hasT, error };
}

/** `(x(t), y(t))` 形式を媒介変数表示として解析する（対象外なら null） */
function tryParseParametric(input: string): ParseResult | null {
  if (input[0] !== "(") return null;
  if (matchParen(input, 0) !== input.length - 1) return null;
  const inner = input.slice(1, -1);
  // 深さ0のカンマで2分割
  let depth = 0;
  let comma = -1;
  for (let i = 0; i < inner.length; i++) {
    const c = inner[i];
    if (c === "(") depth++;
    else if (c === ")") depth--;
    else if (c === "," && depth === 0) {
      if (comma >= 0)
        return { ok: false, message: "媒介変数表示は (x(t), y(t)) の形で入力してください。" };
      comma = i;
    }
  }
  if (comma < 0) return null; // カンマなし → 媒介変数ではない
  const sx = inner.slice(0, comma).trim();
  const sy = inner.slice(comma + 1).trim();
  if (!sx || !sy) return { ok: false, message: PARSE_ERROR_MSG };

  let nx: MathNode;
  let ny: MathNode;
  try {
    nx = math.parse(sx);
    ny = math.parse(sy);
  } catch {
    return { ok: false, message: PARSE_ERROR_MSG };
  }
  const vx = validateParamNode(nx);
  if (vx.error) return { ok: false, message: vx.error };
  const vy = validateParamNode(ny);
  if (vy.error) return { ok: false, message: vy.error };
  if (!vx.hasT && !vy.hasT) {
    return {
      ok: false,
      message: "媒介変数 t を含めてください（例: (cos(t), sin(t)) ）。点の配置は「点を追加」をご利用ください。",
    };
  }

  const cx = nx.compile();
  const cy = ny.compile();
  const scopeX: Record<string, number> = { t: 0 };
  const scopeY: Record<string, number> = { t: 0 };
  const evalAt = (
    c: ReturnType<MathNode["compile"]>,
    scope: Record<string, number>,
    t: number,
  ): number => {
    scope.t = t;
    try {
      const v = c.evaluate(scope);
      return typeof v === "number" && Number.isFinite(v) ? v : NaN;
    } catch {
      return NaN;
    }
  };
  const xt = (t: number) => evalAt(cx, scopeX, t);
  const yt = (t: number) => evalAt(cy, scopeY, t);
  const latex = `\\left(${toTexSafe(nx)},\\; ${toTexSafe(ny)}\\right)`;
  return { ok: true, kind: "parametric", rel: "=", latex, xt, yt, isInequality: false };
}

/**
 * 入力文字列を解析して描画可能な形に変換する。
 * 対応形式:
 *   - y = f(x) / y >= f(x) など（明示関数）
 *   - x = c / x < c など（縦線・半平面）
 *   - x = f(y)（y方向の関数）
 *   - (x(t), y(t))（媒介変数表示）
 *   - F(x,y) REL G(x,y)（円・楕円などの陰関数型）
 *   - 関係演算子なしの f(x)（y = f(x) とみなす）
 */
export function parseExpression(raw: string): ParseResult {
  const input = normalizeInput(raw);
  if (input === "") {
    return { ok: false, message: "式を入力してください。" };
  }

  // 媒介変数表示 (x(t), y(t))
  if (input[0] === "(" && input.includes(",")) {
    const par = tryParseParametric(input);
    if (par) return par;
  }

  // and / or（かつ・または）による論理結合 → 陰関数領域として合成
  if (/\b(and|or)\b/.test(input)) {
    const clippedCurve = tryParseCurveClipExpr(input);
    if (clippedCurve) return clippedCurve;
    const r = parseOrExpr(input);
    if (!r.ok) return r;
    return {
      ok: true,
      kind: "implicit",
      rel: "<=",
      latex: r.latex,
      gxy: r.g,
      orBranches: r.branches,
      isInequality: true,
    };
  }

  // 関係演算子を探す（<= >= == を先に判定）
  const matches = topLevelRelations(input);
  if (matches.length > 2) {
    return {
      ok: false,
      message:
        "関係演算子が多すぎます。範囲指定は `1 <= x <= 3` のように2つまでです。",
    };
  }
  // 連鎖不等式 A <= B <= C（範囲指定）
  if (matches.length === 2) {
    return parseChained(input, matches);
  }

  let lhsStr: string;
  let rhsStr: string;
  let rel: RelOp;

  if (matches.length === 0) {
    // 演算子なし → y = (式) とみなす
    lhsStr = "y";
    rhsStr = input;
    rel = "=";
  } else {
    const m = matches[0];
    const op = m[0] === "==" ? "=" : (m[0] as RelOp);
    lhsStr = input.slice(0, m.index!).trim();
    rhsStr = input.slice(m.index! + m[0].length).trim();
    rel = op;
    if (lhsStr === "" || rhsStr === "") {
      return { ok: false, message: PARSE_ERROR_MSG };
    }
  }

  let lhsNode: MathNode;
  let rhsNode: MathNode;
  try {
    lhsNode = math.parse(lhsStr);
    rhsNode = math.parse(rhsStr);
  } catch {
    return { ok: false, message: PARSE_ERROR_MSG };
  }

  const lv = validateNode(lhsNode);
  if (lv.error) return { ok: false, message: lv.error };
  const rv = validateNode(rhsNode);
  if (rv.error) return { ok: false, message: rv.error };

  const allVars = new Set([...lv.vars, ...rv.vars]);
  if (allVars.size === 0) {
    return { ok: false, message: "この式には変数 x または y が必要です。" };
  }

  // 演算子なし入力に y が含まれる場合は形式が曖昧なのでエラー
  // （lhs は補った "y" なので、ユーザー入力である rhs 側だけを見る）
  if (matches.length === 0 && rv.vars.has("y")) {
    return {
      ok: false,
      message: "y を含む式は `y = ...` または不等式の形で入力してください。",
    };
  }

  const latex = `${toTexSafe(lhsNode)} ${REL_TEX[rel]} ${toTexSafe(rhsNode)}`;
  const isInequality = rel !== "=";

  const lhsCore = unwrap(lhsNode);
  const rhsCore = unwrap(rhsNode);
  const lhsIsY = isSymbolNode(lhsCore) && lhsCore.name === "y";
  const rhsIsY = isSymbolNode(rhsCore) && rhsCore.name === "y";
  const lhsIsX = isSymbolNode(lhsCore) && lhsCore.name === "x";
  const rhsIsX = isSymbolNode(rhsCore) && rhsCore.name === "x";

  /** 関係を左右反転する（a < y → y > a） */
  const flip = (r: RelOp): RelOp =>
    r === "<" ? ">" : r === "<=" ? ">=" : r === ">" ? "<" : r === ">=" ? "<=" : "=";

  // --- 明示関数 y REL f(x) ---
  let fNode: MathNode | null = null;
  let effRel: RelOp = rel;
  if (lhsIsY && !rv.vars.has("y")) {
    fNode = rhsNode;
    effRel = rel;
  } else if (rhsIsY && !lv.vars.has("y")) {
    fNode = lhsNode;
    effRel = flip(rel);
  }
  if (fNode) {
    const compiled = fNode.compile();
    const scope: Record<string, number> = { x: 0 };
    const fx = (x: number): number => {
      scope.x = x;
      try {
        const v = compiled.evaluate(scope);
        return typeof v === "number" && Number.isFinite(v) ? v : NaN;
      } catch {
        return NaN;
      }
    };
    return { ok: true, kind: "explicit-y", rel: effRel, latex, fx, isInequality };
  }

  // --- 縦線・半平面 x REL c ---
  let cNode: MathNode | null = null;
  if (lhsIsX && rv.vars.size === 0) {
    cNode = rhsNode;
    effRel = rel;
  } else if (rhsIsX && lv.vars.size === 0) {
    cNode = lhsNode;
    effRel = flip(rel);
  }
  if (cNode) {
    let c: number;
    try {
      const v = cNode.compile().evaluate({});
      c = typeof v === "number" && Number.isFinite(v) ? v : NaN;
    } catch {
      c = NaN;
    }
    if (Number.isNaN(c)) {
      return { ok: false, message: PARSE_ERROR_MSG };
    }
    return { ok: true, kind: "explicit-x", rel: effRel, latex, xconst: c, isInequality };
  }

  // --- 明示関数 x REL f(y)（yを含むがxを含まない右辺） ---
  let gyNode: MathNode | null = null;
  if (lhsIsX && !rv.vars.has("x") && rv.vars.has("y")) {
    gyNode = rhsNode;
    effRel = rel;
  } else if (rhsIsX && !lv.vars.has("x") && lv.vars.has("y")) {
    gyNode = lhsNode;
    effRel = flip(rel);
  }
  if (gyNode) {
    const compiled = gyNode.compile();
    const scope: Record<string, number> = { y: 0 };
    const fy = (y: number): number => {
      scope.y = y;
      try {
        const v = compiled.evaluate(scope);
        return typeof v === "number" && Number.isFinite(v) ? v : NaN;
      } catch {
        return NaN;
      }
    };
    // 交点・共通部分用の gxy（< / <= は x - f(y) <= 0 が内側、> / >= は f(y) - x <= 0）
    const rightSide = effRel === ">" || effRel === ">=";
    const gxy = (x: number, y: number): number => {
      const f = fy(y);
      if (Number.isNaN(f)) return NaN;
      return rightSide ? f - x : x - f;
    };
    return { ok: true, kind: "explicit-x-fn", rel: effRel, latex, fy, gxy, isInequality };
  }

  // --- 陰関数型 F(x,y) REL G(x,y) → G(x,y) <= 0 の内側判定に正規化 ---
  // rel が < / <= / = : G = lhs - rhs、rel が > / >= : G = rhs - lhs
  const gStr =
    rel === ">" || rel === ">="
      ? `(${rhsStr}) - (${lhsStr})`
      : `(${lhsStr}) - (${rhsStr})`;
  let gCompiled: ReturnType<MathNode["compile"]>;
  try {
    gCompiled = math.parse(gStr).compile();
  } catch {
    return { ok: false, message: PARSE_ERROR_MSG };
  }
  const gScope: Record<string, number> = { x: 0, y: 0 };
  const gxy = (x: number, y: number): number => {
    gScope.x = x;
    gScope.y = y;
    try {
      const v = gCompiled.evaluate(gScope);
      return typeof v === "number" && Number.isFinite(v) ? v : NaN;
    } catch {
      return NaN;
    }
  };
  return { ok: true, kind: "implicit", rel, latex, gxy, isInequality };
}
