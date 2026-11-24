import { useEffect, useMemo, useState } from "react";
import { useApp } from "../App";
import { api, type AiAnswer, type Leftover } from "../api";
import { fmtAge, fmtBytes, fmtNum } from "../util";
import { SafetyPill } from "../App";

const CONF = {
  likely: { label: "Likely leftover", desc: "No installed app matches and nothing inside has changed in over 6 months." },
  possible: { label: "Possible leftover", desc: "No installed app matches; last changed 1–6 months ago." },
  active: { label: "Unrecognised, still in use", desc: "No installed app matches, but something wrote here in the last month. Probably a portable app, background service or tool. Leave it unless you know what it is." },
} as const;

export default function LeftoversView() {
  const { version, toast, requestDelete, goTo } = useApp();
  const [rows, setRows] = useState<Leftover[] | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [ai, setAi] = useState<Record<string, AiAnswer | "busy" | string>>({});

  useEffect(() => {
    setRows(null);
    api
      .leftovers()
      .then((r) => {
        setRows(r);
        setChecked(new Set());
      })
      .catch((e) => toast(String(e), "err"));
  }, [version, toast]);

  const k = (r: Leftover) => `${r.drive}:${r.id}`;
  const sel = useMemo(() => rows?.filter((r) => checked.has(k(r))) ?? [], [rows, checked]);

  const ask = async (r: Leftover) => {
    setAi((a) => ({ ...a, [k(r)]: "busy" }));
    try {
      const ans = await api.askAi(r.drive, r.id);
      setAi((a) => ({ ...a, [k(r)]: ans }));
    } catch (e) {
      setAi((a) => ({ ...a, [k(r)]: String(e) }));
    }
  };

  if (!rows) {
    return (
      <div className="loading-view">
        <div className="spinner" />
        Comparing app-data folders with your installed programs…
      </div>
    );
  }

  return (
    <div className="leftovers">
      <div className="infobox">
        Uninstallers often leave their data behind in <code>AppData</code>, <code>ProgramData</code> and <code>Program Files</code>. These folders don't match any program
        installed on this PC. Matching is by name, so check before deleting. Moving them to the Recycle Bin lets you undo.
      </div>
      {(["likely", "possible", "active"] as const).map((c) => {
        const rs = rows.filter((r) => r.confidence === c);
        if (!rs.length) return null;
        return (
          <section key={c} className="lo-sec">
            <h3>
              {CONF[c].label} <span className="muted">· {fmtBytes(rs.reduce((a, r) => a + r.alloc, 0))}</span>
            </h3>
            <p className="muted small">{CONF[c].desc}</p>
            {rs.map((r) => {
              const a = ai[k(r)];
              return (
                <div key={k(r)} className="lo-row">
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
                  <div className="lo-main">
                    <div>
                      <b>{r.name}</b> <span className="muted small">in {r.location}</span>
                    </div>
                    <div className="muted small">
                      {fmtNum(r.count)} files · last changed {fmtAge(r.newest)}
                    </div>
                    {a && a !== "busy" && typeof a === "object" && (
                      <div className="lo-ai">
                        <SafetyPill s={(["safe", "caution", "keep", "system"].includes(a.verdict) ? a.verdict : "unknown") as never} small /> {a.what}{" "}
                        <span className="muted">{a.important}</span>
                      </div>
                    )}
                    {typeof a === "string" && a !== "busy" && <div className="muted small">{a}</div>}
                  </div>
                  <b className="lo-size">{fmtBytes(r.alloc)}</b>
                  <button className="link" disabled={a === "busy"} onClick={() => ask(r)}>
                    {a === "busy" ? "Asking…" : "Ask AI"}
                  </button>
                  <button className="link" onClick={() => goTo(r.drive, r.id)}>
                    Show
                  </button>
                </div>
              );
            })}
          </section>
        );
      })}
      {rows.length === 0 && <div className="empty">No leftovers found. Every app-data folder matches an installed program.</div>}
      {sel.length > 0 && (
        <div className="selbar floating">
          <span>
            {sel.length} selected · <b>{fmtBytes(sel.reduce((a, r) => a + r.alloc, 0))}</b>
          </span>
          <button className="btn ghost sm" onClick={() => setChecked(new Set())}>
            Clear
          </button>
          <button
            className="btn danger sm"
            onClick={() => requestDelete(sel.map((r) => ({ drive: r.drive, id: r.id, name: r.name, path: r.path, alloc: r.alloc, safety: "caution", dir: true })), "recycle")}
          >
            Delete selected…
          </button>
        </div>
      )}
    </div>
  );
}
