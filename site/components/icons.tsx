import type { SVGProps } from "react";

type P = SVGProps<SVGSVGElement>;

/** 品牌/平台（填充） */
export function GitHubIcon(props: P) {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true" {...props}>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}
export function WindowsIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...props}>
      <path d="M0 3.36L9.75 2v9.4H0V3.36zM10.95 1.83L24 0v11.4H10.95V1.83zM0 12.6h9.75V22L0 20.64V12.6zm10.95 0H24V24l-13.05-1.83V12.6z" />
    </svg>
  );
}
export function AppleIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...props}>
      <path d="M17.05 12.54c-.02-2.14 1.75-3.17 1.83-3.22-1-1.46-2.55-1.66-3.1-1.68-1.32-.13-2.58.78-3.25.78-.67 0-1.7-.76-2.8-.74-1.44.02-2.77.84-3.51 2.13-1.5 2.6-.38 6.44 1.07 8.55.71 1.03 1.55 2.19 2.66 2.15 1.07-.04 1.47-.69 2.76-.69 1.29 0 1.65.69 2.78.67 1.15-.02 1.87-1.05 2.57-2.09.81-1.2 1.14-2.36 1.16-2.42-.03-.01-2.22-.85-2.24-3.37zM14.9 5.62c.59-.72.99-1.71.88-2.7-.85.03-1.88.57-2.49 1.28-.55.63-1.03 1.64-.9 2.61.95.07 1.92-.48 2.51-1.19z" />
    </svg>
  );
}

/** UI（线性） */
const line = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export function DownloadIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M12 3v12m0 0l4-4m-4 4l-4-4M4 17v2a2 2 0 002 2h12a2 2 0 002-2v-2" />
    </svg>
  );
}
export function CheckIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} strokeWidth={2.2} aria-hidden="true" {...props}>
      <path d="M5 12.5l4.5 4.5L19 7" />
    </svg>
  );
}
export function PlusIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} strokeWidth={2} aria-hidden="true" {...props}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}
export function MenuIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M4 7h16M4 12h16M4 17h16" />
    </svg>
  );
}
export function ArrowRightIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M5 12h14m0 0l-6-6m6 6l-6 6" />
    </svg>
  );
}

/** 特性图标（线性） */
export function BoardIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M3 9h18M9 9v11" />
    </svg>
  );
}
export function BellIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M18 8a6 6 0 10-12 0c0 7-3 8-3 8h18s-3-1-3-8" />
      <path d="M10.5 21a1.7 1.7 0 003 0" />
    </svg>
  );
}
export function TerminalIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M7 9l3 3-3 3M13 15h4" />
    </svg>
  );
}
export function CardsIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <rect x="3" y="7" width="14" height="12" rx="2" />
      <path d="M7 4h11a2 2 0 012 2v9" />
    </svg>
  );
}
export function ChartIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M4 20V10M10 20V4M16 20v-7M22 20H2" />
    </svg>
  );
}
export function PlugIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 01-12 0V8zM12 17v4" />
    </svg>
  );
}
export function NetworkIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3c2.5 2.5 3.8 5.5 3.8 9S14.5 18.5 12 21M12 3C9.5 5.5 8.2 8.5 8.2 12S9.5 18.5 12 21" />
    </svg>
  );
}
export function ShieldIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M12 3l7 3v5c0 4.6-2.8 8.1-7 10-4.2-1.9-7-5.4-7-10V6l7-3z" />
      <path d="M9 12l2 2 4-4" />
    </svg>
  );
}
export function InfoIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5M12 8h.01" />
    </svg>
  );
}
export function UsersIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75" />
    </svg>
  );
}
export function PaletteIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <path d="M12 2a10 10 0 0 0 0 20c1.1 0 2-.9 2-2 0-.52-.2-.99-.53-1.34-.32-.35-.52-.82-.52-1.33 0-1.1.9-2 2-2h2.35A4.35 4.35 0 0 0 22 11c0-4.97-4.48-9-10-9z" />
      <circle cx="7.5" cy="11.5" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="9.5" cy="7.5" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="14.5" cy="7.5" r="1.1" fill="currentColor" stroke="none" />
    </svg>
  );
}
export function TrafficIcon(props: P) {
  return (
    <svg viewBox="0 0 24 24" {...line} aria-hidden="true" {...props}>
      <rect x="8" y="2" width="8" height="20" rx="4" />
      <circle cx="12" cy="7" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="17" r="1.2" fill="currentColor" stroke="none" />
    </svg>
  );
}
