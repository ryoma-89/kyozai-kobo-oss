import type { ReactNode } from "react";
import katex from "katex";

function MathFragment({ source, display = false }: { source: string; display?: boolean }) {
  try {
    const html = katex.renderToString(source, {
      displayMode: display,
      throwOnError: false,
      strict: "warn",
      trust: false,
      output: "htmlAndMathml",
    });
    return <span className={display ? "ai-chat-math-block" : "ai-chat-math"} dangerouslySetInnerHTML={{ __html: html }} />;
  } catch {
    return <code>{source}</code>;
  }
}

function inline(text: string): ReactNode[] {
  const token = /(\$[^$\n]+\$|`[^`\n]+`|\*\*[^*\n]+\*\*|\[[^\]\n]+\]\(https?:\/\/[^)\s]+\))/g;
  const parts = text.split(token);
  return parts.filter(Boolean).map((part, index) => {
    if (part.startsWith("$") && part.endsWith("$")) {
      return <MathFragment key={index} source={part.slice(1, -1)} />;
    }
    if (part.startsWith("`") && part.endsWith("`")) {
      return <code key={index}>{part.slice(1, -1)}</code>;
    }
    if (part.startsWith("**") && part.endsWith("**")) {
      return <strong key={index}>{part.slice(2, -2)}</strong>;
    }
    const link = part.match(/^\[([^\]]+)\]\((https?:\/\/[^)]+)\)$/);
    if (link) {
      return <a key={index} href={link[2]} target="_blank" rel="noreferrer">{link[1]}</a>;
    }
    return part;
  });
}

export function ChatMarkdown({ children }: { children: string }) {
  const lines = children.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let code: string[] | null = null;
  let math: string[] | null = null;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim().startsWith("```")) {
      if (code) {
        blocks.push(<pre key={`code-${index}`}><code>{code.join("\n")}</code></pre>);
        code = null;
      } else {
        code = [];
      }
      continue;
    }
    if (code) {
      code.push(line);
      continue;
    }
    if (line.trim() === "$$") {
      if (math) {
        blocks.push(<MathFragment key={`math-${index}`} source={math.join("\n")} display />);
        math = null;
      } else {
        math = [];
      }
      continue;
    }
    if (math) {
      math.push(line);
      continue;
    }
    if (!line.trim()) {
      blocks.push(<div key={`space-${index}`} className="h-2" />);
      continue;
    }
    const heading = line.match(/^(#{1,3})\s+(.+)$/);
    if (heading) {
      blocks.push(<div key={index} className={`ai-chat-heading ai-chat-heading-${heading[1].length}`}>{inline(heading[2])}</div>);
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.+)$/);
    if (bullet) {
      blocks.push(<div key={index} className="ai-chat-bullet"><span>•</span><span>{inline(bullet[1])}</span></div>);
      continue;
    }
    const ordered = line.match(/^\s*(\d+)\.\s+(.+)$/);
    if (ordered) {
      blocks.push(<div key={index} className="ai-chat-bullet"><span>{ordered[1]}.</span><span>{inline(ordered[2])}</span></div>);
      continue;
    }
    blocks.push(<div key={index} className="ai-chat-paragraph">{inline(line)}</div>);
  }
  if (code) blocks.push(<pre key="code-tail"><code>{code.join("\n")}</code></pre>);
  if (math) blocks.push(<MathFragment key="math-tail" source={math.join("\n")} display />);
  return <div className="ai-chat-markdown">{blocks}</div>;
}
