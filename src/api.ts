import { invoke } from "@tauri-apps/api/core";

export type Safety = "safe" | "caution" | "keep" | "system" | "unknown" | "mixed";

export interface ScanMeta {
  scannedAt: number;
  durationMs: number;
  total: number;
  free: number;
  fs: string;
  label: string;
  elevated: boolean;
  denied: number;
  files: number;
  dirs: number;
}

export interface Drive {
  letter: string;
  label: string;
  fs: string;
  kind: string;
  total: number;
  free: number;
  scan: ScanMeta | null;
  rootAlloc: number | null;
}

export interface InitInfo {
  drives: Drive[];
  elevated: boolean;
  busy: string | null;
}

export interface NodeLite {
  drive: string;
  id: number;
  name: string;
  dir: boolean;
  alloc: number;
  size: number;
  count: number;
  newest: number;
  mtime: number;
  flags: number;
  safety: Safety;
  cat: string;
  title: string | null;
  tcat: string;
  path: string | null;
}

export interface Tm {
  id: number;
  name: string;
  dir: boolean;
  alloc: number;
  count: number;
  newest: number;
  flags: number;
  safety: Safety;
  cat: string;
  title: string | null;
  tcat: string;
  other: number;
  children?: Tm[];
}

export interface ExtView {
  ext: string;
  name: string;
  cat: string;
  safety: Safety;
  what: string;
}

export interface Info {
  rule: string | null;
  title: string;
  what: string;
  why: string;
  safety: Safety;
  cat: string;
  clean: string | null;
  note: string;
  inherited: boolean;
  matchedId: number | null;
  matchedPath: string | null;
  apps: string[];
  admin: boolean;
  partial: boolean;
  ext: ExtView | null;
  source: string;
}

export interface Share {
  key: string;
  label: string;
  alloc: number;
  count: number;
}

export interface Detail {
  node: NodeLite;
  path: string;
  info: Info;
  crumbs: [number, string][];
  types: Share[];
  exts: Share[];
  largest: NodeLite[];
  running: string[];
  blocked: string | null;
  hiddenFiles: number;
}

export interface ExtRow {
  ext: string;
  alloc: number;
  count: number;
  name: string;
  cat: string;
  safety: Safety;
  what: string;
}

export interface CleanEntry {
  drive: string;
  id: number;
  path: string;
  alloc: number;
  count: number;
  newest: number;
}

export interface CleanGroup {
  rule: string;
  drive: string;
  title: string;
  what: string;
  why: string;
  note: string;
  safety: Safety;
  cat: string;
  clean: string | null;
  admin: boolean;
  partial: boolean;
  running: string[];
  total: number;
  files: number;
  nEntries: number;
  entries: CleanEntry[];
}

export interface Leftover {
  drive: string;
  id: number;
  name: string;
  path: string;
  location: string;
  alloc: number;
  count: number;
  newest: number;
  confidence: "likely" | "possible" | "active";
}

export interface DupGroup {
  size: number;
  wasted: number;
  files: NodeLite[];
}

export interface CleanResult {
  path: string;
  mode: string;
  freed: number;
  moved: number;
  deletedFiles: number;
  skipped: number;
  error: string | null;
}

export interface AiAnswer {
  model: string;
  what: string;
  important: string;
  verdict: string;
  reason: string;
}

export interface FileQuery {
  drive?: string | null;
  ext?: string | null;
  cat?: string | null;
  minSize: number;
  olderThanDays: number;
  query?: string | null;
  dirs: boolean;
  limit: number;
}

export type DeleteMode = "recycle" | "permanent" | "contents";

export interface Target {
  drive: string;
  id: number;
  mode: DeleteMode;
}

export const api = {
  init: () => invoke<InitInfo>("init"),
  drives: () => invoke<Drive[]>("drives"),
  startScan: (drives: string[], gentle: boolean) => invoke<void>("start_scan", { drives, gentle }),
  cancel: () => invoke<void>("cancel"),
  treemap: (drive: string, id: number, depth: number, byType: boolean) => invoke<Tm>("treemap", { drive, id, depth, byType }),
  children: (drive: string, id: number) => invoke<NodeLite[]>("children", { drive, id }),
  detail: (drive: string, id: number) => invoke<Detail>("detail", { drive, id }),
  locate: (path: string) => invoke<[string, number] | null>("locate", { path }),
  extSummary: (drive: string | null) => invoke<ExtRow[]>("ext_summary", { drive }),
  findFiles: (q: FileQuery) => invoke<NodeLite[]>("find_files", { q }),
  cleanupReport: () => invoke<CleanGroup[]>("cleanup_report"),
  leftovers: () => invoke<Leftover[]>("leftovers"),
  startDupes: (minSize: number, personalOnly: boolean) => invoke<void>("start_dupes", { minSize, personalOnly }),
  clean: (targets: Target[]) => invoke<void>("clean", { targets }),
  runAction: (action: string, drive?: string) => invoke<string>("run_action", { action, drive: drive ?? null }),
  rescan: (drive: string, id: number) => invoke<void>("rescan", { drive, id }),
  openPath: (path: string, select = true) => invoke<void>("open_path", { path, select }),
  relaunchAdmin: () => invoke<void>("relaunch_admin"),
  askAi: (drive: string, id: number) => invoke<AiAnswer>("ask_ai", { drive, id }),
};

export const F_DIR = 1,
  F_HIDDEN = 2,
  F_SYSTEM = 4,
  F_REPARSE = 8,
  F_DENIED = 16,
  F_SYNTH = 64,
  F_HARDLINK = 128,
  F_CLOUD = 256;
