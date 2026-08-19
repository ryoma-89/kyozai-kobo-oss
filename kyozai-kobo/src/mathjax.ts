// MathJax（オフライン同梱）で要素内の数式を組版する
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

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/**
 * LaTeXソースを簡易プレビュー用HTMLに変換する。
 * 数式はそのまま残してMathJaxに任せ、よく使う環境だけHTMLに置き換える。
 */
export function latexToPreviewHtml(src: string): string {
  let s = escapeHtml(src);
  // 簡易表示で描画できない図や複雑な表は、長い生ソースを見せずPDF確認へ案内する。
  s = s.replace(
    /(?:\\resizebox\{[^{}]*\}\{!\}\{)?\\begin\{tikzpicture\}[\s\S]*?\\end\{tikzpicture\}\}?/g,
    '<span class="preview-placeholder">TikZ図（PDFで正確に表示）</span>',
  );
  s = s.replace(
    /\\begin\{tabular\}(?:\{[^{}]*\})?[\s\S]*?\\end\{tabular\}/g,
    '<span class="preview-placeholder">表（PDFで正確に表示）</span>',
  );
  // 紙面制御用コマンドはDocumentPreview側で表現する。
  s = s.replace(/\\begin\{multicols\}\{2\}|\\end\{multicols\}/g, "");
  s = s.replace(/\\(?:noindent|raggedcolumns|smallskip|medskip|bigskip)\b/g, "");
  s = s.replace(/\\(?:vspace|hspace)\*?\{[^{}]*\}/g, "");
  s = s.replace(/\\par\b/g, "\n\n");
  // 見出し
  s = s.replace(/\\section\*?\{([^{}]*)\}/g, '<h3 class="preview-heading">$1</h3>');
  s = s.replace(/\\subsection\*?\{([^{}]*)\}/g, '<h4 class="preview-subheading">$1</h4>');
  // 箇条書き環境
  s = s.replace(/\\begin\{enumerate\}(\[[^\]]*\])?/g, "<ol>");
  s = s.replace(/\\end\{enumerate\}/g, "</ol>");
  s = s.replace(/\\begin\{itemize\}/g, "<ul>");
  s = s.replace(/\\end\{itemize\}/g, "</ul>");
  s = s.replace(/\\item\s*/g, "<li>");
  // センタリング
  s = s.replace(/\\begin\{center\}/g, '<div style="text-align:center">');
  s = s.replace(/\\end\{center\}/g, "</div>");
  // 太字・強調（単純なもののみ）
  s = s.replace(/\\textbf\{([^{}]*)\}/g, "<strong>$1</strong>");
  s = s.replace(/\\textit\{([^{}]*)\}/g, "<em>$1</em>");
  s = s.replace(/\\underline\{([^{}]*)\}/g, "<u>$1</u>");
  // 画像はプレースホルダ表示
  s = s.replace(
    /\\includegraphics(\[[^\]]*\])?\{([^{}]*)\}/g,
    '<span style="display:inline-block;border:1px dashed #9ca3af;color:#6b7280;padding:2px 10px;border-radius:4px;">画像: $2</span>',
  );
  // 改ページ
  s = s.replace(/\\newpage|\\clearpage/g, '<hr style="border-top:1px dashed #9ca3af" />');
  // 段落
  const paragraphs = s.split(/\n\s*\n/);
  return paragraphs.map((p) => `<p>${p.replace(/\n/g, "<br/>")}</p>`).join("");
}

export async function typeset(el: HTMLElement): Promise<void> {
  const mj = window.MathJax;
  if (!mj?.typesetPromise) return;
  try {
    if (mj.startup?.promise) await mj.startup.promise;
    mj.typesetClear?.([el]);
    await mj.typesetPromise([el]);
  } catch {
    // 数式エラーはプレビューなので無視
  }
}
