import { X, Save } from "lucide-react";

interface Props {
  url: string;
  onSave: () => void;
  onClose: () => void;
}

/** PDF出力前の確認プレビュー（余白・文字切れの確認用） */
export default function PdfPreviewModal({ url, onSave, onClose }: Props) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal"
        style={{ width: "min(920px, 92vw)", height: "90vh" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="flex items-center gap-3 px-4 h-12 flex-none"
          style={{ borderBottom: "1px solid var(--border)" }}
        >
          <span className="text-[13px] font-bold">PDFプレビュー</span>
          <span className="text-[11px]" style={{ color: "var(--text-dim)" }}>
            余白・文字切れを確認してから保存してください
          </span>
          <div className="flex-1" />
          <button className="btn btn-primary" onClick={onSave}>
            <Save size={14} /> PDFを保存
          </button>
          <button className="btn-icon" onClick={onClose} title="閉じる">
            <X size={17} />
          </button>
        </div>
        <iframe
          src={url}
          title="PDFプレビュー"
          className="flex-1 w-full"
          style={{ border: "none", background: "#33363d" }}
        />
      </div>
    </div>
  );
}
