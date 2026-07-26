import { useEffect, useMemo, useRef, useState } from "react";
import { buildFileDownloadUrl, buildFileUrl } from "../transport";

type StandaloneNavigator = Navigator & {
  standalone?: boolean;
};

type CachedShareFile = {
  key: string;
  promise: Promise<File>;
};

let cachedShareFile: CachedShareFile | null = null;

function isStandaloneWebApp(): boolean {
  if (typeof window === "undefined") return false;
  return (
    window.matchMedia("(display-mode: standalone)").matches
    || (navigator as StandaloneNavigator).standalone === true
  );
}

function pdfFilename(path: string): string {
  const filename = path.split(/[\\/]/).pop()?.trim();
  return filename && filename.toLowerCase().endsWith(".pdf") ? filename : "教材.pdf";
}

function canPrepareShareFile(): boolean {
  return (
    isStandaloneWebApp()
    && typeof navigator.share === "function"
    && typeof navigator.canShare === "function"
    && typeof File !== "undefined"
  );
}

function loadShareFile(path: string, cacheKey: string | number): Promise<File> {
  const key = `${path}\n${cacheKey}`;
  if (cachedShareFile?.key === key) return cachedShareFile.promise;
  const promise = fetch(buildFileUrl(path, cacheKey), {
    cache: "no-store",
    credentials: "same-origin",
  }).then(async (response) => {
    if (!response.ok) throw new Error(`PDFの取得に失敗しました（HTTP ${response.status}）`);
    const blob = await response.blob();
    return new File([blob], pdfFilename(path), {
      type: "application/pdf",
      lastModified: Date.now(),
    });
  });
  cachedShareFile = { key, promise };
  return promise;
}

export function PdfSaveButton({
  path,
  cacheKey,
  className = "",
  compact = false,
  onError,
}: {
  path: string;
  cacheKey: string | number;
  className?: string;
  compact?: boolean;
  onError?: (message: string) => void;
}) {
  const shareMode = useMemo(() => canPrepareShareFile(), []);
  const [shareFile, setShareFile] = useState<File | null>(null);
  const [shareUnavailable, setShareUnavailable] = useState(false);
  const onErrorRef = useRef(onError);
  const downloadUrl = buildFileDownloadUrl(path, cacheKey);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  useEffect(() => {
    if (!shareMode) return;
    let active = true;
    setShareFile(null);
    setShareUnavailable(false);
    void loadShareFile(path, cacheKey)
      .then((file) => {
        if (!active) return;
        if (!navigator.canShare({ files: [file] })) {
          setShareUnavailable(true);
          return;
        }
        setShareFile(file);
      })
      .catch((error) => {
        if (!active) return;
        setShareUnavailable(true);
        onErrorRef.current?.(String(error));
      });
    return () => {
      active = false;
    };
  }, [cacheKey, path, shareMode]);

  const buttonClass = `${className} ${compact ? "btn-sm" : ""}`.trim();

  if (shareMode && !shareUnavailable) {
    return (
      <button
        type="button"
        className={buttonClass}
        disabled={!shareFile}
        aria-busy={!shareFile}
        onClick={() => {
          if (!shareFile) return;
          void navigator
            .share({
              files: [shareFile],
              title: shareFile.name,
            })
            .catch((error: unknown) => {
              if ((error as { name?: string })?.name !== "AbortError") {
                onErrorRef.current?.(`PDFを保存できませんでした: ${String(error)}`);
              }
            });
        }}
      >
        {shareFile ? "PDFを保存" : "保存準備中…"}
      </button>
    );
  }

  return (
    <a
      className={buttonClass}
      href={downloadUrl}
      download
      target="_blank"
      rel="noopener noreferrer"
    >
      PDFを保存
    </a>
  );
}
