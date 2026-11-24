import { useMemo, useState } from "react";
import { useApp, SafetyPill } from "../App";
import type { NodeLite } from "../api";
import { fmtAge, fmtBytes, fmtNum } from "../util";
import { FileIcon, describe, toDel } from "./ItemList";

/** A selectable list of files/folders from anywhere on the PC, with delete + "show". */
export default function FileTable({ rows, empty, showWhat = true }: { rows: NodeLite[]; empty?: string; showWhat?: boolean }) {
  const { requestDelete, goTo } = useApp();
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const k = (r: NodeLite) => `${r.drive}:${r.id}`;
  const sel = useMemo(() => rows.filter((r) => checked.has(k(r))), [rows, checked]);
  const selBytes = sel.reduce((a, r) => a + r.alloc, 0);
  const max = rows[0]?.alloc || 1;

  return (
    <div className="ftable">
      <div className="ft-body">
        {rows.map((r) => (
          <div key={k(r)} className={`ft-row${checked.has(k(r)) ? " checked" : ""}`}>
            <input
              type="checkbox"
              checked={checked.has(k(r))}
              onChange={() =>
                setChecked((s) => {
                  const n = new Set(s);
                  if (n.has(k(r))) n.delete(k(r));
                  else n.add(k(r));
                  return n;
                })
              }
            />
            <FileIcon n={r} />
            <div className="ft-name">
              <div className="ell">
                <b>{r.name}</b>
                {showWhat && describe(r) && <span className="muted"> · {describe(r)}</span>}
              </div>
              <div className="ell muted small" title={r.path ?? ""}>
                {r.path}
              </div>
            </div>
            <SafetyPill s={r.safety} small />
            <span className="ft-date muted small">{fmtAge(r.dir ? r.newest : r.mtime)}</span>
            <span className="ft-size">
              <span className="sizebar">
                <i style={{ width: `${(r.alloc / max) * 100}%` }} />
              </span>
              <b>{fmtBytes(r.alloc)}</b>
            </span>
            <button className="link" onClick={() => goTo(r.drive, r.id)}>
              Show
            </button>
          </div>
        ))}
        {rows.length === 0 && <div className="empty small">{empty ?? "Nothing found."}</div>}
      </div>
      {sel.length > 0 && (
        <div className="selbar">
          <span>
            {fmtNum(sel.length)} selected · <b>{fmtBytes(selBytes)}</b>
          </span>
          <button className="btn ghost sm" onClick={() => setChecked(new Set())}>
            Clear
          </button>
          <button className="btn danger sm" onClick={() => requestDelete(sel.map(toDel), "recycle")}>
            Delete selected…
          </button>
        </div>
      )}
    </div>
  );
}
