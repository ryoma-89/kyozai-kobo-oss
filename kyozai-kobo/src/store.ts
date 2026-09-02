import { create } from "zustand";
import type { BankNode, CompleteGraphWebSessionResult, CompileResult, GraphWebSession, SubjectNode } from "./types";
import { getBankTree, getTree } from "./api";

export type View = "bank" | "patterns" | "search" | "projects" | "parts" | "templates" | "graphs" | "ai" | "settings";

/** 他端末からの変更イベントで増える更新カウンター（各画面が変化を監視して再読込する） */
export interface RemoteBumps {
  problems: number;
  patterns: number;
  projects: number;
  parts: number;
  templates: number;
  graphs: number;
  settings: number;
  ai_job: number;
  ai_chat: number;
  codex: number;
  server: number;
}

export interface LastCompile extends CompileResult {
  label: string;
  download_key: number;
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

export interface ProjectReviewFix {
  projectId: number;
  itemId: number;
  field: "statement" | "answer" | "explanation" | "content" | "item";
  guidance: string;
  action: "manual" | "ai";
}

interface AppStore {
  view: View;
  setView: (v: View) => void;

  tree: SubjectNode[];
  bankTree: BankNode[];
  refreshTree: () => Promise<void>;

  selectedBankNodeId: number | null;
  selectBankNode: (id: number | null) => void;
  /** Parts・旧AIコンテキスト用の互換Unit ID。 */
  selectedUnitId: number | null;
  selectUnit: (id: number | null) => void;
  selectedProblemId: number | null;
  selectProblem: (id: number | null) => void;
  selectedPatternId: number | null;
  selectPattern: (id: number | null) => void;
  selectedProjectId: number | null;
  selectProject: (id: number | null) => void;
  pendingProjectReviewFix: ProjectReviewFix | null;
  openProjectReviewFix: (fix: ProjectReviewFix) => void;
  clearProjectReviewFix: () => void;

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
  openProblemInBank: (bankNodeId: number, problemId: number) => void;
  /** 定石ライブラリへ移動して定石を開く */
  openPattern: (patternId: number) => void;

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

function findBankNode(nodes: BankNode[], predicate: (node: BankNode) => boolean): BankNode | null {
  for (const node of nodes) {
    if (predicate(node)) return node;
    const nested = findBankNode(node.children, predicate);
    if (nested) return nested;
  }
  return null;
}

export const useApp = create<AppStore>((set, get) => ({
  view: "bank",
  setView: (v) => set({ view: v }),

  tree: [],
  bankTree: [],
  refreshTree: async () => {
    try {
      const [tree, bankTree] = await Promise.all([getTree(), getBankTree()]);
      set({ tree, bankTree });
    } catch (e) {
      get().showToast(String(e), "error");
    }
  },

  selectedBankNodeId: null,
  selectBankNode: (id) => {
    const node = id == null ? null : findBankNode(get().bankTree, (candidate) => candidate.id === id);
    set({
      selectedBankNodeId: id,
      selectedUnitId: node?.legacy_unit_id ?? null,
      selectedProblemId: null,
    });
  },
  selectedUnitId: null,
  selectUnit: (id) => {
    const node = id == null ? null : findBankNode(get().bankTree, (candidate) => candidate.legacy_unit_id === id);
    set({ selectedUnitId: id, selectedBankNodeId: node?.id ?? null, selectedProblemId: null });
  },
  selectedProblemId: null,
  selectProblem: (id) => set({ selectedProblemId: id }),
  selectedPatternId: null,
  selectPattern: (id) => set({ selectedPatternId: id }),
  selectedProjectId: null,
  selectProject: (id) => set({ selectedProjectId: id }),
  pendingProjectReviewFix: null,
  openProjectReviewFix: (fix) =>
    set({
      view: "projects",
      selectedProjectId: fix.projectId,
      pendingProjectReviewFix: fix,
    }),
  clearProjectReviewFix: () => set({ pendingProjectReviewFix: null }),

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

  openProblemInBank: (bankNodeId, problemId) => {
    const node = findBankNode(get().bankTree, (candidate) => candidate.id === bankNodeId);
    set({
      view: "bank",
      selectedBankNodeId: bankNodeId,
      selectedUnitId: node?.legacy_unit_id ?? null,
      selectedProblemId: problemId,
    });
  },
  openPattern: (patternId) => set({ view: "patterns", selectedPatternId: patternId }),

  lastCompile: null,
  setLastCompile: (r) => set({ lastCompile: r }),
  logOpen: false,
  setLogOpen: (open) => set({ logOpen: open }),

  contextName: "",
  setContextName: (name) => set({ contextName: name }),

  bumps: {
    problems: 0,
    patterns: 0,
    projects: 0,
    parts: 0,
    templates: 0,
    graphs: 0,
    settings: 0,
    ai_job: 0,
    ai_chat: 0,
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
