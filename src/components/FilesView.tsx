import { useEffect, useState } from "react";
import { useApp } from "../App";
import { api, type FileQuery, type NodeLite } from "../api";
import { TYPE_LABELS, fmtBytes } from "../util";
import FileTable from "./FileTable";

const MB = 1024 * 1024;
const SIZES = [10 * MB, 100 * MB, 500 * MB, 1024 * MB, 5 * 1024 * MB];
const AGES = [
  { d: 0, l: "Any age" },
  { d: 90, l: "Untouched 3+ months" },
  { d: 180, l: "Untouched 6+ months" },
  { d: 365, l: "Untouched 1+ year" },
  { d: 730, l: "Untouched 2+ years" },
];
const CATS = ["", "video", "image", "audio", "archive", "diskimage", "installer", "document", "design", "ai", "vm", "backup", "temp", "log", "program", "game", "other"];

const PRESETS: { label: string; q: Partial<FileQuery> }[] = [
  { label: "Biggest files", q: { minSize: 100 * MB, olderThanDays: 0, cat: null, dirs: false } },
  { label: "Forgotten big files", q: { minSize: 100 * MB, olderThanDays: 365, cat: null, dirs: false } },
  { label: "Videos", q: { minSize: 50 * MB, olderThanDays: 0, cat: "video", dirs: false } },
  { label: "Archives & disk images", q: { minSize: 10 * MB, olderThanDays: 0, cat: "archive", dirs: false } },
  { label: "Installers", q: { minSize: 10 * MB, olderThanDays: 0, cat: "installer", dirs: false } },
  { label: "Stale folders", q: { minSize: 1024 * MB, olderThanDays: 365, cat: null, dirs: true } },
];

export default function FilesView() {
  const { version, toast } = useApp();
  const [q, setQ] = useState<FileQuery>({ minSize: 100 * MB, olderThanDays: 0, cat: null, dirs: false, limit: 500, query: "" });
  const [text, setText] = useState("");
  const [rows, setRows] = useState<NodeLite[] | null>(null);

  useEffect(() => {
    const t = setTimeout(() => setQ((x) => ({ ...x, query: text })), 250);
    return () => clearTimeout(t);
  }, [text]);

  useEffect(() => {
    setRows(null);
    const searching = !!q.query?.trim();
    api
      .findFiles(searching ? { ...q, minSize: 0, olderThanDays: 0, cat: null } : q)
      .then(setRows)
      .catch((e) => toast(String(e), "err"));
  }, [q, version, toast]);

  const set = (p: Partial<FileQuery>) => setQ((x) => ({ ...x, ...p }));
  const searching = !!q.query?.trim();

  return (
    <div className="files">
      <div className="presets">
        {PRESETS.map((p) => (
          <button
            key={p.label}
            className={`preset${!searching && p.q.minSize === q.minSize && p.q.olderThanDays === q.olderThanDays && (p.q.cat ?? null) === (q.cat ?? null) && p.q.dirs === q.dirs ? " on" : ""}`}
            onClick={() => {
              setText("");
              set({ ...p.q, query: "" });
            }}
          >
            {p.label}
          </button>
        ))}
      </div>
      <div className="toolbar filters">
        <input className="search wide" placeholder="Search any file or folder name on your PC…" value={text} onChange={(e) => setText(e.target.value)} />
        <div className="seg">
          <button className={!q.dirs ? "on" : ""} onClick={() => set({ dirs: false })}>
            Files
          </button>
          <button className={q.dirs ? "on" : ""} onClick={() => set({ dirs: true })}>
            Folders
          </button>
        </div>
        <select value={q.minSize} onChange={(e) => set({ minSize: +e.target.value })} disabled={searching}>
          {SIZES.map((s) => (
            <option key={s} value={s}>
              ≥ {fmtBytes(s, 0)}
            </option>
          ))}
        </select>
        <select value={q.olderThanDays} onChange={(e) => set({ olderThanDays: +e.target.value })} disabled={searching}>
          {AGES.map((a) => (
            <option key={a.d} value={a.d}>
              {a.l}
            </option>
          ))}
        </select>
        <select value={q.cat ?? ""} onChange={(e) => set({ cat: e.target.value || null })} disabled={q.dirs || searching}>
          {CATS.map((c) => (
            <option key={c} value={c}>
              {c ? TYPE_LABELS[c] : "All types"}
            </option>
          ))}
        </select>
      </div>
      <p className="muted small hint">
        {searching
          ? `Files and folders matching "${q.query}" anywhere on your scanned drives, biggest first. Size, age and type filters are ignored while searching.`
          : q.olderThanDays
            ? "'Untouched' means not modified in that time. Big files nobody has changed in a year are often forgotten downloads, old exports or finished projects."
            : "The biggest individual items on your PC. Each one says what it is and whether it's safe to delete."}
      </p>
      {!rows ? <div className="loading-view">Searching…</div> : <FileTable rows={rows} empty="Nothing matches these filters." />}
    </div>
  );
}
