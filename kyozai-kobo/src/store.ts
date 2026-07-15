import { create } from "zustand";
import type { CompleteGraphWebSessionResult, CompileResult, GraphWebSession, SubjectNode } from "./types";
import { getTree } from "./api";

export type View = "bank" | "search" | "projects" | "parts" | "templates" | "graphs" | "ai" | "settings";

/** 他端末からの変更イベントで増える更新カウンター（各画面が変化を監視して再読込する） */
export interface RemoteBumps {
  problems: number;
  projects: number;
  parts: number;
  templates: number;
  graphs: number;
  settings: number;
  ai_job: number;
  codex: number;
  server: number;
}

export interface LastCompile extends CompileResult {
  label: string;
}

interface ConfirmState {
  message: string;
  resolve: (ok: boolean) => void;
}

export interface GraphOverlayState {
  session: GraphWebSession;
  initialGraphId?: string;
  onComplete: (result: CompleteGraphWebSessionResult) => void | Promise<void>;
  onCancel: () => void;
}

interface AppStore {
  view: View;
  setView: (v: View) => void;

  tree: SubjectNode[];
  refreshTree: () => Promise<void>;

  selectedUnitId: number | null;
  selectUnit: (id: number | null) => void;
  selectedProblemId: number | null;
  selectProblem: (id: number | null) => void;
  selectedProjectId: number | null;
  selectProject: (id: number | null) => void;

  /** 保存されていない変更があるか（問題編集画面） */
  dirty: boolean;
  setDirty: (d: boolean) => void;

  toast: string | null;
  toastKind: "info" | "error";
  showToast: (msg: string, kind?: "info" | "error") => void;

  confirmState: ConfirmState | null;
  confirm: (message: string) => Promise<boolean>;
  resolveConfirm: (ok: boolean) => void;

  /** 問題バンクの編集画面へ移動して問題を開く */
  openProblemInBank: (unitId: number, problemId: number) => void;

  /** 直近のLaTeXコンパイル結果（下部ログパネルに表示） */
  lastCompile: LastCompile | null;
  setLastCompile: (r: LastCompile | null) => void;
  logOpen: boolean;
  setLogOpen: (open: boolean) => void;

  /** トップバーに表示する現在のコンテキスト名 */
  contextName: string;
  setContextName: (name: string) => void;

  /** 他端末からの変更通知（SSE / Tauriイベント）による更新カウンター */
  bumps: RemoteBumps;
  bump: (kind: string) => void;

  /** サーバー接続状態（Web版のみ意味を持つ。デスクトップは常にtrue） */
  connected: boolean;
  setConnected: (c: boolean) => void;

  graphOverlay: GraphOverlayState | null;
  openGraphOverlay: (
    session: GraphWebSession,
    onComplete: GraphOverlayState["onComplete"],
    onCancel: GraphOverlayState["onCancel"],
    initialGraphId?: string,
  ) => void;
  closeGraphOverlay: () => void;
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export const useApp = create<AppStore>((set, get) => ({
  view: "bank",
  setView: (v) => set({ view: v }),

  tree: [],
  refreshTree: async () => {
    try {
      const tree = await getTree();
      set({ tree });
    } catch (e) {
      get().showToast(String(e), "error");
    }
  },

  selectedUnitId: null,
  selectUnit: (id) => set({ selectedUnitId: id, selectedProblemId: null }),
  selectedProblemId: null,
  selectProblem: (id) => set({ selectedProblemId: id }),
  selectedProjectId: null,
  selectProject: (id) => set({ selectedProjectId: id }),

  dirty: false,
  setDirty: (d) => set({ dirty: d }),

  toast: null,
  toastKind: "info",
  showToast: (msg, kind = "info") => {
    if (toastTimer) clearTimeout(toastTimer);
    set({ toast: msg, toastKind: kind });
    toastTimer = setTimeout(() => set({ toast: null }), kind === "error" ? 6000 : 2500);
  },

  confirmState: null,
  confirm: (message) =>
    new Promise<boolean>((resolve) => {
      set({ confirmState: { message, resolve } });
    }),
  resolveConfirm: (ok) => {
    const st = get().confirmState;
    if (st) st.resolve(ok);
    set({ confirmState: null });
  },

  openProblemInBank: (unitId, problemId) =>
    set({
      view: "bank",
      selectedUnitId: unitId,
      selectedProblemId: problemId,
    }),

  lastCompile: null,
  setLastCompile: (r) => set({ lastCompile: r }),
  logOpen: false,
  setLogOpen: (open) => set({ logOpen: open }),

  contextName: "",
  setContextName: (name) => set({ contextName: name }),

  bumps: {
    problems: 0,
    projects: 0,
    parts: 0,
    templates: 0,
    graphs: 0,
    settings: 0,
    ai_job: 0,
    codex: 0,
    server: 0,
  },
  bump: (kind) =>
    set((s) => {
      if (!(kind in s.bumps)) return s;
      return { bumps: { ...s.bumps, [kind]: s.bumps[kind as keyof RemoteBumps] + 1 } };
    }),

  connected: true,
  setConnected: (c) => set({ connected: c }),

  graphOverlay: null,
  openGraphOverlay: (session, onComplete, onCancel, initialGraphId) =>
    set({ graphOverlay: { session, onComplete, onCancel, initialGraphId } }),
  closeGraphOverlay: () => set({ graphOverlay: null }),
}));
