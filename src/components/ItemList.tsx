import { useEffect, useMemo, useState } from "react";
import { useApp, SafetyPill } from "../App";
import { api, type NodeLite, F_HIDDEN, F_DENIED, F_REPARSE, F_SYNTH, F_CLOUD } from "../api";
import { CAT_LABELS, TYPE_LABELS, fmtAge, fmtBytes, fmtNum } from "../util";
import type { DelItem } from "./ConfirmDelete";

type SortKey = "size" | "name" | "date" | "count";

export function FileIcon({ n }: { n: Pick<NodeLite, "dir" | "flags"> }) {
  if (n.flags & F_SYNTH) return <span className="ficon sys">◆</span>;
  if (n.flags & F_REPARSE && n.dir) return <span className="ficon link">↪</span>;
  return n.dir ? <span className="ficon dir">▰</span> : <span className="ficon file">▭</span>;
}

export function describe(n: NodeLite): string {
  if (n.title) return n.title;
  // A file inside e.g. Downloads is better described by its type than by its folder.
  if (!n.dir && n.tcat && n.tcat !== "other") return TYPE_LABELS[n.tcat] ?? "File";
  if (n.cat && n.cat !== "unknown") return CAT_LABELS[n.cat] ?? n.cat;
  if (!n.dir) return "File";
  return "";
}

export function toDel(n: NodeLite): DelItem {
  return { drive: n.drive, id: n.id, name: n.name, path: n.path ?? n.name, alloc: n.alloc, safety: n.safety, dir: n.dir };
}

export default function ItemList({
  drive,
  parent,
  selected,
  onSelect,
  onOpen,
}: {
  drive: string;
  parent: number;
  selected: number | null;
  onSelect: (id: number) => void;
  onOpen: (id: number) => void;
}) {
  const { version, requestDelete, toast } = useApp();
  const [rows, setRows] = useState<NodeLite[]>([]);
  const [sort, setSort] = useState<SortKey>("size");
  const [checked, setChecked] = useState<Set<number>>(new Set());

  useEffect(() => {
    let alive = true;
    api
      .children(drive, parent)
      .then((r) => alive && setRows(r))
      .catch((e) => toast(String(e), "err"));
    setChecked(new Set());
    return () => {
      alive = false;
    };
  }, [drive, parent, version, toast]);

  const total = rows.reduce((a, r) => a + r.alloc, 0) || 1;
  const sorted = useMemo(() => {
    const r = [...rows];
    if (sort === "name") r.sort((a, b) => a.name.localeCompare(b.name));
    else if (sort === "date") r.sort((a, b) => (b.dir ? b.newest : b.mtime) - (a.dir ? a.newest : a.mtime));
    else if (sort === "count") r.sort((a, b) => b.count - a.count);
    return r;
  }, [rows, sort]);

  const checkedRows = rows.filter((r) => checked.has(r.id));
  const checkedSize = checkedRows.reduce((a, r) => a + r.alloc, 0);
  const toggle = (id: number) =>
    setChecked((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const H = ({ k, children, cls }: { k: SortKey; children: React.ReactNode; cls?: string }) => (
    <button className={`th ${cls ?? ""} ${sort === k ? "on" : ""}`} onClick={() => setSort(k)}>
      {children}
    </button>
  );

  return (
    <div className="itemlist">
      <div className="il-head">
        <span className="th chk" />
        <H k="name" cls="name">
          Name
        </H>
        <span className="th what">What it is</span>
        <span className="th saf">Verdict</span>
        <H k="size" cls="size">
          Size on disk
        </H>
        <H k="count" cls="num">
          Files
        </H>
        <H k="date" cls="date">
          Last changed
        </H>
      </div>
      <div className="il-body">
        {sorted.map((r) => {
          const pct = (r.alloc / total) * 100;
          return (
            <div
              key={r.id}
              className={`il-row${selected === r.id ? " sel" : ""}${r.flags & F_HIDDEN ? " hidden" : ""}`}
              onClick={() => onSelect(r.id)}
              onDoubleClick={() => r.dir && !(r.flags & F_REPARSE) && onOpen(r.id)}
            >
              <span className="td chk" onClick={(e) => e.stopPropagation()}>
                {!(r.flags & F_SYNTH) && <input type="checkbox" checked={checked.has(r.id)} onChange={() => toggle(r.id)} />}
              </span>
              <span className="td name">
                <FileIcon n={r} />
                <span className="ell">{r.name}</span>
                {r.flags & F_HIDDEN ? <span className="tag">hidden</span> : null}
                {r.flags & F_DENIED ? <span className="tag warn" title="Windows denied access — run as administrator to see inside">locked</span> : null}
                {r.flags & F_REPARSE ? <span className="tag" title="A link to another location; not counted here">link</span> : null}
                {r.flags & F_CLOUD ? <span className="tag" title="Cloud file — not stored on this PC">cloud</span> : null}
              </span>
              <span className="td what ell muted">{describe(r)}</span>
              <span className="td saf">
                <SafetyPill s={r.safety} small />
              </span>
              <span className="td size">
                <span className="sizebar">
                  <i style={{ width: `${Math.max(pct, 0.5)}%` }} />
                </span>
                <b>{fmtBytes(r.alloc)}</b>
              </span>
              <span className="td num muted">{r.dir ? fmtNum(r.count) : ""}</span>
              <span className="td date muted">{fmtAge(r.dir ? r.newest : r.mtime)}</span>
            </div>
          );
        })}
        {rows.length === 0 && <div className="empty small">Empty folder</div>}
      </div>
      {checkedRows.length > 0 && (
        <div className="selbar">
          <span>
            {checkedRows.length} selected · <b>{fmtBytes(checkedSize)}</b>
          </span>
          <button className="btn ghost sm" onClick={() => setChecked(new Set())}>
            Clear
          </button>
          <button
            className="btn danger sm"
            onClick={() =>
              requestDelete(
                checkedRows.map(toDel),
                "recycle",
              )
            }
          >
            Delete selected…
          </button>
        </div>
      )}
    </div>
  );
}
