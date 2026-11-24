import { useEffect, useMemo, useState } from "react";
import { useApp, SafetyPill } from "../App";
import { api, type ExtRow, type NodeLite } from "../api";
import { TYPE_COLORS, TYPE_LABELS, fmtBytes, fmtNum } from "../util";
import FileTable from "./FileTable";

export default function TypesView() {
  const { version, toast, info } = useApp();
  const [drive, setDrive] = useState<string | null>(null);
  const [rows, setRows] = useState<ExtRow[] | null>(null);
  const [cat, setCat] = useState<string | null>(null);
  const [pick, setPick] = useState<ExtRow | null>(null);
  const [files, setFiles] = useState<NodeLite[] | null>(null);
  const [filter, setFilter] = useState("");

  useEffect(() => {
    setRows(null);
    api.extSummary(drive).then(setRows).catch((e) => toast(String(e), "err"));
  }, [drive, version, toast]);

  useEffect(() => {
    if (!pick) return;
    setFiles(null);
    api
      .findFiles({ drive, ext: pick.ext, minSize: 0, olderThanDays: 0, dirs: false, limit: 300 })
      .then(setFiles)
      .catch((e) => toast(String(e), "err"));
  }, [pick, drive, version, toast]);

  const cats = useMemo(() => {
    const m = new Map<string, { alloc: number; count: number }>();
    rows?.forEach((r) => {
      const c = m.get(r.cat) ?? { alloc: 0, count: 0 };
      c.alloc += r.alloc;
      c.count += r.count;
      m.set(r.cat, c);
    });
    return [...m.entries()].sort((a, b) => b[1].alloc - a[1].alloc);
  }, [rows]);
  const total = cats.reduce((a, c) => a + c[1].alloc, 0) || 1;
  const shown = (rows ?? []).filter((r) => (!cat || r.cat === cat) && (!filter || r.ext.includes(filter.toLowerCase().replace(/^\./, "")) || r.name.toLowerCase().includes(filter.toLowerCase())));
  const scanned = info?.drives.filter((d) => d.scan) ?? [];

  return (
    <div className="types">
      <div className="types-left">
        <div className="toolbar">
          <div className="seg">
            <button className={!drive ? "on" : ""} onClick={() => setDrive(null)}>
              All drives
            </button>
            {scanned.map((d) => (
              <button key={d.letter} className={drive === d.letter ? "on" : ""} onClick={() => setDrive(d.letter)}>
                {d.letter}
              </button>
            ))}
          </div>
          <input className="search" placeholder="Filter extensions…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
        {!rows ? (
          <div className="loading-view">
            <div className="spinner" />
            Counting every file…
          </div>
        ) : (
          <>
            <div className="stack big">
              {cats.map(([c, v]) => (
                <i
                  key={c}
                  title={`${TYPE_LABELS[c] ?? c}: ${fmtBytes(v.alloc)}`}
                  onClick={() => setCat(cat === c ? null : c)}
                  style={{ width: `${(v.alloc / total) * 100}%`, background: TYPE_COLORS[c] ?? TYPE_COLORS.other, opacity: cat && cat !== c ? 0.3 : 1 }}
                />
              ))}
            </div>
            <div className="cat-chips">
              {cats.map(([c, v]) => (
                <button key={c} className={`cat-chip${cat === c ? " on" : ""}`} onClick={() => setCat(cat === c ? null : c)}>
                  <i style={{ background: TYPE_COLORS[c] ?? TYPE_COLORS.other }} />
                  {TYPE_LABELS[c] ?? c}
                  <em>{fmtBytes(v.alloc)}</em>
                </button>
              ))}
            </div>
            <div className="ext-table">
              <div className="et-head">
                <span>Extension</span>
                <span>What it is</span>
                <span>Usually</span>
                <span className="r">Files</span>
                <span className="r">Total size</span>
              </div>
              <div className="et-body">
                {shown.map((r) => (
                  <div key={r.ext} className={`et-row${pick?.ext === r.ext ? " sel" : ""}`} onClick={() => setPick(r)}>
                    <span>
                      <i style={{ background: TYPE_COLORS[r.cat] ?? TYPE_COLORS.other }} />
                      <code>{r.ext ? `.${r.ext}` : "(none)"}</code>
                    </span>
                    <span className="ell" title={r.what}>
                      {r.name}
                    </span>
                    <span>{r.safety !== "unknown" ? <SafetyPill s={r.safety} small /> : <span className="muted small">—</span>}</span>
                    <span className="r muted">{fmtNum(r.count)}</span>
                    <span className="r">
                      <b>{fmtBytes(r.alloc)}</b>
                    </span>
                  </div>
                ))}
              </div>
            </div>
          </>
        )}
      </div>
      <div className="types-right">
        {!pick ? (
          <div className="empty">
            <h3>Pick a file type</h3>
            <p className="muted">See what each extension is, whether it's usually safe to delete, and the biggest files of that type anywhere on your PC.</p>
          </div>
        ) : (
          <>
            <div className="ext-card">
              <div className="ext-big">
                <code>{pick.ext ? `.${pick.ext}` : "(no extension)"}</code>
                <span>{pick.name}</span>
              </div>
              <p>{pick.what || "SpaceScope doesn't have a description for this extension. Look at where these files live to judge what they belong to."}</p>
              <div className="muted small">
                {fmtNum(pick.count)} files · {fmtBytes(pick.alloc)} · {TYPE_LABELS[pick.cat] ?? pick.cat}
                {pick.safety !== "unknown" && (
                  <>
                    {" "}
                    · usually <SafetyPill s={pick.safety} small />
                  </>
                )}
              </div>
              <p className="muted tiny">Where a file lives matters more than its extension. Each file's own verdict below takes its location into account.</p>
            </div>
            {!files ? <div className="loading-view small">Finding files…</div> : <FileTable rows={files} />}
          </>
        )}
      </div>
    </div>
  );
}
