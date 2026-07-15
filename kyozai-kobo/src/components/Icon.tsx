import type { ReactNode } from "react";

export type IconName = "search" | "settings" | "warning" | "play" | "sparkle";

const PATHS: Record<IconName, ReactNode> = {
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.25" />
      <path d="m15.2 15.2 4.55 4.55" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 2.8v2M12 19.2v2M2.8 12h2M19.2 12h2M5.5 5.5l1.4 1.4M17.1 17.1l1.4 1.4M18.5 5.5l-1.4 1.4M6.9 17.1l-1.4 1.4" />
      <circle cx="12" cy="12" r="7.2" />
    </>
  ),
  warning: (
    <>
      <path d="M12 3.2 21 19H3L12 3.2Z" />
      <path d="M12 8.5v5.2M12 17.1h.01" />
    </>
  ),
  play: <path d="m8.2 5.2 10 6.8-10 6.8V5.2Z" />,
  sparkle: (
    <>
      <path d="M12 2.8c.5 4.8 2.4 6.7 7.2 7.2-4.8.5-6.7 2.4-7.2 7.2-.5-4.8-2.4-6.7-7.2-7.2 4.8-.5 6.7-2.4 7.2-7.2Z" />
      <path d="M19 16.2c.2 2 1 2.8 3 3-2 .2-2.8 1-3 3-.2-2-1-2.8-3-3 2-.2 2.8-1 3-3Z" />
    </>
  ),
};

export function Icon({ name, size = 16, className = "" }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      aria-hidden="true"
      className={`ui-icon ${className}`}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {PATHS[name]}
    </svg>
  );
}
