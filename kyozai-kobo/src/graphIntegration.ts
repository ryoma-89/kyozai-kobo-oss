import { pollGraphIntegration } from "./api";
import type { GraphIntegrationPoll, GraphIntegrationSession } from "./types";

const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

export function insertTextAtRange(value: string, text: string, start: number, end: number): string {
  const from = Math.max(0, Math.min(start, value.length));
  const to = Math.max(from, Math.min(end, value.length));
  return value.slice(0, from) + text + value.slice(to);
}

export async function waitForGraphIntegration(
  session: GraphIntegrationSession,
  onPoll?: (poll: GraphIntegrationPoll) => void,
): Promise<GraphIntegrationPoll> {
  const started = Date.now();
  while (Date.now() - started < 10 * 60 * 1000) {
    const poll = await pollGraphIntegration(session.requestId, session.requestPath);
    onPoll?.(poll);
    if (poll.status !== "pending") return poll;
    await sleep(1000);
  }
  throw new Error("Graph integration timed out.");
}
