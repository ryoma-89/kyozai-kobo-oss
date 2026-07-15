import { lazy, Suspense, useEffect, useState } from "react";
import { createSampleData, hasAnyData, openPath, showInFolder } from "./api";
import { AiJobsView } from "./components/AiJobsView";
import { BankView } from "./components/BankView";
import { PairingScreen } from "./components/PairingScreen";
import { PdfCanvasViewer } from "./components/PdfCanvasViewer";
import { PartsView } from "./components/PartsView";
import { ProjectsView } from "./components/ProjectsView";
import { SearchView } from "./components/SearchView";
import { SettingsView } from "./components/SettingsView";
import { TemplatesView } from "./components/TemplatesView";
import { ConfirmDialog, Modal, Toast } from "./components/ui";
import { Icon } from "./components/Icon";
import { useApp, type View } from "./store";
import { authLogout, authMe, buildFileUrl, isTauri, subscribeEvents } from "./transport";

const NAV: { view: View; label: string; icon: string }[] = [
  { view: "bank", label: "問題バンク", icon: "▦" },
  { view: "projects", label: "教材", icon: "▤" },
  { view: "parts", label: "部品", icon: "▧" },
  { view: "templates", label: "テンプレート", icon: "❖" },
  { view: "graphs", label: "グラフ", icon: "⌁" },
  { view: "ai", label: "AI変換", icon: "" },
];

const GraphsView = lazy(() => import("./components/GraphsView").then((module) => ({ default: module.GraphsView })));

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
    connected,
    setConnected,
    confirm,
    graphOverlay,
    closeGraphOverlay,
  } = useApp();
  const [welcome, setWelcome] = useState(false);
  // Web版の認証状態: null=確認中
  const [authed, setAuthed] = useState<boolean | null>(isTauri ? true : null);
  const [authUnavailable, setAuthUnavailable] = useState(false);
  const [pdfViewer, setPdfViewer] = useState<{ title: string; url: string; zoom: number } | null>(null);

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

  // 初回起動時: データが空ならサンプルデータ作成を提案（デスクトップのみ）
  useEffect(() => {
    if (!isTauri || !authed) return;
    hasAnyData()
      .then((has) => {
        if (!has) setWelcome(true);
      })
      .catch(() => {});
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
        style={{ background: "var(--panel)", borderColor: "var(--border)" }}
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
          style={{ background: "var(--panel)", borderColor: "var(--border)" }}
        >
          {NAV.map((n) => (
            <button
              key={n.view}
              onClick={() => void navigate(n.view)}
              className="flex w-[56px] flex-col items-center rounded py-1.5 text-[9.5px] transition-colors"
              style={
                view === n.view
                  ? {
                      background: "var(--accent-dim)",
                      color: "var(--accent)",
                      border: "1px solid rgba(157,108,242,0.42)",
                      fontWeight: 700,
                    }
                  : { color: "var(--muted)", border: "1px solid transparent" }
              }
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
      </div>

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
          style={{ background: "var(--panel)", borderColor: "var(--border)" }}
        >
          <div
            className="flex h-7 cursor-pointer items-center gap-2 px-3 text-xs select-none"
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
                  <button
                    className="btn btn-outline btn-sm"
                    onClick={() => {
                      if (isTauri) {
                        openPath(lastCompile.pdf_path!).catch((e) => showToast(String(e), "error"));
                      } else {
                        setPdfViewer({
                          title: lastCompile.label,
                          url: buildFileUrl(lastCompile.pdf_path!, Date.now()),
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
          <div className="mb-2 flex items-center justify-end gap-1">
            <button className="btn btn-ghost btn-sm" onClick={() => setPdfViewer((v) => v ? { ...v, zoom: Math.max(50, v.zoom - 10) } : v)}>－</button>
            <button className="btn btn-ghost btn-sm w-14 justify-center" onClick={() => setPdfViewer((v) => v ? { ...v, zoom: 100 } : v)}>{pdfViewer.zoom}%</button>
            <button className="btn btn-ghost btn-sm" onClick={() => setPdfViewer((v) => v ? { ...v, zoom: Math.min(300, v.zoom + 10) } : v)}>＋</button>
          </div>
          <div className="max-h-[68vh] overflow-auto rounded border p-2" style={{ borderColor: "var(--border)" }}>
            <PdfCanvasViewer src={pdfViewer.url} zoom={pdfViewer.zoom} />
          </div>
        </Modal>
      )}

      {/* 初回起動ダイアログ */}
      {welcome && (
        <div className="safe-area-overlay fixed inset-0 z-50 flex items-center justify-center bg-black/60">
          <div
            className="fade-in w-full max-w-md rounded-md border p-6 shadow-2xl"
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
