import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useApp } from "../App";
import { api, type DupGroup, type NodeLite } from "../api";
import { fmtAge, fmtBytes, fmtNum } from "../util";
import { toDel } from "./ItemList";

const MB = 1024 * 1024;
let cache: { groups: DupGroup[]; at: number } | null = null;

export default function DupesView() {
  const { toast, requestDelete, goTo, busy, version } = useApp();
  const [minSize, setMinSize] = useState(10 * MB);
  const [personal, setPersonal] = useState(true);
  const [groups, setGroups] = useState<DupGroup[] | null>(cache?.groups ?? null);
  const [running, setRunning] = useState(false);
  const [prog, setProg] = useState<{ done: number; total: number; current: string } | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());

  useEffect(() => {
    const a = listen<{ done: number; total: number; current: string }>("dupes-progress", (e) => setProg(e.payload));
    const b = listen<DupGroup[]>("dupes-done", (e) => {
      setRunning(false);
      setProg(null);
      cache = { groups: e.payload, at: Date.now() };
      setGroups(e.payload);
      setChecked(new Set());
    });
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  }, []);

  // After a delete finishes, drop the deleted copies from the results.
  const pending = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (!pending.current.size) return;
    const gone = pending.current;
    pending.current = new Set();
    setGroups((gs) => {
      const next = gs
        ?.map((g) => {
          const files = g.files.filter((f) => !gone.has(`${f.drive}:${f.id}`));
          return { ...g, files, wasted: g.size * Math.max(0, files.length - 1) };
        })
        .filter((g) => g.files.length > 1);
      if (next) cache = { groups: next, at: cache?.at ?? Date.now() };
      return next ?? null;
    });
    setChecked(new Set());
  }, [version]);

  const start = async () => {
    try {
      await api.startDupes(minSize, personal);
      setRunning(true);
      setGroups(null);
    } catch (e) {
      toast(String(e), "err");
    }
  };

  const k = (f: NodeLite) => `${f.drive}:${f.id}`;
  const all = useMemo(() => groups?.flatMap((g) => g.files) ?? [], [groups]);
  const sel = all.filter((f) => checked.has(k(f)));
  const wasted = groups?.reduce((a, g) => a + g.wasted, 0) ?? 0;

  const autoSelect = (keep: "oldest" | "newest") => {
    const n = new Set<string>();
    groups?.forEach((g) => {
      const sorted = [...g.files].sort((a, b) => (keep === "oldest" ? a.mtime - b.mtime : b.mtime - a.mtime));
      sorted.slice(1).forEach((f) => n.add(k(f)));
    });
    setChecked(n);
  };

  return (
    <div className="dupes">
      <div className="toolbar">
        <label className="check-line">
          Files larger than
          <select value={minSize} onChange={(e) => setMinSize(+e.target.value)}>
            {[1, 10, 50, 100, 500].map((m) => (
              <option key={m} value={m * MB}>
                {m} MB
              </option>
            ))}
          </select>
        </label>
        <label className="check-line" title="Skips Windows, Program Files, AppData and developer folders, where duplicates are normal and not yours to remove.">
          <input type="checkbox" checked={personal} onChange={(e) => setPersonal(e.target.checked)} /> Only my files (skip system & app folders)
        </label>
        <button className="btn primary" disabled={running || !!busy} onClick={start}>
          {running ? "Searching…" : groups ? "Search again" : "Find duplicates"}
        </button>
        {groups && groups.length > 0 && (
          <>
            <button className="btn ghost sm" onClick={() => autoSelect("oldest")}>
              Select all but the oldest copy
            </button>
            <button className="btn ghost sm" onClick={() => autoSelect("newest")}>
              Select all but the newest copy
            </button>
          </>
        )}
      </div>

      {running && (
        <div className="dupe-prog">
          <div className="bar big">
            <i style={{ width: `${prog && prog.total ? Math.min(100, (prog.done / prog.total) * 100) : 2}%` }} />
          </div>
          <p className="path-line">{prog?.current ?? "Grouping files by size…"}</p>
          <p className="muted small">Files are compared by size, then a quick sample, then their full content. Only byte-for-byte identical files are reported. Uses low disk priority.</p>
          <button className="btn ghost sm" onClick={() => api.cancel()}>
            Stop
          </button>
        </div>
      )}

      {!running && !groups && (
        <div className="empty">
          <h3>Find identical copies of the same file</h3>
          <p className="muted">
            Same video exported twice, the same installer downloaded three times, photos copied to several folders. Reads only files that share an exact size, so it's fast.
          </p>
        </div>
      )}

      {groups && !running && (
        <>
          <p className="muted">
            {groups.length ? (
              <>
                <b>{fmtNum(groups.length)}</b> sets of identical files · <b>{fmtBytes(wasted)}</b> taken up by extra copies
              </>
            ) : (
              "No duplicates found."
            )}
            {cache && ` · searched ${fmtAge(cache.at / 1000)}`}
          </p>
          <div className="dupe-list">
            {groups.slice(0, 400).map((g, gi) => (
              <div key={gi} className="dupe-group">
                <div className="dg-head">
                  <b>{g.files[0].name}</b>
                  <span className="muted small">
                    {g.files.length} copies × {fmtBytes(g.size)} · {fmtBytes(g.wasted)} wasted
                  </span>
                </div>
                {g.files.map((f) => (
                  <div key={k(f)} className={`dg-file${checked.has(k(f)) ? " checked" : ""}`}>
                    <input
                      type="checkbox"
                      checked={checked.has(k(f))}
                      onChange={() =>
                        setChecked((s) => {
                          const n = new Set(s);
                          if (n.has(k(f))) n.delete(k(f));
                          else n.add(k(f));
                          return n;
                        })
                      }
                    />
                    <span className="ell" title={f.path ?? ""}>
                      {f.path}
                    </span>
                    <span className="muted small">modified {fmtAge(f.mtime)}</span>
                    <button className="link" onClick={() => goTo(f.drive, f.id)}>
                      Show
                    </button>
                  </div>
                ))}
              </div>
            ))}
          </div>
        </>
      )}

      {sel.length > 0 && (
        <div className="selbar floating">
          <span>
            {sel.length} copies selected · <b>{fmtBytes(sel.reduce((a, f) => a + f.alloc, 0))}</b>
          </span>
          {groups?.some((g) => g.files.every((f) => checked.has(k(f)))) && <span className="warn-text">Some sets have every copy selected!</span>}
          <button className="btn ghost sm" onClick={() => setChecked(new Set())}>
            Clear
          </button>
          <button
            className="btn danger sm"
            onClick={() => {
              pending.current = new Set(sel.map(k));
              requestDelete(sel.map(toDel), "recycle");
            }}
          >
            Delete selected…
          </button>
        </div>
      )}
    </div>
  );
}
