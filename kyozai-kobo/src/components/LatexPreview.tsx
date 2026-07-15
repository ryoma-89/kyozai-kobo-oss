import { useEffect, useRef } from "react";
import { latexToPreviewHtml, typeset } from "../mathjax";

/** LaTeXソースの簡易プレビュー（MathJaxで数式を描画） */
export function LatexPreview({ source }: { source: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(async () => {
      const el = ref.current;
      if (!el) return;
      el.innerHTML = latexToPreviewHtml(source);
      await typeset(el);
    }, 300);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [source]);

  return <div ref={ref} className="preview-body" />;
}
