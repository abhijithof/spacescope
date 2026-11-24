import { useState } from "react";
import type { DeleteMode, Safety } from "../api";
import { SafetyPill } from "../App";
import { fmtBytes } from "../util";

export interface DelItem {
  drive: string;
  id: number;
  name: string;
  path: string;
  alloc: number;
  safety: Safety;
  dir: boolean;
  /** This item is always cleaned this way (e.g. a cache folder is emptied, not removed). */
  forceMode?: DeleteMode;
}

export default function ConfirmDelete({
  items,
  preferred,
  onCancel,
  onConfirm,
}: {
  items: DelItem[];
  preferred?: DeleteMode;
  onCancel: () => void;
  onConfirm: (items: DelItem[], mode: DeleteMode) => void;
}) {
  const free = items.filter((i) => !i.forceMode);
  const [mode, setMode] = useState<DeleteMode>(preferred === "permanent" ? "permanent" : "recycle");
  const total = items.reduce((a, i) => a + i.alloc, 0);
  const risky = items.filter((i) => i.safety === "keep" || i.safety === "unknown");
  const caution = items.filter((i) => i.safety === "caution");
  const allContents = free.length === 0 && items.every((i) => i.forceMode === "contents");
  const recycleRoots = new Set(items.map((i) => i.drive));

  return (
    <div className="modal-back" onClick={onCancel}>
      <div className="modal confirm" onClick={(e) => e.stopPropagation()}>
        <h2>
          {allContents ? "Clear" : "Delete"} {items.length === 1 ? `"${items[0].name}"` : `${items.length} items`}?
        </h2>
        <p className="lead">
          {allContents ? "The folder stays, everything inside it is removed. " : ""}
          This frees about <b>{fmtBytes(total)}</b>.
        </p>

        <div className="confirm-list">
          {items.slice(0, 8).map((i) => (
            <div key={`${i.drive}${i.id}`}>
              <SafetyPill s={i.safety} small />
              <span className="ell" title={i.path}>
                {i.path}
              </span>
              <em>{fmtBytes(i.alloc)}</em>
            </div>
          ))}
          {items.length > 8 && <div className="muted">…and {items.length - 8} more</div>}
        </div>

        {risky.length > 0 && (
          <div className="warnbox">
            <b>
              {risky.length} item{risky.length === 1 ? " is" : "s are"} marked Keep or Unknown.
            </b>{" "}
            {risky.length === 1 ? "It may be" : "They may be"} your personal files or part of an installed app. Make sure you don't need {risky.length === 1 ? "it" : "them"}.
          </div>
        )}
        {caution.length > 0 && risky.length === 0 && (
          <div className="infobox">
            {caution.length} item{caution.length === 1 ? "" : "s"} marked Caution: you may lose settings, logins or need to re-download something.
          </div>
        )}

        {free.length > 0 && (
          <div className="mode-pick">
            <label className={mode === "recycle" ? "on" : ""}>
              <input type="radio" checked={mode === "recycle"} onChange={() => setMode("recycle")} />
              <div>
                <b>Move to Recycle Bin</b>
                <span>You can restore it if you change your mind. The space is only freed when you empty the Recycle Bin{recycleRoots.size > 1 ? "s" : ""}.</span>
              </div>
            </label>
            <label className={mode === "permanent" ? "on" : ""}>
              <input type="radio" checked={mode === "permanent"} onChange={() => setMode("permanent")} />
              <div>
                <b>Delete permanently</b>
                <span>Frees the space immediately. Can't be undone. Files that are in use are skipped.</span>
              </div>
            </label>
          </div>
        )}
        {allContents && <p className="muted small">Files that are currently in use are skipped automatically. Nothing outside this folder is touched.</p>}

        <div className="modal-actions">
          <button className="btn ghost" onClick={onCancel} autoFocus>
            Cancel
          </button>
          <button className={`btn ${mode === "permanent" || allContents ? "danger" : "primary"}`} onClick={() => onConfirm(items, mode)}>
            {allContents ? `Clear ${fmtBytes(total)}` : mode === "recycle" ? "Move to Recycle Bin" : `Delete ${fmtBytes(total)} permanently`}
          </button>
        </div>
      </div>
    </div>
  );
}
