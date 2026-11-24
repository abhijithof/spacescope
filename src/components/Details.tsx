import { useEffect, useState } from "react";
import { useApp, SafetyPill } from "../App";
import { api, type AiAnswer, type Detail, type Safety, F_HARDLINK, F_CLOUD, F_DENIED, F_HIDDEN, F_SYSTEM, F_SYNTH } from "../api";
import { ACTION_LABELS, CAT_LABELS, SAFETY, TYPE_COLORS, TYPE_LABELS, actionOf, cleanToMode, fmtAge, fmtBytes, fmtDate, fmtNum } from "../util";
import { FileIcon } from "./ItemList";

export default function Details({ drive, id, onOpen, onSelect }: { drive: string; id: number; onOpen: (id: number) => void; onSelect: (id: number) => void }) {
  const { version, requestDelete, runAction, toast, info: appInfo, goTo, busy } = useApp();
  const [d, setD] = useState<Detail | null>(null);
  const [ai, setAi] = useState<AiAnswer | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiErr, setAiErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setAi(null);
    setAiErr(null);
    api
      .detail(drive, id)
      .then((x) => alive && setD(x))
      .catch(() => alive && setD(null));
    return () => {
      alive = false;
    };
  }, [drive, id, version]);

  if (!d) return <aside className="details empty-details">Select something to see what it is.</aside>;

  const { node: n, info } = d;
  const drv = appInfo?.drives.find((x) => x.letter === drive);
  const pctDrive = drv ? (n.alloc / drv.total) * 100 : 0;
  const safety: Safety = info.safety;
  const action = actionOf(info.clean);
  const mode = cleanToMode(info.clean);
  const synth = !!(n.flags & F_SYNTH);
  const unrecognised = info.source === "none" || safety === "unknown" || info.rule?.endsWith("-app") || info.rule === "program" || info.rule === "programdata-app";
  const del = { drive, id, name: n.name, path: d.path, alloc: n.alloc, safety, dir: n.dir };

  const ask = async () => {
    setAiBusy(true);
    setAiErr(null);
    try {
      setAi(await api.askAi(drive, id));
    } catch (e) {
      setAiErr(String(e));
    } finally {
      setAiBusy(false);
    }
  };

  const title = info.title && !info.inherited ? info.title : null;
  const what = info.what || info.ext?.what || "";

  return (
    <aside className="details">
      <div className="det-head">
        <FileIcon n={n} />
        <div className="det-title">
          <h2 className="ell" title={n.name}>
            {n.name}
          </h2>
          {title && title !== n.name && <div className="det-sub">{title}</div>}
        </div>
      </div>
      <button className="det-path" title="Copy path" onClick={() => navigator.clipboard.writeText(d.path).then(() => toast("Path copied", "ok"))}>
        {d.path}
      </button>

      <div className="det-size">
        <div>
          <b>{fmtBytes(n.alloc)}</b>
          <span>on disk{n.size !== n.alloc && n.size > 0 && ` · ${fmtBytes(n.size)} actual size`}</span>
        </div>
        <div className="det-facts">
          {n.dir && !synth && <span>{fmtNum(n.count)} files</span>}
          {!synth && <span>changed {fmtAge(n.dir ? n.newest : n.mtime)}</span>}
          {pctDrive >= 0.1 && <span>{pctDrive.toFixed(1)}% of {drive}</span>}
        </div>
        <div className="det-flags">
          {n.flags & F_HIDDEN ? <span className="tag">hidden</span> : null}
          {n.flags & F_SYSTEM ? <span className="tag">system attribute</span> : null}
          {n.flags & F_DENIED ? <span className="tag warn">access denied: contents not counted</span> : null}
          {n.flags & F_HARDLINK ? <span className="tag">hard link, counted elsewhere</span> : null}
          {n.flags & F_CLOUD ? <span className="tag">cloud-only, not on disk</span> : null}
          {d.hiddenFiles > 0 && <span className="tag">{fmtNum(d.hiddenFiles)} hidden files inside</span>}
        </div>
      </div>

      <div className={`verdict v-${safety}`}>
        <div className="verdict-top">
          <SafetyPill s={safety} />
          <b>{SAFETY[safety]?.verdict}</b>
        </div>
        {info.cat && info.cat !== "unknown" && <div className="verdict-cat">{CAT_LABELS[info.cat] ?? info.cat}</div>}
      </div>

      {(what || info.inherited) && (
        <section className="det-sec">
          <h4>What is this?</h4>
          {info.inherited && info.matchedPath && info.title && (
            <p className="part-of">
              Inside <button className="link" onClick={() => info.matchedId != null && onSelect(info.matchedId)}>{info.title}</button>
            </p>
          )}
          {what && <p>{what}</p>}
          {info.ext && info.source === "rule" && (
            <p className="muted">
              <b>.{info.ext.ext}</b>: {info.ext.name}. {info.ext.what}
            </p>
          )}
        </section>
      )}
      {info.why && (
        <section className="det-sec">
          <h4>{safety === "safe" ? "Why it's safe" : "What is it important for?"}</h4>
          <p>{info.why}</p>
        </section>
      )}
      {info.note && (
        <section className="det-sec note">
          <h4>{action ? "How to clean it" : "Good to know"}</h4>
          <p>{info.note}</p>
        </section>
      )}
      {!what && !info.why && !synth && (
        <section className="det-sec">
          <h4>What is this?</h4>
          <p className="muted">
            SpaceScope's built-in knowledge doesn't recognise this {n.dir ? "folder" : "file"}. The contents breakdown below often gives it away, or ask the local AI.
          </p>
        </section>
      )}

      {d.running.length > 0 && (
        <div className="warnbox">
          {d.running.join(", ")} {d.running.length === 1 ? "is" : "are"} running. Close {d.running.length === 1 ? "it" : "them"} first for a complete clean; files in use are skipped.
        </div>
      )}
      {info.admin && !appInfo?.elevated && (
        <div className="warnbox">This location is protected by Windows. Restart SpaceScope as administrator (top-right) to clean it fully.</div>
      )}

      <div className="det-actions">
        {action && (
          <button className="btn primary" disabled={!!busy} onClick={() => runAction(action, drive)}>
            {ACTION_LABELS[action] ?? action}
          </button>
        )}
        {mode === "contents" && n.dir && (
          <button className="btn primary" onClick={() => requestDelete([{ ...del, forceMode: "contents" }], "contents")}>
            Clear contents · {fmtBytes(n.alloc)}
          </button>
        )}
        {!d.blocked && (
          <button className={`btn ${safety === "safe" && mode !== "contents" ? "danger" : "ghost"}`} onClick={() => requestDelete([del], safety === "safe" ? "permanent" : "recycle")}>
            Delete…
          </button>
        )}
        <button className="btn ghost" onClick={() => api.openPath(d.path, !n.dir || id === 0 ? true : false)} disabled={synth}>
          Show in Explorer
        </button>
        {n.dir && id !== 0 && !synth && (
          <button className="btn ghost" onClick={() => onOpen(id)}>
            Open
          </button>
        )}
        {n.dir && !synth && (
          <button
            className="btn ghost"
            disabled={!!busy}
            onClick={() =>
              api
                .rescan(drive, id)
                .then(() => {
                  toast("Folder rescanned", "ok");
                  goTo(drive, id);
                })
                .catch((e) => toast(String(e), "err"))
            }
          >
            Rescan
          </button>
        )}
      </div>
      {d.blocked && <p className="blocked">{d.blocked}</p>}

      {!synth && id !== 0 && (
        <section className={`ai-box${unrecognised ? " suggest" : ""}`}>
          {!ai && !aiBusy && (
            <button className="btn ai" onClick={ask}>
              ✦ Ask local AI what this is
            </button>
          )}
          {aiBusy && (
            <div className="ai-wait">
              <div className="spinner sm" /> Asking the local model (stays on this PC)…
            </div>
          )}
          {aiErr && <p className="muted small">{aiErr}</p>}
          {ai && (
            <div className="ai-answer">
              <div className="ai-top">
                <span>AI's best guess</span>
                {(["safe", "caution", "keep", "system"] as const).includes(ai.verdict as never) && <SafetyPill s={ai.verdict as Safety} small />}
                <em>{ai.model}</em>
              </div>
              <p>{ai.what}</p>
              {ai.important && <p className="muted">{ai.important}</p>}
              {ai.reason && <p className="muted small">{ai.reason}</p>}
              <p className="muted tiny">Local models can be wrong. Treat this as a hint and check before deleting.</p>
            </div>
          )}
        </section>
      )}

      {n.dir && d.types.length > 0 && (
        <section className="det-sec">
          <h4>What's inside</h4>
          <div className="stack">
            {d.types.map((t) => (
              <i
                key={t.key}
                title={`${TYPE_LABELS[t.key] ?? t.key}: ${fmtBytes(t.alloc)}`}
                style={{ width: `${(t.alloc / Math.max(n.alloc, 1)) * 100}%`, background: TYPE_COLORS[t.key] ?? TYPE_COLORS.other }}
              />
            ))}
          </div>
          <div className="type-list">
            {d.types.slice(0, 6).map((t) => (
              <div key={t.key}>
                <i style={{ background: TYPE_COLORS[t.key] ?? TYPE_COLORS.other }} />
                <span>{TYPE_LABELS[t.key] ?? t.key}</span>
                <em>{fmtBytes(t.alloc)}</em>
              </div>
            ))}
          </div>
          <div className="ext-list">
            {d.exts.map((e) => (
              <div key={e.key} title={e.label}>
                <code>.{e.key}</code>
                <span className="ell muted">{e.label || "—"}</span>
                <em>
                  {fmtNum(e.count)} · {fmtBytes(e.alloc)}
                </em>
              </div>
            ))}
          </div>
        </section>
      )}

      {n.dir && d.largest.length > 0 && (
        <section className="det-sec">
          <h4>Biggest files inside</h4>
          <div className="mini-files">
            {d.largest.map((f) => (
              <button key={f.id} className="mini-file" onClick={() => goTo(f.drive, f.id)} title={f.path ?? ""}>
                <span className="ell">{f.name}</span>
                <em>{fmtBytes(f.alloc)}</em>
              </button>
            ))}
          </div>
        </section>
      )}

      {!n.dir && !synth && (
        <section className="det-sec muted small">
          Modified {fmtDate(n.mtime)}
          {info.ext && (
            <>
              {" "}
              · {info.ext.name} ({TYPE_LABELS[info.ext.cat] ?? info.ext.cat})
            </>
          )}
        </section>
      )}
    </aside>
  );
}
