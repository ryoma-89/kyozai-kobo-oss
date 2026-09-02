import { lazy, Suspense, useEffect, useRef, useState } from "react";
import {
  aiCancelJob,
  aiChatSessionStatus,
  aiListJobs,
  createSampleData,
  getSettings,
  hasAnyData,
  openPath,
  setSettings,
  showInFolder,
} from "./api";
import { AiJobsView } from "./components/AiJobsView";
import { BankView } from "./components/BankView";
import { PairingScreen } from "./components/PairingScreen";
import { PdfCanvasViewer } from "./components/PdfCanvasViewer";
import { PdfSaveButton } from "./components/PdfSaveButton";
import { PartsView } from "./components/PartsView";
import { PatternLibraryView } from "./components/PatternLibraryView";
import { ProjectsView } from "./components/ProjectsView";
import { SearchView } from "./components/SearchView";
import { SetupGuide } from "./components/SetupGuide";
import { SettingsView } from "./components/SettingsView";
import { TemplatesView } from "./components/TemplatesView";
import { ConfirmDialog, Modal, Toast } from "./components/ui";
import { Icon } from "./components/Icon";
import { useApp, type View } from "./store";
import {
  authLogout,
  authMe,
  buildFileUrl,
  isTauri,
  subscribeEvents,
} from "./transport";
import type { AiChatStatus, AiJob } from "./types";
import type { AiChatLaunchTarget } from "./aiChat";

const NAV: { view: View; label: string; icon: string }[] = [
  { view: "bank", label: "問題バンク", icon: "▦" },
  { view: "patterns", label: "定石", icon: "◇" },
  { view: "projects", label: "教材", icon: "▤" },
  { view: "parts", label: "部品", icon: "▧" },
  { view: "templates", label: "テンプレート", icon: "❖" },
  { view: "graphs", label: "グラフ", icon: "⌁" },
  { view: "ai", label: "AI変換", icon: "" },
];

const GraphsView = lazy(() => import("./components/GraphsView").then((module) => ({ default: module.GraphsView })));
const AiChatPanel = lazy(() => import("./components/AiChatPanel").then((module) => ({ default: module.AiChatPanel })));

const RUNNING_AI_STATUSES = new Set([
  "queued",
  "preprocessing",
  "waiting_for_codex",
  "converting",
  "validating",
  "compiling",
]);
const RUNNING_AI_CHAT_STATUSES = new Set<AiChatStatus>(["running", "cancelling"]);
const AI_CHAT_SESSION_KEY = "kyozai-kobo-ai-chat-session";

export default function App() {
  const {
    view,
    setView,
    dirty,
    refreshTree,
    showToast,
    contextName,
    lastCompile,
    logOpen,
    setLogOpen,
    bump,
    bumps,
    connected,
    setConnected,
    confirm,
    graphOverlay,
    closeGraphOverlay,
  } = useApp();
  const [welcome, setWelcome] = useState(false);
  const [setupGuideOpen, setSetupGuideOpen] = useState(false);
  const [initialHasData, setInitialHasData] = useState<boolean | null>(null);
  // Web版の認証状態: null=確認中
  const [authed, setAuthed] = useState<boolean | null>(isTauri ? true : null);
  const [authUnavailable, setAuthUnavailable] = useState(false);
  const [pdfViewer, setPdfViewer] = useState<{
    title: string;
    url: string;
    path: string;
    downloadKey: number;
    zoom: number;
  } | null>(null);
  const [aiJobs, setAiJobs] = useState<AiJob[]>([]);
  const [latestFinishedAiJob, setLatestFinishedAiJob] = useState<AiJob | null>(null);
  const [hiddenAiPanelKeys, setHiddenAiPanelKeys] = useState<Set<string>>(() => new Set());
  const [aiChatOpen, setAiChatOpen] = useState(() => localStorage.getItem("kk-ai-chat-open") === "1");
  const [aiChatLaunch, setAiChatLaunch] = useState<AiChatLaunchTarget | null>(null);
  const [aiChatStatus, setAiChatStatus] = useState<AiChatStatus | null>(null);
  const aiStatusesRef = useRef<Map<number, string>>(new Map());
  const aiActivityInitializedRef = useRef(false);

  const refreshAiActivity = async () => {
    try {
      const jobs = (await aiListJobs(30)).filter((job) => job.options.hideFromHistory !== true);
      if (aiActivityInitializedRef.current) {
        const newlyFinished = jobs
          .filter((job) => {
            const previous = aiStatusesRef.current.get(job.id);
            return !!previous
              && RUNNING_AI_STATUSES.has(previous)
              && !RUNNING_AI_STATUSES.has(job.status);
          })
          .sort((a, b) => b.id - a.id)[0];
        if (newlyFinished) setLatestFinishedAiJob(newlyFinished);
      }
      aiStatusesRef.current = new Map(jobs.map((job) => [job.id, job.status]));
      aiActivityInitializedRef.current = true;
      setAiJobs(jobs);
    } catch {
      /* AI機能を使っていない画面の操作は妨げない */
    }
  };

  const refreshAiChatActivity = async () => {
    const sessionId = localStorage.getItem(AI_CHAT_SESSION_KEY);
    if (!sessionId) {
      setAiChatStatus(null);
      return;
    }
    try {
      const session = await aiChatSessionStatus(sessionId);
      setAiChatStatus(session.status);
    } catch {
      /* 一時的な通信断では直前の実行表示を維持する */
    }
  };

  const runningAiJobs = aiJobs.filter((job) => RUNNING_AI_STATUSES.has(job.status));
  const aiChatAgentRunning = aiChatStatus !== null && RUNNING_AI_CHAT_STATUSES.has(aiChatStatus);
  const aiPanelKey = (job: AiJob) =>
    `${job.id}:${RUNNING_AI_STATUSES.has(job.status) ? "running" : job.status}`;
  const floatingAiJob =
    runningAiJobs.find((job) => !hiddenAiPanelKeys.has(aiPanelKey(job)))
    ?? (latestFinishedAiJob && !hiddenAiPanelKeys.has(aiPanelKey(latestFinishedAiJob))
      ? latestFinishedAiJob
      : null);

  const navigate = async (next: View, focusSearch = false) => {
    if (next === view) return;
    if (dirty && !(await confirm("未保存の変更があります。保存せずに画面を移動しますか？"))) return;
    setView(next);
    if (focusSearch) {
      requestAnimationFrame(() => {
        document.querySelector<HTMLInputElement>("[data-search-input]")?.focus();
      });
    }
  };

  const openAiJob = (job: AiJob) => {
    const detail = { jobId: job.id, handled: false };
    window.dispatchEvent(new CustomEvent("kk-open-ai-job", { detail }));
    if (!detail.handled) void navigate("ai");
    if (!RUNNING_AI_STATUSES.has(job.status)) setLatestFinishedAiJob(null);
  };

  const hideAiPanel = (job: AiJob) => {
    setHiddenAiPanelKeys((current) => {
      const next = new Set(current);
      next.add(aiPanelKey(job));
      return next;
    });
  };

  const cancelAiJob = async (job: AiJob) => {
    try {
      await aiCancelJob(job.id);
      showToast(`AIジョブ #${job.id} をキャンセルしました`);
      await refreshAiActivity();
    } catch (error) {
      showToast(String(error), "error");
    }
  };

  const checkWebAuth = async () => {
    try {
      const result = await authMe();
      setAuthed(result.authenticated);
      setAuthUnavailable(false);
      setConnected(true);
      if (result.authenticated) {
        localStorage.setItem("kk-was-authenticated", "1");
      } else {
        localStorage.removeItem("kk-was-authenticated");
      }
    } catch {
      setConnected(false);
      setAuthUnavailable(true);
      if (localStorage.getItem("kk-was-authenticated") === "1") {
        setAuthed(true);
      } else {
        setAuthed(null);
      }
    }
  };

  // Web版: 認証チェック
  useEffect(() => {
    if (isTauri) return;
    void checkWebAuth();
    const onAuthRequired = () => {
      localStorage.removeItem("kk-was-authenticated");
      setAuthed(false);
      setAuthUnavailable(false);
    };
    window.addEventListener("kk-auth-required", onAuthRequired);
    return () => window.removeEventListener("kk-auth-required", onAuthRequired);
  }, []);

  // SPA直リンク: /graphs と /graphs/:id はグラフ画面として開く。
  useEffect(() => {
    if (!isTauri && window.location.pathname.startsWith("/graphs")) setView("graphs");
  }, [setView]);

  // 変更イベントの購読（他端末での変更を反映）
  useEffect(() => {
    if (!authed) return;
    const unsubscribe = subscribeEvents(
      (ev) => {
        bump(ev.kind);
        if (ev.kind === "tree" || ev.kind === "problems") {
          refreshTree();
        }
      },
      (ok) => setConnected(ok),
    );
    return unsubscribe;
  }, [authed]);

  // AIジョブは画面を閉じても継続するため、アプリ全体で進捗と完了を追う。
  useEffect(() => {
    if (!authed) return;
    void refreshAiActivity();
  }, [authed, bumps.ai_job]);

  useEffect(() => {
    if (!authed || runningAiJobs.length === 0) return;
    const timer = setInterval(() => void refreshAiActivity(), 2500);
    return () => clearInterval(timer);
  }, [authed, runningAiJobs.map((job) => `${job.id}:${job.status}`).join(",")]);

  // チャットを閉じても、上部のAI Chatボタンでエージェントの実行状態を追う。
  useEffect(() => {
    if (!authed) return;
    void refreshAiChatActivity();
  }, [authed, bumps.ai_chat, aiChatOpen]);

  useEffect(() => {
    if (!authed || !aiChatAgentRunning || aiChatOpen) return;
    const timer = setInterval(() => void refreshAiChatActivity(), 1400);
    return () => clearInterval(timer);
  }, [authed, aiChatAgentRunning, aiChatOpen]);

  // Web版: オフライン検知
  useEffect(() => {
    if (isTauri) return;
    const onOffline = () => setConnected(false);
    const onOnline = () => {
      void checkWebAuth();
    };
    window.addEventListener("offline", onOffline);
    window.addEventListener("online", onOnline);
    return () => {
      window.removeEventListener("offline", onOffline);
      window.removeEventListener("online", onOnline);
    };
  }, []);

  // 初回起動時: 先に必要環境を案内し、その後データが空ならサンプル作成を提案する
  useEffect(() => {
    if (!isTauri || !authed) return;
    let active = true;
    Promise.all([
      getSettings().catch(() => ({} as Record<string, string>)),
      hasAnyData(),
    ])
      .then(([settings, has]) => {
        if (!active) return;
        setInitialHasData(has);
        if (settings["setup_guide_completed"] !== "1") {
          setWelcome(false);
          setSetupGuideOpen(true);
        } else if (!has) {
          setWelcome(true);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [authed]);

  // グローバルショートカット: Ctrl+F で検索
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        void navigate("search", true);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [dirty, view]);

  useEffect(() => {
    const handler = (event: BeforeUnloadEvent) => {
      if (!dirty) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, [dirty]);

  useEffect(() => {
    const openForTarget = (event: Event) => {
      const detail = (event as CustomEvent<AiChatLaunchTarget>).detail;
      if (!detail) return;
      setAiChatLaunch(detail);
      localStorage.setItem("kk-ai-chat-open", "1");
      setAiChatOpen(true);
    };
    window.addEventListener("kk-open-ai-chat", openForTarget);
    return () => window.removeEventListener("kk-open-ai-chat", openForTarget);
  }, []);

  const onSample = async (create: boolean) => {
    setWelcome(false);
    if (!create) return;
    try {
      await createSampleData();
      await refreshTree();
      showToast("サンプルデータを追加しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const finishSetupGuide = async (openSettings: boolean) => {
    try {
      await setSettings({ setup_guide_completed: "1" });
      setSetupGuideOpen(false);
      if (openSettings) {
        setView("settings");
      } else if (initialHasData === false) {
        setWelcome(true);
      }
    } catch (error) {
      showToast(`セットアップ案内の状態を保存できませんでした: ${String(error)}`, "error");
    }
  };

  // Web版: 認証確認中／未認証
  if (!isTauri && authed === null) {
    return (
      <div className="flex h-full items-center justify-center text-sm" style={{ color: "var(--muted)" }}>
        {authUnavailable
          ? "教材サーバーへ接続できません。PCの起動状態とネットワークを確認してください。"
          : "接続を確認しています..."}
      </div>
    );
  }
  if (!isTauri && authed === false) {
    return (
      <PairingScreen
        onPaired={() => {
          localStorage.setItem("kk-was-authenticated", "1");
          setAuthed(true);
          setAuthUnavailable(false);
          refreshTree();
        }}
      />
    );
  }

  return (
    <div className={`app-shell flex h-full min-w-0 flex-col overflow-hidden ${lastCompile ? "has-compile-log" : ""}`}>
      {/* トップバー */}
      <header
        className="app-header flex h-10 shrink-0 items-center gap-3 border-b px-3"
        style={{ borderColor: "var(--border)" }}
      >
        <span className="text-sm font-bold tracking-wider">
          <span className="brand-mark">◆</span> 教材工房
        </span>
        {contextName && (
          <>
            <span className="app-header-context" style={{ color: "var(--border-strong)" }}>/</span>
            <span className="app-header-context max-w-80 truncate text-xs" style={{ color: "var(--muted)" }}>
              {contextName}
            </span>
          </>
        )}
        {dirty && (
          <span
            className="badge badge-warn"
            title="保存されていない変更があります (Ctrl+S で保存)"
          >
            ● 未保存
          </span>
        )}
        {!isTauri && !connected && (
          <span
            className="badge"
            style={{ color: "var(--danger)", borderColor: "rgba(241,106,117,0.4)", background: "var(--danger-dim)" }}
            title="サーバーへ接続できません。編集内容は保存ボタンを押すまで端末内に保持されます"
          >
            <Icon name="warning" size={14} /> オフライン
          </span>
        )}
        <span className="app-header-actions ml-auto flex min-w-0 items-center gap-1.5">
          {runningAiJobs.length > 0 ? (
            <button
              onClick={() => {
                setLatestFinishedAiJob(null);
                openAiJob(runningAiJobs[0]);
              }}
              className="btn btn-sm btn-outline"
              title="AIジョブの進捗を開く"
            >
              <span
                className="h-3 w-3 animate-spin rounded-full border-2 border-t-transparent"
                style={{ borderColor: "var(--accent)", borderTopColor: "transparent" }}
              />
              AI {runningAiJobs.length}件処理中
            </button>
          ) : latestFinishedAiJob ? (
            <button
              onClick={() => openAiJob(latestFinishedAiJob)}
              className="btn btn-sm btn-outline"
              style={latestFinishedAiJob.status === "completed" ? { color: "var(--success)" } : { color: "var(--danger)" }}
              title="AIジョブの結果を開く"
            >
              {latestFinishedAiJob.status === "completed" ? "✓" : <Icon name="warning" size={14} />}
              AI #{latestFinishedAiJob.id} {latestFinishedAiJob.status === "completed" ? "完了" : "要確認"}
            </button>
          ) : null}
          <button
            onClick={() => {
              if (!aiChatOpen) setAiChatLaunch(null);
              setAiChatOpen((current) => {
                localStorage.setItem("kk-ai-chat-open", current ? "0" : "1");
                return !current;
              });
            }}
            className={`btn btn-sm ${aiChatOpen ? "btn-outline" : "btn-ghost"}`}
            title={aiChatAgentRunning ? "AIエージェントが実行中です。チャットを開く" : "AIチャットを開閉"}
          >
            {aiChatAgentRunning ? (
              <span
                className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-t-transparent"
                style={{ borderColor: "var(--purple)", borderTopColor: "transparent" }}
              />
            ) : (
              <Icon name="sparkle" size={15} />
            )}
            AI Chat{aiChatAgentRunning ? " 実行中" : ""}
          </button>
          <button
            onClick={() => {
              void navigate("search", true);
            }}
            className={`btn btn-sm ${view === "search" ? "btn-outline" : "btn-ghost"}`}
          >
            <Icon name="search" size={15} /> 検索 <span className="app-shortcut" style={{ color: "var(--muted)", fontSize: 10 }}>Ctrl+F</span>
          </button>
          <button
            onClick={() => void navigate("settings")}
            className={`btn btn-sm ${view === "settings" ? "btn-outline" : "btn-ghost"}`}
            title="設定"
          >
            <Icon name="settings" size={15} /> 設定
          </button>
          {!isTauri && (
            <button
              onClick={async () => {
                try {
                  await authLogout();
                } finally {
                  localStorage.removeItem("kk-was-authenticated");
                  setAuthed(false);
                }
              }}
              className="btn btn-sm btn-ghost"
              title="この端末のセッションを終了"
            >
              ログアウト
            </button>
          )}
        </span>
      </header>

      <div className="app-body flex min-h-0 min-w-0 flex-1">
        {/* 左ナビゲーション */}
        <nav
          className="app-nav flex w-[64px] shrink-0 flex-col items-center gap-1 border-r py-2"
          style={{ borderColor: "var(--border)" }}
        >
          {NAV.map((n) => (
            <button
              key={n.view}
              onClick={() => void navigate(n.view)}
              className={`app-nav-btn flex w-[56px] flex-col items-center rounded py-1.5 text-[9.5px] ${view === n.view ? "app-nav-btn-active" : ""}`}
              title={n.label}
            >
              <span className="text-base leading-6">{n.view === "ai" ? <Icon name="sparkle" size={17} /> : n.icon}</span>
              {n.label}
            </button>
          ))}
        </nav>

        {/* メイン */}
        <main className="app-main min-h-0 min-w-0 flex-1 overflow-hidden" style={{ background: "var(--bg)" }}>
          {view === "bank" && <BankView />}
          {view === "patterns" && <PatternLibraryView />}
          {view === "search" && <SearchView />}
          {view === "projects" && <ProjectsView />}
          {view === "parts" && <PartsView />}
          {view === "templates" && <TemplatesView />}
          {view === "graphs" && (
            <Suspense fallback={<div className="flex h-full items-center justify-center text-xs" style={{ color: "var(--muted)" }}>グラフ機能を読み込んでいます...</div>}>
              <GraphsView />
            </Suspense>
          )}
          {view === "ai" && <AiJobsView />}
          {view === "settings" && <SettingsView />}
        </main>
        {aiChatOpen && (
          <Suspense fallback={<aside className="ai-chat-panel flex items-center justify-center text-xs" style={{ width: 380, color: "var(--muted)" }}>AIチャットを読み込んでいます...</aside>}>
            <AiChatPanel
              launch={aiChatLaunch}
              onStatusChange={setAiChatStatus}
              onClose={() => {
                localStorage.setItem("kk-ai-chat-open", "0");
                setAiChatOpen(false);
              }}
            />
          </Suspense>
        )}
      </div>

      {floatingAiJob && !aiChatOpen && (
        <aside
          className="ai-progress-panel fixed z-30 w-[min(22rem,calc(100vw-2rem))] rounded-md border p-3 shadow-2xl"
          style={{ background: "var(--panel)", borderColor: "var(--border-strong)" }}
          role="status"
          aria-live="polite"
        >
          <div className="flex items-start gap-2">
            {RUNNING_AI_STATUSES.has(floatingAiJob.status) ? (
              <span
                className="mt-0.5 h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-t-transparent"
                style={{ borderColor: "var(--accent)", borderTopColor: "transparent" }}
              />
            ) : floatingAiJob.status === "completed" ? (
              <span className="shrink-0" style={{ color: "var(--success)" }}>✓</span>
            ) : (
              <Icon name="warning" size={16} />
            )}
            <div className="min-w-0 flex-1">
              <div className="text-xs font-semibold">
                AIジョブ #{floatingAiJob.id}{" "}
                {RUNNING_AI_STATUSES.has(floatingAiJob.status)
                  ? "を実行中"
                  : floatingAiJob.status === "completed"
                    ? "が完了"
                    : "を確認してください"}
              </div>
              <div className="mt-1 line-clamp-2 text-[11px]" style={{ color: "var(--muted)" }}>
                {RUNNING_AI_STATUSES.has(floatingAiJob.status)
                  ? floatingAiJob.progressMessage || "処理を続けています..."
                  : floatingAiJob.status === "failed"
                    ? floatingAiJob.errorMessage || "生成に失敗しました"
                    : "生成結果を確認できます"}
              </div>
              {RUNNING_AI_STATUSES.has(floatingAiJob.status) && runningAiJobs.length > 1 && (
                <div className="mt-1 text-[10px]" style={{ color: "var(--muted)" }}>
                  ほか {runningAiJobs.length - 1} 件を処理中
                </div>
              )}
            </div>
            <button
              type="button"
              className="btn btn-ghost btn-sm shrink-0"
              onClick={() => hideAiPanel(floatingAiJob)}
              aria-label="進捗パネルを閉じる"
              title="パネルを閉じる（処理は継続します）"
            >
              ✕
            </button>
          </div>
          <div className="mt-3 flex justify-end gap-2">
            {RUNNING_AI_STATUSES.has(floatingAiJob.status) && (
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void cancelAiJob(floatingAiJob)}
              >
                キャンセル
              </button>
            )}
            <button
              type="button"
              className="btn btn-outline btn-sm"
              onClick={() => openAiJob(floatingAiJob)}
            >
              {RUNNING_AI_STATUSES.has(floatingAiJob.status) ? "進捗を開く" : "結果を確認"}
            </button>
          </div>
        </aside>
      )}

      {graphOverlay && (
        <div className="graph-integration-overlay">
          <Suspense fallback={<div className="flex h-full items-center justify-center text-sm" style={{ color: "var(--muted)" }}>グラフ編集画面を読み込んでいます...</div>}>
            <GraphsView
              integration={{
                session: graphOverlay.session,
                initialGraphId: graphOverlay.initialGraphId,
                onComplete: async (result) => {
                  await graphOverlay.onComplete(result);
                  closeGraphOverlay();
                },
                onCancel: () => {
                  graphOverlay.onCancel();
                  closeGraphOverlay();
                },
              }}
            />
          </Suspense>
        </div>
      )}

      {/* 下部: コンパイルログパネル */}
      {lastCompile && (
        <footer
          className="app-footer shrink-0 border-t"
          style={{ borderColor: "var(--border)" }}
        >
          <div
            className="app-footer-summary flex h-7 cursor-pointer items-center gap-2 px-3 text-xs select-none"
            onClick={() => setLogOpen(!logOpen)}
          >
            <span style={{ color: "var(--muted)" }}>{logOpen ? "▾" : "▸"}</span>
            <span className="section-label">LaTeXログ</span>
            <span
              className={`badge ${lastCompile.success ? "badge-basic" : ""}`}
              style={
                lastCompile.success
                  ? undefined
                  : {
                      color: "var(--danger)",
                      borderColor: "rgba(241,106,117,0.4)",
                      background: "var(--danger-dim)",
                    }
              }
            >
              {lastCompile.success ? "✓ 成功" : "✗ 失敗"}
            </span>
            <span className="truncate" style={{ color: "var(--muted)" }}>
              {lastCompile.label} — {lastCompile.message.split("\n")[0]}
            </span>
            <span className="ml-auto flex gap-1.5" onClick={(e) => e.stopPropagation()}>
              {lastCompile.success && lastCompile.pdf_path && (
                <>
                  {isTauri && (
                    <button className="btn btn-ghost btn-sm" onClick={() => showInFolder(lastCompile.pdf_path!)}>
                      フォルダ
                    </button>
                  )}
                  {!isTauri && (
                    <PdfSaveButton
                      className="app-footer-download btn btn-solid btn-sm"
                      path={lastCompile.pdf_path!}
                      cacheKey={lastCompile.download_key}
                      compact
                      onError={(message) => showToast(message, "error")}
                    />
                  )}
                  <button
                    className="btn btn-outline btn-sm"
                    onClick={() => {
                      if (isTauri) {
                        openPath(lastCompile.pdf_path!).catch((e) => showToast(String(e), "error"));
                      } else {
                        setPdfViewer({
                          title: lastCompile.label,
                          url: buildFileUrl(lastCompile.pdf_path!, Date.now()),
                          path: lastCompile.pdf_path!,
                          downloadKey: lastCompile.download_key,
                          zoom: 100,
                        });
                      }
                    }}
                  >
                    {isTauri ? "PDFを開く" : "PDFを表示"}
                  </button>
                </>
              )}
              <button
                className="btn btn-ghost btn-sm"
                onClick={() => {
                  useApp.getState().setLastCompile(null);
                  setLogOpen(false);
                }}
              >
                ✕
              </button>
            </span>
          </div>
          {logOpen && (
            <div className="max-h-56 overflow-y-auto border-t px-3 py-2" style={{ borderColor: "var(--border)" }}>
              <pre className="log-pre">
                {lastCompile.log
                  ? lastCompile.log.split("\n").map((line, i) => (
                      <div key={i} className={line.startsWith("!") || /\.tex:\d+:/.test(line) ? "log-line-error" : ""}>
                        {line || " "}
                      </div>
                    ))
                  : "(ログなし)"}
              </pre>
            </div>
          )}
        </footer>
      )}

      <Toast />
      <ConfirmDialog />
      {pdfViewer && (
        <Modal title={`PDFプレビュー — ${pdfViewer.title}`} onClose={() => setPdfViewer(null)} wide>
          <div className="pdf-preview-toolbar mb-2 flex items-center justify-end gap-1">
            <PdfSaveButton
              className="pdf-download-action btn btn-solid btn-sm mr-auto"
              path={pdfViewer.path}
              cacheKey={pdfViewer.downloadKey}
              compact
              onError={(message) => showToast(message, "error")}
            />
            <button className="btn btn-ghost btn-sm" onClick={() => setPdfViewer((v) => v ? { ...v, zoom: Math.max(50, v.zoom - 10) } : v)}>－</button>
            <button className="btn btn-ghost btn-sm w-14 justify-center" onClick={() => setPdfViewer((v) => v ? { ...v, zoom: 100 } : v)}>{pdfViewer.zoom}%</button>
            <button className="btn btn-ghost btn-sm" onClick={() => setPdfViewer((v) => v ? { ...v, zoom: Math.min(300, v.zoom + 10) } : v)}>＋</button>
          </div>
          <div className="max-h-[68vh] overflow-auto rounded border p-2" style={{ borderColor: "var(--border)" }}>
            <PdfCanvasViewer src={pdfViewer.url} zoom={pdfViewer.zoom} />
          </div>
        </Modal>
      )}

      {setupGuideOpen && (
        <Modal title="初回セットアップ" onClose={() => void finishSetupGuide(false)} wide>
          <SetupGuide
            onDone={() => void finishSetupGuide(false)}
            onOpenSettings={() => void finishSetupGuide(true)}
          />
        </Modal>
      )}

      {/* 初回起動ダイアログ */}
      {welcome && (
        <div className="safe-area-overlay modal-scrim fixed inset-0 z-50 flex items-center justify-center">
          <div
            className="modal-panel w-full max-w-md rounded-md border p-6 shadow-2xl"
            style={{ background: "var(--panel)", borderColor: "var(--border-strong)" }}
          >
            <h2 className="mb-2 text-base font-bold">
              <span className="brand-mark">◆</span> 教材工房へようこそ
            </h2>
            <p className="mb-5 text-sm" style={{ color: "var(--muted)" }}>
              数学のサンプル問題（二次関数・判別式・場合の数など6問）を登録して、すぐに操作を試せるようにしますか？
            </p>
            <div className="flex justify-end gap-2">
              <button onClick={() => onSample(false)} className="btn btn-ghost">
                あとで（設定画面から追加可能）
              </button>
              <button onClick={() => onSample(true)} className="btn btn-solid">
                サンプルを追加
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
