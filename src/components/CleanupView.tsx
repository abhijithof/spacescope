import { useEffect, useMemo, useState } from "react";
import { useApp, SafetyPill } from "../App";
import { api, type CleanGroup, type DeleteMode } from "../api";
import { ACTION_LABELS, CAT_LABELS, actionOf, cleanToMode, cls, fmtAge, fmtBytes, fmtNum } from "../util";
import type { DelItem } from "./ConfirmDelete";

const key = (d: string, id: number) => `${d}:${id}`;

const SECTIONS = [
  { id: "safe", title: "Safe: nothing is lost", desc: "Caches, temporary files, logs and crash dumps. Apps rebuild them automatically. Clearing these never deletes anything you made." },
  { id: "caution", title: "With caution: read before deleting", desc: "Deletable, but you lose something: a rollback option, re-downloadable files, or items you'll want to review first." },
  { id: "tools", title: "Windows clean-up tools", desc: "Space that only Windows can safely reclaim. SpaceScope runs the official tool for you." },
] as const;

export default function CleanupView() {
  const { version, requestDelete, runAction, toast, goTo, busy, info, drive: activeDrive } = useApp();
  const [all, setAll] = useState<CleanGroup[] | null>(null);
  const [drive, setDrive] = useState<string | null>(activeDrive);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Set<string>>(new Set());

  useEffect(() => {
    setAll(null);
    api
      .cleanupReport()
      .then((g) => {
        setAll(g);
        setSel(new Set());
      })
      .catch((e) => toast(String(e), "err"));
  }, [version, toast]);

  const sectionOf = (g: CleanGroup) => (actionOf(g.clean) ? "tools" : g.safety === "safe" ? "safe" : "caution");

  // Per-drive totals for the drive switcher (reclaimable = everything except "part of" estimates).
  const drives = useMemo(() => {
    const m = new Map<string, { total: number; safe: number }>();
    info?.drives.filter((d) => d.scan).forEach((d) => m.set(d.letter, { total: 0, safe: 0 }));
    all?.forEach((g) => {
      const t = m.get(g.drive) ?? { total: 0, safe: 0 };
      if (!g.partial) t.total += g.total;
      if (sectionOf(g) === "safe") t.safe += g.total;
      m.set(g.drive, t);
    });
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [all, info]);

  const cur = drive && drives.some(([d]) => d === drive) ? drive : drives[0]?.[0] ?? null;
  const groups = useMemo(() => all?.filter((g) => g.drive === cur) ?? null, [all, cur]);
  const curInfo = info?.drives.find((d) => d.letter === cur);

  const pickDrive = (d: string) => {
    setDrive(d);
    setSel(new Set());
    setOpen(new Set());
  };

  const totals = useMemo(() => {
    const t = { safe: 0, caution: 0, tools: 0 };
    groups?.forEach((g) => (t[sectionOf(g) as keyof typeof t] += g.partial ? 0 : g.total));
    return t;
  }, [groups]);

  const selected = useMemo(() => {
    const items: { g: CleanGroup; e: CleanGroup["entries"][number] }[] = [];
    groups?.forEach((g) => g.entries.forEach((e) => sel.has(key(e.drive, e.id)) && items.push({ g, e })));
    return items;
  }, [groups, sel]);
  const selBytes = selected.reduce((a, x) => a + x.e.alloc, 0);

  const toggleGroup = (g: CleanGroup, on: boolean) =>
    setSel((s) => {
      const n = new Set(s);
      g.entries.forEach((e) => (on ? n.add(key(e.drive, e.id)) : n.delete(key(e.drive, e.id))));
      return n;
    });
  const toggleOne = (k: string) =>
    setSel((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });

  const selectAllSafe = () => groups?.filter((g) => sectionOf(g) === "safe").forEach((g) => toggleGroup(g, true));

  const clean = () => {
    const items: DelItem[] = selected.map(({ g, e }) => {
      const m = cleanToMode(g.clean);
      const force: DeleteMode | undefined = m === "contents" ? "contents" : m === "permanent" ? "permanent" : undefined;
      return { drive: e.drive, id: e.id, name: e.path.split("\\").pop() || e.path, path: e.path, alloc: e.alloc, safety: g.safety, dir: true, forceMode: force };
    });
    requestDelete(items, items.some((i) => !i.forceMode) ? "recycle" : "permanent");
  };

  if (!all || !groups) {
    return (
      <div className="loading-view">
        <div className="spinner" />
        Checking 150+ known locations across your scanned drives…
      </div>
    );
  }

  return (
    <div className="cleanup">
      <div className="drive-tabs">
        {drives.map(([d, t]) => {
          const di = info?.drives.find((x) => x.letter === d);
          return (
            <button key={d} className={`drive-tab${d === cur ? " on" : ""}`} onClick={() => pickDrive(d)}>
              <b>{d}</b>
              <span className="muted ell">{di?.label || "Local Disk"}</span>
              <em>{fmtBytes(t.total)}</em>
              <small>
                {fmtBytes(t.safe)} safe · {di ? fmtBytes(di.free) : "?"} free now
              </small>
            </button>
          );
        })}
      </div>

      <div className="cleanup-hero">
        <div>
          <div className="hero-num">{fmtBytes(totals.safe + totals.caution + totals.tools)}</div>
          <div className="muted">
            could be freed on {cur}
            {curInfo && ` · ${fmtBytes(curInfo.free)} free of ${fmtBytes(curInfo.total)}`}
          </div>
        </div>
        <div className="hero-split">
          <div>
            <i style={{ background: "var(--safe)" }} />
            <b>{fmtBytes(totals.safe)}</b> safe
          </div>
          <div>
            <i style={{ background: "var(--caution)" }} />
            <b>{fmtBytes(totals.caution)}</b> with caution
          </div>
          {totals.tools > 0 && (
            <div>
              <i style={{ background: "var(--system)" }} />
              <b>{fmtBytes(totals.tools)}</b> via Windows tools
            </div>
          )}
        </div>
        <button className="btn primary" onClick={selectAllSafe}>
          Select everything safe
        </button>
      </div>

      {!info?.elevated && groups?.some((g) => g.admin) && (
        <div className="infobox">
          Some locations (Windows Update cache, Windows temp, other users) need administrator rights to clean completely. Items marked <span className="tag">admin</span> will be
          partly skipped unless you restart as administrator.
        </div>
      )}

      {groups && groups.length === 0 && <div className="empty">Nothing worth cleaning on {cur}. It's already tidy.</div>}

      {SECTIONS.map((sec) => {
        const gs = groups.filter((g) => sectionOf(g) === sec.id);
        if (!gs.length) return null;
        return (
          <section key={sec.id} className={`clean-sec sec-${sec.id}`}>
            <div className="sec-head">
              <h3>{sec.title}</h3>
              <p className="muted">{sec.desc}</p>
            </div>
            {gs.map((g) => {
              const action = actionOf(g.clean);
              const n = g.entries.filter((e) => sel.has(key(e.drive, e.id))).length;
              const all = n === g.entries.length && n > 0;
              const isOpen = open.has(g.rule);
              return (
                <div key={g.rule} className={cls("cgroup", all && "checked", isOpen && "open")}>
                  <div className="cg-row" onClick={() => setOpen((o) => (o.has(g.rule) ? new Set([...o].filter((x) => x !== g.rule)) : new Set([...o, g.rule])))}>
                    {!action ? (
                      <input
                        type="checkbox"
                        checked={all}
                        ref={(el) => {
                          if (el) el.indeterminate = n > 0 && !all;
                        }}
                        onClick={(e) => e.stopPropagation()}
                        onChange={(e) => toggleGroup(g, e.target.checked)}
                      />
                    ) : (
                      <span className="cg-tool">⚙</span>
                    )}
                    <div className="cg-main">
                      <div className="cg-title">
                        <b>{g.title}</b>
                        <SafetyPill s={g.safety} small />
                        <span className="tag">{CAT_LABELS[g.cat] ?? g.cat}</span>
                        {g.nEntries > 1 && <span className="tag">{fmtNum(g.nEntries)} places</span>}
                        {g.admin && !info?.elevated && <span className="tag warn">admin</span>}
                        {g.running.length > 0 && <span className="tag warn">{g.running.join(", ")} running</span>}
                      </div>
                      <div className="cg-what muted ell">{g.what}</div>
                    </div>
                    <div className="cg-size">
                      {g.partial ? <span className="muted small">part of </span> : null}
                      <b>{fmtBytes(g.total)}</b>
                    </div>
                    {action && (
                      <button
                        className="btn primary sm"
                        disabled={!!busy}
                        onClick={(e) => {
                          e.stopPropagation();
                          runAction(action, g.drive);
                        }}
                      >
                        {ACTION_LABELS[action] ?? action}
                      </button>
                    )}
                    <span className="chev">{isOpen ? "▾" : "▸"}</span>
                  </div>
                  {isOpen && (
                    <div className="cg-body">
                      {g.why && <p>{g.why}</p>}
                      {g.note && <p className="note">{g.note}</p>}
                      {g.running.length > 0 && <p className="warn-text">Close {g.running.join(", ")} first so files in use can be removed too.</p>}
                      <div className="cg-entries">
                        {g.entries.slice(0, 60).map((e) => {
                          const k = key(e.drive, e.id);
                          return (
                            <div key={k} className="cg-entry">
                              {!action && <input type="checkbox" checked={sel.has(k)} onChange={() => toggleOne(k)} />}
                              <span className="ell" title={e.path}>
                                {e.path}
                              </span>
                              <span className="muted small">{fmtAge(e.newest)}</span>
                              <b>{fmtBytes(e.alloc)}</b>
                              <button className="link" onClick={() => goTo(e.drive, e.id)}>
                                Show
                              </button>
                            </div>
                          );
                        })}
                        {g.entries.length > 60 && (
                          <div className="muted small">
                            …and {fmtNum(g.entries.length - 60)} smaller places (included when the group is ticked)
                          </div>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </section>
        );
      })}

      <div className="clean-foot">
        <span>
          {selected.length ? (
            <>
              <b>{fmtBytes(selBytes)}</b> selected on {cur} in {fmtNum(selected.length)} place{selected.length === 1 ? "" : "s"}
            </>
          ) : (
            <span className="muted">Tick what you want to remove. Expand a group to see exactly which folders are included.</span>
          )}
        </span>
        <button className="btn danger big" disabled={!selected.length || !!busy} onClick={clean}>
          Clean selected
        </button>
      </div>
    </div>
  );
}
