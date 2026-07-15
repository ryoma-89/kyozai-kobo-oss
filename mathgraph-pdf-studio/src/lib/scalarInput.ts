import { all, create, isFunctionNode, isSymbolNode } from "mathjs";
import { normalizeInput } from "./parser";

const math = create(all, { predictable: true });
math.import({ ln: (x: number) => Math.log(x) }, { override: false });

const ALLOWED_FUNCS = new Set([
  "sqrt",
  "abs",
  "sin",
  "cos",
  "tan",
  "asin",
  "acos",
  "atan",
  "log",
  "ln",
  "log10",
  "log2",
  "exp",
  "pow",
  "cbrt",
  "nthRoot",
  "floor",
  "ceil",
  "round",
  "sign",
  "min",
  "max",
  "sinh",
  "cosh",
  "tanh",
]);

const ALLOWED_SYMBOLS = new Set(["pi", "PI", "e", "E", "tau"]);

export function formatScalar(value: number): string {
  if (!Number.isFinite(value)) return "0";
  return String(parseFloat(value.toFixed(10)));
}

export function evalScalarInput(raw: string): number | null {
  try {
    const src = normalizeInput(raw.trim());
    if (!src) return null;
    const node = math.parse(src);
    let valid = true;
    node.traverse((n) => {
      if (isSymbolNode(n) && !ALLOWED_SYMBOLS.has(n.name)) valid = false;
      if (isFunctionNode(n)) {
        const fn = n.fn;
        const name = isSymbolNode(fn) ? fn.name : "";
        if (!ALLOWED_FUNCS.has(name)) valid = false;
      }
    });
    if (!valid) return null;
    const value = node.compile().evaluate({});
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  } catch {
    return null;
  }
}
