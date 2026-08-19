import { useEffect, useMemo, useRef, useState } from "react";
import {
  aiChatCancel,
  aiChatConfirm,
  aiChatCreateSession,
  aiChatGetSession,
  aiChatRedo,
  aiChatReadAttachment,
  aiChatRegenerate,
  aiChatSendMessage,
  aiChatUndo,
  aiStoreInputImage,
} from "../api";
import { useApp } from "../store";
import type { AiChatMessage, AiChatSession, AiChatStatus } from "../types";
import type { AiChatLaunchTarget } from "../aiChat";
import { Icon } from "./Icon";
import { ChatMarkdown } from "./ChatMarkdown";

interface PendingImage {
  file: File;
  url: string;
}

function ChatAttachmentImage({ sessionId, storedName, name }: { sessionId: string; storedName: string; name: string }) {
  const [source, setSource] = useState("");
  useEffect(() => {
    let active = true;
    void aiChatReadAttachment(sessionId, storedName)
      .then((image) => {
        if (active) setSource(`data:${image.mimeType};base64,${image.dataBase64}`);
      })
      .catch(() => {});
    return () => { active = false; };
  }, [sessionId, storedName]);
  return source
    ? <img className="ai-chat-message-image" src={source} alt={name} loading="lazy" />
    : <span className="badge badge-muted">画像: {name}</span>;
}

const RUNNING = new Set(["running", "cancelling"]);
const SESSION_KEY = "kyozai-kobo-ai-chat-session";
const TOOL_LABELS: Record<string, string> = {
  create_problem: "問題を登録",
  update_problem: "問題を更新",
  get_part: "部品を取得",
  update_part: "部品を更新",
  generate_solution: "解答を生成・検査・保存",
  generate_explanation: "解説を生成・検査・保存",
  create_material: "教材を作成",
  add_problem_to_material: "教材へ問題を追加",
  reorder_material_problems: "教材を並べ替え",
  replace_material_problem: "教材の問題を交換",
  create_topic_explanation: "分野解説を保存",
  generate_pdf: "PDFを生成",
  create_graph: "グラフを作成",
  create_2d_figure: "平面図形を作成",
  create_3d_figure: "空間図形を作成",
  undo_action: "AI操作を元に戻す",
  redo_action: "AI操作をやり直す",
};

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("画像を読み込めません"));
    reader.onload = () => {
      const result = String(reader.result ?? "");
      resolve(result.includes(",") ? result.slice(result.indexOf(",") + 1) : result);
    };
    reader.readAsDataURL(file);
  });
}

function messageClass(message: AiChatMessage): string {
  if (message.role === "user") return "ai-chat-message ai-chat-message-user";
  if (message.role === "tool") return `ai-chat-tool ai-chat-tool-${message.status}`;
  return "ai-chat-message ai-chat-message-assistant";
}

export function AiChatPanel({
  onClose,
  launch,
  onStatusChange,
}: {
  onClose: () => void;
  launch?: AiChatLaunchTarget | null;
  onStatusChange?: (status: AiChatStatus) => void;
}) {
  const {
    view,
    selectedUnitId,
    selectedProblemId,
    selectedProjectId,
    contextName,
    bumps,
    bump,
    showToast,
  } = useApp();
  const [session, setSession] = useState<AiChatSession | null>(null);
  const [input, setInput] = useState("");
  const [images, setImages] = useState<PendingImage[]>([]);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [width, setWidth] = useState(() => Math.min(430, Math.max(320, Number(localStorage.getItem("kk-ai-chat-width")) || 380)));
  const scrollRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const handledLaunchRef = useRef<string | null>(null);
  const imagesRef = useRef(images);
  imagesRef.current = images;

  const context = useMemo(() => {
    const active = document.activeElement as HTMLTextAreaElement | HTMLInputElement | null;
    const selection = active && typeof active.selectionStart === "number"
      ? { start: active.selectionStart, end: active.selectionEnd, fieldName: active.getAttribute("name") ?? active.getAttribute("aria-label") ?? "" }
      : null;
    return {
      currentScreen: launch?.currentScreen ?? view,
      selectedUnitId,
      selectedProblemId: launch ? (launch.kind === "problem" ? launch.id : null) : selectedProblemId,
      selectedPartId: launch?.kind === "part" ? launch.id : null,
      selectedMaterialId: launch ? (launch.kind === "material" ? launch.id : null) : selectedProjectId,
      contextName: launch?.title ?? contextName,
      launchTarget: launch ? {
        requestId: launch.requestId,
        kind: launch.kind,
        id: launch.id,
        title: launch.title,
      } : null,
      editorSelection: selection,
    };
  }, [view, selectedUnitId, selectedProblemId, selectedProjectId, contextName, input, launch?.requestId]);

  const reload = async (id = session?.id) => {
    if (!id) return;
    try {
      setSession(await aiChatGetSession(id));
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  useEffect(() => {
    let active = true;
    if (launch) {
      handledLaunchRef.current = launch.requestId;
      setInput(launch.starter);
      setSession(null);
      void aiChatCreateSession(context)
        .then((created) => {
          localStorage.setItem(SESSION_KEY, created.id);
          if (active) setSession(created);
        })
        .catch((error) => showToast(String(error), "error"));
      return () => { active = false; };
    }
    void (async () => {
      const remembered = localStorage.getItem(SESSION_KEY);
      if (remembered) {
        try {
          const existing = await aiChatGetSession(remembered);
          if (active) setSession(existing);
          return;
        } catch {
          localStorage.removeItem(SESSION_KEY);
        }
      }
      try {
        const created = await aiChatCreateSession(context);
        localStorage.setItem(SESSION_KEY, created.id);
        if (active) setSession(created);
      } catch (error) {
        showToast(String(error), "error");
      }
    })();
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (!launch || handledLaunchRef.current === launch.requestId) return;
    handledLaunchRef.current = launch.requestId;
    let active = true;
    imagesRef.current.forEach((image) => URL.revokeObjectURL(image.url));
    setImages([]);
    setInput(launch.starter);
    setSession(null);
    void aiChatCreateSession(context)
      .then((created) => {
        localStorage.setItem(SESSION_KEY, created.id);
        if (active) setSession(created);
      })
      .catch((error) => showToast(String(error), "error"));
    return () => { active = false; };
  }, [launch?.requestId]);

  useEffect(() => {
    if (!session) return;
    void reload(session.id);
  }, [bumps.ai_chat]);

  useEffect(() => {
    if (!session || !RUNNING.has(session.status)) return;
    const timer = setInterval(() => void reload(session.id), 1400);
    return () => clearInterval(timer);
  }, [session?.id, session?.status]);

  useEffect(() => {
    if (session) onStatusChange?.(session.status);
  }, [session?.status, onStatusChange]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [session?.messages.length, session?.status]);

  useEffect(() => () => imagesRef.current.forEach((image) => URL.revokeObjectURL(image.url)), []);

  const addFiles = (files: File[]) => {
    const accepted = files.filter((file) => file.type.startsWith("image/")).slice(0, Math.max(0, 8 - images.length));
    if (accepted.length !== files.length) showToast("画像ファイルのみ、最大8枚まで添付できます", "error");
    setImages((current) => [...current, ...accepted.map((file) => ({ file, url: URL.createObjectURL(file) }))]);
  };

  const removeImage = (index: number) => {
    setImages((current) => {
      URL.revokeObjectURL(current[index].url);
      return current.filter((_, i) => i !== index);
    });
  };

  const send = async () => {
    if (!session || busy || (!input.trim() && images.length === 0)) return;
    setBusy(true);
    try {
      const uploaded: string[] = [];
      for (const image of images) {
        const stored = await aiStoreInputImage(await fileToBase64(image.file), image.file.name);
        uploaded.push(stored.name);
      }
      const next = await aiChatSendMessage({ sessionId: session.id, content: input.trim(), inputNames: uploaded, context });
      setSession(next);
      setInput("");
      images.forEach((image) => URL.revokeObjectURL(image.url));
      setImages([]);
    } catch (error) {
      showToast(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const act = async (fn: () => Promise<AiChatSession>) => {
    if (busy) return;
    setBusy(true);
    try {
      const next = await fn();
      setSession(next);
      bump("problems");
      bump("projects");
      bump("parts");
      bump("graphs");
    } catch (error) {
      showToast(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const revisePending = async () => {
    if (!session || busy) return;
    setBusy(true);
    try {
      setSession(await aiChatConfirm(session.id, false));
      setInput("登録候補を次のように修正して、もう一度提案してください: ");
    } catch (error) {
      showToast(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const beginResize = (event: React.PointerEvent) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startWidth = width;
    const move = (next: PointerEvent) => setWidth(Math.min(680, Math.max(300, startWidth + startX - next.clientX)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setWidth((value) => {
        localStorage.setItem("kk-ai-chat-width", String(value));
        return value;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const agentRunning = !!session && RUNNING.has(session.status);
  const runningTool = [...(session?.messages ?? [])]
    .reverse()
    .find((message) => message.role === "tool" && message.status === "running");
  const agentActivityText = session?.status === "cancelling"
    ? "進行中の処理を安全に停止しています…"
    : runningTool
      ? runningTool.content.replace(/^[✓×]\s*/, "")
      : "依頼を解析し、実行する操作を組み立てています…";

  return (
    <aside className="ai-chat-panel" style={{ width }} aria-label="AIチャット">
      <div className="ai-chat-resizer" onPointerDown={beginResize} />
      <header className="ai-chat-header">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 font-semibold">
            <Icon name="sparkle" size={15} /> AI Chat
            {launch && <span className="ai-chat-target-badge">{launch.kind === "problem" ? "問題" : launch.kind === "part" ? "部品" : "教材"} #{launch.id}</span>}
          </div>
          <div className="truncate text-[10px]" style={{ color: "var(--muted)" }}>
            {agentRunning ? "AIエージェントが操作中" : launch ? `対象: ${launch.title}` : session?.executionMode === "auto" ? "自動実行" : session?.executionMode === "suggest" ? "提案のみ" : "書き込み前に確認"}
          </div>
        </div>
        <div className="ml-auto flex items-center gap-1">
          <button className="btn btn-ghost btn-sm" disabled={!session || RUNNING.has(session.status)} onClick={() => session && void act(() => aiChatUndo(session.id))} title="直前のAI操作を元に戻す">↶</button>
          <button className="btn btn-ghost btn-sm" disabled={!session || RUNNING.has(session.status)} onClick={() => session && void act(() => aiChatRedo(session.id))} title="AI操作をやり直す">↷</button>
          <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="AIチャットを閉じる">✕</button>
        </div>
      </header>

      {agentRunning && (
        <div className="ai-chat-agent-activity" role="status" aria-live="polite">
          <span className="ai-chat-agent-spinner animate-spin" aria-hidden="true" />
          <div className="min-w-0 flex-1">
            <div className="text-xs font-semibold">
              {session?.status === "cancelling" ? "AIエージェントを停止中" : "AIエージェントが実行中"}
            </div>
            <div className="mt-0.5 truncate text-[10px]" style={{ color: "var(--muted)" }}>
              {agentActivityText}
            </div>
          </div>
          <span className="ai-chat-agent-live">LIVE</span>
        </div>
      )}

      <div className="ai-chat-messages" ref={scrollRef}>
        {!session && <div className="p-4 text-xs" style={{ color: "var(--muted)" }}>AIチャットを準備しています...</div>}
        {session && session.messages.length === 0 && (
          <div className="ai-chat-empty">
            <Icon name="sparkle" size={22} />
            <p>問題検索、画像取込、解答生成、教材作成、PDF生成を自然な言葉で依頼できます。</p>
            <button className="btn btn-outline btn-sm" onClick={() => setInput("微分法のBランクの問題を5問見せて")}>例を入力</button>
          </div>
        )}
        {session?.messages.map((message) => (
          <div key={message.id} className={messageClass(message)}>
            {message.role === "tool" ? (
              <div className="flex items-start gap-2">
                {message.status === "running" ? <span className="mt-0.5 h-3 w-3 animate-spin rounded-full border border-t-transparent" /> : message.status === "failed" ? <span>×</span> : <span>✓</span>}
                <span>{message.content.replace(/^[✓×]\s*/, "")}</span>
              </div>
            ) : (
              <ChatMarkdown>{message.content}</ChatMarkdown>
            )}
            {message.attachments.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1">
                {message.attachments.map((attachment) => (
                  <ChatAttachmentImage key={attachment.stored_name} sessionId={session.id} storedName={attachment.stored_name} name={attachment.name} />
                ))}
              </div>
            )}
          </div>
        ))}
      </div>

      {session?.status === "awaiting_confirmation" && (
        <div className="ai-chat-confirm">
          <div className="text-xs font-semibold">{session.executionMode === "suggest" ? "操作案（提案のみ）" : "この操作を実行しますか？"}</div>
          <div className="mt-1 text-[11px]" style={{ color: "var(--muted)" }}>{session.pendingCalls.map((call) => TOOL_LABELS[call.name] ?? call.name).join(" / ")}</div>
          {session.pendingCalls.filter((call) => call.name === "create_problem").map((call) => {
            const args = call.arguments;
            return (
              <div className="ai-chat-problem-preview" key={call.call_id}>
                <div className="font-semibold">{String(args.title ?? "問題登録候補")}</div>
                <div>単元ID: {String(args.unit_id ?? "要確認")} · 難易度: {String(args.difficulty_rank ?? "未判定")} · 必須: {args.required === true ? "＊" : "なし"}</div>
                <div className="ai-chat-problem-preview-body">{String(args.statement_latex ?? "本文を認識できませんでした")}</div>
              </div>
            );
          })}
          <div className="mt-2 flex justify-end gap-2">
            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void act(() => aiChatConfirm(session.id, false))}>{session.executionMode === "suggest" ? "閉じる" : "キャンセル"}</button>
            {session.executionMode !== "suggest" && session.pendingCalls.some((call) => call.name === "create_problem") && (
              <button className="btn btn-outline btn-sm" disabled={busy} onClick={() => void revisePending()}>修正</button>
            )}
            {session.executionMode !== "suggest" && <button className="btn btn-solid btn-sm" disabled={busy} onClick={() => void act(() => aiChatConfirm(session.id, true))}>実行</button>}
          </div>
        </div>
      )}

      <div
        className={`ai-chat-composer ${dragging ? "ai-chat-composer-drag" : ""}`}
        onDragOver={(event) => { event.preventDefault(); setDragging(true); }}
        onDragLeave={() => setDragging(false)}
        onDrop={(event) => { event.preventDefault(); setDragging(false); addFiles(Array.from(event.dataTransfer.files)); }}
        onPaste={(event) => {
          const pasted = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
          if (pasted.length) { event.preventDefault(); addFiles(pasted); }
        }}
      >
        {images.length > 0 && (
          <div className="ai-chat-image-strip">
            {images.map((image, index) => (
              <div key={`${image.file.name}-${index}`} className="ai-chat-image-thumb">
                <img src={image.url} alt={image.file.name} />
                <button onClick={() => removeImage(index)} aria-label="画像を外す">×</button>
              </div>
            ))}
          </div>
        )}
        <textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void send();
            }
          }}
          className="ai-chat-input"
          placeholder="例: 数列のBランクから5問選んで、簡単な順に教材を作って"
          disabled={!session || RUNNING.has(session.status) || session.status === "awaiting_confirmation"}
        />
        <div className="ai-chat-composer-actions">
          <input ref={fileRef} type="file" accept="image/*" multiple hidden onChange={(event) => addFiles(Array.from(event.target.files ?? []))} />
          <button className="btn btn-ghost btn-sm" onClick={() => fileRef.current?.click()} disabled={!session || RUNNING.has(session.status)} title="画像を添付">＋画像</button>
          <button className="btn btn-ghost btn-sm" disabled={!session || session.messages.length === 0 || session.status !== "idle"} onClick={() => session && void act(() => aiChatRegenerate(session.id))}>再生成</button>
          <span className="ml-auto" />
          {session && RUNNING.has(session.status) ? (
            <button className="btn btn-outline btn-sm" onClick={async () => { try { await aiChatCancel(session.id); await reload(session.id); } catch (error) { showToast(String(error), "error"); } }}>停止</button>
          ) : (
            <button className="btn btn-solid btn-sm" disabled={busy || !session || session.status === "awaiting_confirmation" || (!input.trim() && images.length === 0)} onClick={() => void send()}>送信</button>
          )}
        </div>
        <div className="ai-chat-hint">Enterで送信 · Shift+Enterで改行 · 画像はドロップ/貼り付け可</div>
      </div>
    </aside>
  );
}
