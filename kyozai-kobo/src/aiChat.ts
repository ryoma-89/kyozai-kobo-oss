export type AiChatTargetKind = "problem" | "part" | "material";

export interface AiChatLaunchTarget {
  requestId: string;
  kind: AiChatTargetKind;
  id: number;
  title: string;
  currentScreen: "bank" | "parts" | "projects";
  starter: string;
}

export function openAiChatForTarget(
  target: Omit<AiChatLaunchTarget, "requestId" | "starter"> & { starter?: string },
) {
  const label = target.kind === "problem" ? "問題" : target.kind === "part" ? "部品" : "教材";
  const detail: AiChatLaunchTarget = {
    ...target,
    requestId: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    starter: target.starter ?? `この${label}「${target.title}」について、`,
  };
  window.dispatchEvent(new CustomEvent<AiChatLaunchTarget>("kk-open-ai-chat", { detail }));
}
