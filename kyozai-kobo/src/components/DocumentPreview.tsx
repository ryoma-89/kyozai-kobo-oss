import { LatexPreview } from "./LatexPreview";

export type PreviewLayout = "single_column" | "two_column";

export interface PreviewSection {
  label?: string;
  source: string;
}

interface DocumentPreviewProps {
  title?: string;
  eyebrow?: string;
  layout: PreviewLayout;
  sections: PreviewSection[];
  zoom: number;
  emptyMessage?: string;
}

/**
 * A4紙面に近い寸法・余白・段組でLaTeXの簡易表示を行う。
 * 正確な改ページやTikZはPDFプレビューへ任せ、編集中の確認を即時に行うための表示。
 */
export function DocumentPreview({
  title,
  eyebrow,
  layout,
  sections,
  zoom,
  emptyMessage = "プレビューする内容がありません",
}: DocumentPreviewProps) {
  const visibleSections = sections.filter((section) => section.source.trim());

  return (
    <div className="paper-preview-stage" data-layout={layout}>
      <article
        className={`paper paper-preview-page paper-preview-page-${layout}`}
        style={{ zoom: zoom / 100 }}
        aria-label={`${layout === "two_column" ? "二段組" : "一段組"}簡易プレビュー`}
      >
        {(eyebrow || title) && (
          <header className="paper-preview-header">
            {eyebrow && <div className="paper-preview-eyebrow">{eyebrow}</div>}
            {title && <h2 className="paper-preview-title">{title}</h2>}
          </header>
        )}
        <div className={`paper-preview-content paper-preview-content-${layout}`}>
          {visibleSections.length > 0 ? (
            visibleSections.map((section, index) => (
              <section className="paper-preview-section" key={`${section.label ?? "section"}-${index}`}>
                {section.label && <h3 className="paper-preview-section-label">{section.label}</h3>}
                <LatexPreview source={section.source} />
              </section>
            ))
          ) : (
            <div className="paper-preview-empty">{emptyMessage}</div>
          )}
        </div>
        <footer className="paper-preview-footer">
          <span>{layout === "two_column" ? "二段組" : "一段組"}</span>
          <span>簡易表示</span>
        </footer>
      </article>
    </div>
  );
}
