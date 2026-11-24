import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type CleanResult, type DeleteMode, type InitInfo, type Safety } from "./api";
import { fmtBytes, fmtNum, ACTION_LABELS } from "./util";
import Header from "./components/Header";
import ScanOverlay from "./components/ScanOverlay";
import ExploreView from "./components/ExploreView";
import CleanupView from "./components/CleanupView";
import TypesView from "./components/TypesView";
import FilesView from "./components/FilesView";
import LeftoversView from "./components/LeftoversView";
import DupesView from "./components/DupesView";
import ConfirmDelete, { type DelItem } from "./components/ConfirmDelete";
import Welcome from "./components/Welcome";

export type Tab = "explore" | "cleanup" | "types" | "files" | "leftovers" | "dupes";

interface Toast {
  id: number;
  text: string;
  kind: "ok" | "err" | "info";
}

interface AppCtx {
  info: InitInfo | null;
  version: number;
  drive: string | null;
  setDrive: (d: string) => void;
  nav: { drive: string; root: number; select: number | null; seq: number } | null;
  goTo: (drive: string, id: number, opts?: { open?: boolean }) => Promise<void>;
  requestDelete: (items: DelItem[], preferred?: DeleteMode) => void;
  runAction: (action: string, drive?: string) => Promise<void>;
  toast: (text: string, kind?: Toast["kind"]) => void;
  busy: string | null;
  refresh: () => Promise<void>;
}

const Ctx = createContext<AppCtx>(null as unknown as AppCtx);
export const useApp = () => useContext(Ctx);

const TABS: { id: Tab; label: string; hint: string }[] = [
  { id: "explore", label: "Explore", hint: "See what fills your drives" },
  { id: "cleanup", label: "Clean up", hint: "Recommended things to remove" },
  { id: "types", label: "File types", hint: "Space by extension" },
  { id: "files", label: "Big & old files", hint: "Largest and forgotten files, search" },
  { id: "leftovers", label: "Leftovers", hint: "Data from uninstalled apps" },
  { id: "dupes", label: "Duplicates", hint: "Identical copies of files" },
];

export default function App() {
  const [info, setInfo] = useState<InitInfo | null>(null);
  const [tab, setTab] = useState<Tab>("explore");
  const [version, setVersion] = useState(0);
  const [drive, setDrive] = useState<string | null>(null);
  const [nav, setNav] = useState<AppCtx["nav"]>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{ items: DelItem[]; preferred?: DeleteMode } | null>(null);
  const [cleanProg, setCleanProg] = useState<{ index: number; of: number; files: number; current: string } | null>(null);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, text, kind }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "err" ? 9000 : 6000);
  }, []);

  const refresh = useCallback(async () => {
    const i = await api.init();
    setInfo(i);
    setBusy(i.busy);
    setDrive((d) => d ?? i.drives.find((x) => x.scan)?.letter ?? i.drives[0]?.letter ?? null);
  }, []);

  useEffect(() => {
    refresh().catch((e) => toast(String(e), "err"));
  }, [refresh, toast]);

  useEffect(() => {
    const un = [
      listen<boolean>("scan-done", async (e) => {
        setScanning(false);
        setBusy(null);
        await refresh();
        setVersion((v) => v + 1);
        toast(e.payload ? "Scan cancelled — previous results kept." : "Scan complete.", e.payload ? "info" : "ok");
      }),
      listen<{ index: number; of: number; files: number; current: string }>("clean-progress", (e) => setCleanProg(e.payload)),
      listen<CleanResult[]>("clean-done", async (e) => {
        setCleanProg(null);
        setBusy(null);
        const r = e.payload;
        const freed = r.reduce((a, x) => a + x.freed, 0);
        const moved = r.reduce((a, x) => a + x.moved, 0);
        const skipped = r.reduce((a, x) => a + x.skipped, 0);
        const errs = r.filter((x) => x.error && x.freed === 0 && x.moved === 0);
        const parts: string[] = [];
        if (freed > 0) parts.push(`Freed ${fmtBytes(freed)}`);
        if (moved > 0) parts.push(`moved ${fmtBytes(moved)} to the Recycle Bin (empty it to free the space)`);
        if (skipped > 0) parts.push(`${fmtNum(skipped)} item${skipped === 1 ? "" : "s"} skipped because they were in use or protected`);
        if (parts.length) toast(parts.join(" · ") + ".", freed + moved > 0 ? "ok" : "info");
        errs.slice(0, 2).forEach((x) => toast(x.error!, "err"));
        await refresh();
        setVersion((v) => v + 1);
      }),
    ];
    return () => {
      un.forEach((p) => p.then((f) => f()));
    };
  }, [refresh, toast]);

  const goTo = useCallback(async (d: string, id: number, opts?: { open?: boolean }) => {
    const det = await api.detail(d, id);
    const crumbs = det.crumbs;
    const isDir = det.node.dir;
    const root = opts?.open && isDir ? id : crumbs.length >= 2 ? crumbs[crumbs.length - 2][0] : 0;
    setDrive(d);
    setNav({ drive: d, root, select: opts?.open && isDir ? null : id, seq: Date.now() });
    setTab("explore");
  }, []);

  const requestDelete = useCallback((items: DelItem[], preferred?: DeleteMode) => {
    if (items.length) setConfirm({ items, preferred });
  }, []);

  const doDelete = async (items: DelItem[], mode: DeleteMode) => {
    setConfirm(null);
    setBusy("clean");
    setCleanProg({ index: 0, of: items.length, files: 0, current: "" });
    try {
      await api.clean(items.map((i) => ({ drive: i.drive, id: i.id, mode: i.forceMode ?? mode })));
    } catch (e) {
      setCleanProg(null);
      setBusy(null);
      toast(String(e), "err");
    }
  };

  const runAction = useCallback(
    async (action: string, drive?: string) => {
      setBusy(ACTION_LABELS[action] ?? action);
      toast(`${ACTION_LABELS[action] ?? action}…`, "info");
      try {
        const msg = await api.runAction(action, drive);
        toast(msg, "ok");
        await refresh();
        setVersion((v) => v + 1);
      } catch (e) {
        toast(String(e), "err");
      } finally {
        setBusy(null);
      }
    },
    [refresh, toast],
  );

  const startScan = async (drives: string[], gentle: boolean) => {
    try {
      await api.startScan(drives, gentle);
      setScanning(true);
      setBusy("scan");
    } catch (e) {
      toast(String(e), "err");
    }
  };

  const ctx: AppCtx = useMemo(
    () => ({ info, version, drive, setDrive, nav, goTo, requestDelete, runAction, toast, busy, refresh }),
    [info, version, drive, nav, goTo, requestDelete, runAction, toast, busy, refresh],
  );

  const anyScanned = info?.drives.some((d) => d.scan) ?? false;

  return (
    <Ctx.Provider value={ctx}>
      <div className="app">
        <Header onScan={startScan} scanning={scanning} />
        {info && !anyScanned && !scanning ? (
          <Welcome onScan={startScan} />
        ) : (
          <>
            <nav className="tabs">
              {TABS.map((t) => (
                <button key={t.id} className={tab === t.id ? "tab active" : "tab"} onClick={() => setTab(t.id)} title={t.hint}>
                  {t.label}
                </button>
              ))}
            </nav>
            <main className="main">
              {tab === "explore" && <ExploreView />}
              {tab === "cleanup" && <CleanupView />}
              {tab === "types" && <TypesView />}
              {tab === "files" && <FilesView />}
              {tab === "leftovers" && <LeftoversView />}
              {tab === "dupes" && <DupesView />}
            </main>
          </>
        )}
        {scanning && <ScanOverlay />}
        {confirm && <ConfirmDelete items={confirm.items} preferred={confirm.preferred} onCancel={() => setConfirm(null)} onConfirm={doDelete} />}
        {cleanProg && (
          <div className="modal-back">
            <div className="modal progress-modal">
              <div className="spinner" />
              <h3>Cleaning up…</h3>
              <p className="muted">
                Item {Math.min(cleanProg.index + 1, cleanProg.of)} of {cleanProg.of}
                {cleanProg.files > 0 && ` · ${fmtNum(cleanProg.files)} files removed`}
              </p>
              <p className="path-line">{cleanProg.current}</p>
              <button className="btn ghost" onClick={() => api.cancel()}>
                Stop
              </button>
            </div>
          </div>
        )}
        <div className="toasts">
          {toasts.map((t) => (
            <div key={t.id} className={`toast ${t.kind}`} onClick={() => setToasts((x) => x.filter((y) => y.id !== t.id))}>
              {t.text}
            </div>
          ))}
        </div>
      </div>
    </Ctx.Provider>
  );
}

export function SafetyPill({ s, small }: { s: Safety; small?: boolean }) {
  const map: Record<string, string> = { safe: "Safe", caution: "Caution", keep: "Keep", system: "System", unknown: "Unknown", mixed: "Mixed" };
  return <span className={`pill s-${s}${small ? " sm" : ""}`}>{map[s] ?? s}</span>;
}
