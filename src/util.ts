import type { Safety } from "./api";

export function fmtBytes(b: number, digits = 1): string {
  if (!isFinite(b) || b <= 0) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = b;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${b} B` : `${v.toFixed(v >= 100 ? 0 : digits)} ${u[i]}`;
}

export function fmtNum(n: number): string {
  return n.toLocaleString("en-US");
}

export function fmtAge(unixSecs: number): string {
  if (!unixSecs) return "—";
  const s = Date.now() / 1000 - unixSecs;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86400);
  if (d < 31) return `${d} day${d === 1 ? "" : "s"} ago`;
  if (d < 365) return `${Math.floor(d / 30)} month${Math.floor(d / 30) === 1 ? "" : "s"} ago`;
  const y = d / 365;
  return `${y < 2 ? "1" : Math.floor(y)} year${y < 2 ? "" : "s"} ago`;
}

export function fmtDate(unixSecs: number): string {
  if (!unixSecs) return "—";
  return new Date(unixSecs * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export const SAFETY: Record<Safety, { label: string; short: string; color: string; verdict: string }> = {
  safe: { label: "Safe to delete", short: "Safe", color: "var(--safe)", verdict: "Safe to delete — nothing is lost" },
  caution: { label: "Delete with caution", short: "Caution", color: "var(--caution)", verdict: "Can be deleted — but read this first" },
  keep: { label: "Keep", short: "Keep", color: "var(--keep)", verdict: "Keep — your data or an installed app" },
  system: { label: "Don't touch", short: "System", color: "var(--system)", verdict: "Don't delete — Windows or a program needs this" },
  unknown: { label: "Unrecognised", short: "Unknown", color: "var(--unknown)", verdict: "Not recognised" },
  mixed: { label: "Mixed", short: "Mixed", color: "var(--mixed)", verdict: "" },
};

export const TYPE_COLORS: Record<string, string> = {
  video: "#e8590c",
  image: "#f59f00",
  audio: "#d6336c",
  document: "#4dabf7",
  design: "#cc5de8",
  model3d: "#9775fa",
  archive: "#a9e34b",
  diskimage: "#74b816",
  installer: "#66d9a0",
  program: "#748ffc",
  system: "#5c5f99",
  code: "#22b8cf",
  data: "#3bc9db",
  database: "#1098ad",
  config: "#868e96",
  log: "#adb5bd",
  temp: "#ffd43b",
  backup: "#ffa94d",
  font: "#e599f7",
  ai: "#ff6b6b",
  vm: "#20c997",
  game: "#94d82d",
  email: "#91a7ff",
  other: "#5c6575",
  folder: "#465063",
  mixed: "#363d4b",
};

export const TYPE_LABELS: Record<string, string> = {
  video: "Video",
  image: "Images",
  audio: "Audio",
  document: "Documents",
  design: "Design projects",
  model3d: "3D",
  archive: "Archives",
  diskimage: "Disk images",
  installer: "Installers",
  program: "Programs",
  system: "System files",
  code: "Code",
  data: "Data",
  database: "Databases",
  config: "Config",
  log: "Logs",
  temp: "Temp / cache",
  backup: "Backups",
  font: "Fonts",
  ai: "AI models",
  vm: "Virtual disks",
  game: "Game data",
  email: "Email",
  other: "Other",
  folder: "Folder",
  mixed: "Small items",
};

export const CAT_LABELS: Record<string, string> = {
  cache: "Cache",
  temp: "Temporary",
  logs: "Logs",
  crash: "Crash dumps",
  system: "System",
  app: "App",
  userdata: "Your files",
  dev: "Developer",
  games: "Games",
  media: "Media",
  backup: "Backup",
  installer: "Installer",
  ai: "AI models",
  vm: "Virtual machine",
  browser: "Browser",
  update: "Windows Update",
  trash: "Deleted files",
  archive: "Archive",
  unknown: "Unknown",
};

export const ACTION_LABELS: Record<string, string> = {
  "empty-recycle-bin": "Empty Recycle Bin",
  "dism-cleanup": "Run Windows Update clean-up",
  "hibernate-off": "Turn off hibernation",
  cleanmgr: "Open Disk Cleanup",
  "restore-points": "Manage restore points",
  pagefile: "Virtual memory settings",
  apps: "Open Installed apps",
  steam: "Open Steam library",
  "storage-sense": "Open Storage Sense",
};

export function cleanToMode(clean: string | null): "recycle" | "permanent" | "contents" | null {
  if (clean === "contents") return "contents";
  if (clean === "delete") return "permanent";
  if (clean === "recycle") return "recycle";
  return null;
}

export function actionOf(clean: string | null): string | null {
  return clean && clean.startsWith("action:") ? clean.slice(7) : null;
}

export function cls(...xs: (string | false | null | undefined)[]): string {
  return xs.filter(Boolean).join(" ");
}
