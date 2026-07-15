import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef } from "react";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import {
  HighlightStyle,
  bracketMatching,
  defaultHighlightStyle,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import { stex } from "@codemirror/legacy-modes/mode/stex";
import { searchKeymap } from "@codemirror/search";
import { EditorSelection, EditorState, RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  placeholder as cmPlaceholder,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { StreamLanguage } from "@codemirror/language";

interface Props {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  className?: string;
}

export interface LatexEditorHandle {
  focus: () => void;
  selectionStart: number;
  selectionEnd: number;
  setSelectionRange: (start: number, end: number) => void;
  scrollTop: number;
  readonly clientHeight: number;
}

const latexHighlight = HighlightStyle.define([
  { tag: t.keyword, color: "#67e8f9" },
  { tag: t.processingInstruction, color: "#67e8f9" },
  { tag: t.variableName, color: "#c4b5fd" },
  { tag: t.atom, color: "#c5b7df" },
  { tag: t.string, color: "#e8c66a" },
  { tag: t.number, color: "#fbbf24" },
  { tag: t.comment, color: "#6d8877", fontStyle: "italic" },
  { tag: t.bracket, color: "#e8c66a" },
  { tag: t.invalid, color: "#f16a75" },
]);

const latexMarks = {
  command: Decoration.mark({ class: "cm-latex-command" }),
  environment: Decoration.mark({ class: "cm-latex-environment" }),
  mathDelimiter: Decoration.mark({ class: "cm-latex-math-delimiter" }),
  comment: Decoration.mark({ class: "cm-latex-comment" }),
  placeholder: Decoration.mark({ class: "cm-latex-placeholder" }),
  bracket: Decoration.mark({ class: "cm-latex-bracket" }),
  optionalBracket: Decoration.mark({ class: "cm-latex-optional-bracket" }),
};

interface MarkCandidate {
  from: number;
  to: number;
  priority: number;
  deco: Decoration;
}

function isEscaped(text: string, index: number): boolean {
  let count = 0;
  for (let i = index - 1; i >= 0 && text[i] === "\\"; i--) count += 1;
  return count % 2 === 1;
}

function commentStart(text: string): number {
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "%" && !isEscaped(text, i)) return i;
  }
  return -1;
}

function addCandidate(
  candidates: MarkCandidate[],
  from: number,
  to: number,
  priority: number,
  deco: Decoration,
) {
  if (to > from) candidates.push({ from, to, priority, deco });
}

function lineDecorations(text: string): MarkCandidate[] {
  const candidates: MarkCandidate[] = [];
  const comment = commentStart(text);
  const scanEnd = comment >= 0 ? comment : text.length;
  const source = text.slice(0, scanEnd);

  if (comment >= 0) {
    addCandidate(candidates, comment, text.length, 100, latexMarks.comment);
  }

  for (const match of source.matchAll(/\{\{[A-Z_]+\}\}/g)) {
    addCandidate(candidates, match.index, match.index + match[0].length, 90, latexMarks.placeholder);
  }

  for (const match of source.matchAll(/\\(begin|end)\s*\{([^}]*)\}/g)) {
    const commandEnd = match.index + 1 + match[1].length;
    const envStart = match.index + match[0].lastIndexOf("{") + 1;
    const envEnd = match.index + match[0].lastIndexOf("}");
    addCandidate(candidates, match.index, commandEnd, 80, latexMarks.command);
    addCandidate(candidates, envStart, envEnd, 85, latexMarks.environment);
  }

  for (const match of source.matchAll(/\\(?:[a-zA-Z@]+[*]?|.)/g)) {
    addCandidate(candidates, match.index, match.index + match[0].length, 70, latexMarks.command);
  }

  for (let i = 0; i < source.length; i++) {
    const pair = source.slice(i, i + 2);
    if (pair === "\\[" || pair === "\\]" || pair === "\\(" || pair === "\\)") {
      addCandidate(candidates, i, i + 2, 75, latexMarks.mathDelimiter);
      i += 1;
      continue;
    }
    if (source[i] === "$" && !isEscaped(source, i)) {
      const isDouble = source[i + 1] === "$";
      addCandidate(candidates, i, i + (isDouble ? 2 : 1), 75, latexMarks.mathDelimiter);
      if (isDouble) i += 1;
    }
  }

  for (let i = 0; i < source.length; i++) {
    const char = source[i];
    if (char === "{" || char === "}") {
      addCandidate(candidates, i, i + 1, 20, latexMarks.bracket);
    } else if (char === "[" || char === "]") {
      addCandidate(candidates, i, i + 1, 20, latexMarks.optionalBracket);
    }
  }

  const occupied: Array<[number, number]> = [];
  const accepted: MarkCandidate[] = [];
  const overlaps = (from: number, to: number) => occupied.some(([a, b]) => from < b && to > a);
  for (const candidate of candidates.sort((a, b) => b.priority - a.priority || a.from - b.from)) {
    if (overlaps(candidate.from, candidate.to)) continue;
    occupied.push([candidate.from, candidate.to]);
    accepted.push(candidate);
  }
  return accepted.sort((a, b) => a.from - b.from || a.to - b.to);
}

function buildLatexDecorations(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    let pos = from;
    while (pos <= to) {
      const line = view.state.doc.lineAt(pos);
      for (const mark of lineDecorations(line.text)) {
        builder.add(line.from + mark.from, line.from + mark.to, mark.deco);
      }
      if (line.to + 1 > to) break;
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

const latexDecorations = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildLatexDecorations(view);
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildLatexDecorations(update.view);
      }
    }
  },
  {
    decorations: (plugin) => plugin.decorations,
  },
);

const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    backgroundColor: "#111111",
    color: "#e3e3e3",
    fontSize: "13px",
  },
  "&.cm-focused": {
    outline: "none",
  },
  ".cm-scroller": {
    fontFamily: 'Consolas, "Cascadia Mono", "Courier New", monospace',
    lineHeight: "1.65",
    overflow: "auto",
  },
  ".cm-content": {
    minHeight: "100%",
    padding: "10px 0",
    caretColor: "var(--accent)",
  },
  ".cm-line": {
    padding: "0 12px",
  },
  ".cm-gutters": {
    backgroundColor: "#0d0d0d",
    color: "#777777",
    borderRight: "1px solid var(--border)",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    padding: "0 8px 0 10px",
    minWidth: "34px",
  },
  ".cm-activeLine": {
    backgroundColor: "rgba(255, 255, 255, 0.05)",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "rgba(185, 169, 214, 0.09)",
    color: "var(--accent)",
  },
  ".cm-selectionBackground": {
    backgroundColor: "rgba(185, 169, 214, 0.22) !important",
  },
  "&.cm-focused .cm-selectionBackground": {
    backgroundColor: "rgba(185, 169, 214, 0.28) !important",
  },
  ".cm-content ::selection": {
    backgroundColor: "rgba(185, 169, 214, 0.28)",
    color: "inherit",
  },
  ".cm-line::selection, .cm-line span::selection": {
    backgroundColor: "rgba(185, 169, 214, 0.28)",
    color: "inherit",
  },
  ".cm-cursor": {
    borderLeftColor: "var(--accent)",
  },
  ".cm-placeholder": {
    color: "#4a5870",
  },
  ".cm-matchingBracket": {
    backgroundColor: "rgba(197, 183, 223, 0.16)",
    outline: "1px solid rgba(197, 183, 223, 0.4)",
  },
  ".cm-nonmatchingBracket": {
    backgroundColor: "rgba(241, 106, 117, 0.16)",
    outline: "1px solid rgba(241, 106, 117, 0.4)",
  },
  ".cm-latex-command": {
    color: "#5eead4",
    fontWeight: "650",
  },
  ".cm-latex-environment": {
    color: "#9d6cf2",
    fontWeight: "700",
  },
  ".cm-latex-math-delimiter": {
    color: "#fbbf24",
    fontWeight: "700",
  },
  ".cm-latex-comment": {
    color: "#6d8877",
    fontStyle: "italic",
  },
  ".cm-latex-placeholder": {
    color: "#c5b7df",
    fontWeight: "700",
    backgroundColor: "rgba(197, 183, 223, 0.08)",
    borderRadius: "2px",
  },
  ".cm-latex-bracket": {
    color: "#e8c66a",
  },
  ".cm-latex-optional-bracket": {
    color: "#7dd3fc",
  },
});

/**
 * LaTeX入力用エディタ。
 * CodeMirror 6 を使い、長文・日本語・折り返しでもカーソル位置が安定する実エディタ上で
 * LaTeXの色分け、行番号、括弧対応、検索キー操作を提供する。
 */
export const LatexEditor = forwardRef<LatexEditorHandle, Props>(function LatexEditor(
  { value, onChange, placeholder, className }: Props,
  ref,
) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  const extensions = useMemo<Extension[]>(
    () => [
      highlightSpecialChars(),
      lineNumbers(),
      highlightActiveLineGutter(),
      history(),
      drawSelection(),
      dropCursor(),
      EditorState.allowMultipleSelections.of(true),
      indentOnInput(),
      StreamLanguage.define(stex),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      syntaxHighlighting(latexHighlight),
      latexDecorations,
      bracketMatching(),
      closeBrackets(),
      highlightActiveLine(),
      EditorView.lineWrapping,
      cmPlaceholder(placeholder ?? ""),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          onChangeRef.current(update.state.doc.toString());
        }
      }),
      EditorState.readOnly.of(false),
      editorTheme,
      keymap.of([indentWithTab, ...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap, ...searchKeymap]),
    ],
    [placeholder],
  );

  useEffect(() => {
    if (!hostRef.current) return;
    const state = EditorState.create({
      doc: value,
      extensions,
    });
    const view = new EditorView({
      state,
      parent: hostRef.current,
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (current === value) return;
    const length = value.length;
    const main = view.state.selection.main;
    const selection = EditorSelection.range(Math.min(main.anchor, length), Math.min(main.head, length));
    view.dispatch({
      changes: { from: 0, to: current.length, insert: value },
      selection,
    });
  }, [value]);

  useImperativeHandle(
    ref,
    () => ({
      focus: () => viewRef.current?.focus(),
      get selectionStart() {
        return viewRef.current?.state.selection.main.from ?? 0;
      },
      get selectionEnd() {
        return viewRef.current?.state.selection.main.to ?? 0;
      },
      setSelectionRange: (start: number, end: number) => {
        const view = viewRef.current;
        if (!view) return;
        const length = view.state.doc.length;
        const anchor = Math.min(Math.max(start, 0), length);
        const head = Math.min(Math.max(end, 0), length);
        view.dispatch({
          selection: EditorSelection.range(anchor, head),
          scrollIntoView: true,
        });
      },
      get scrollTop() {
        return viewRef.current?.scrollDOM.scrollTop ?? 0;
      },
      set scrollTop(value: number) {
        const view = viewRef.current;
        if (view) view.scrollDOM.scrollTop = value;
      },
      get clientHeight() {
        return viewRef.current?.scrollDOM.clientHeight ?? 0;
      },
    }),
    [],
  );

  return (
    <div className={`latex-editor-wrap ${className ?? ""}`}>
      <div
        ref={hostRef}
        className="h-full"
        data-placeholder={placeholder ?? ""}
        aria-label={placeholder ?? "LaTeX editor"}
      />
    </div>
  );
});
