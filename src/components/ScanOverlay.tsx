import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import { useApp } from "../App";
import { fmtBytes, fmtNum } from "../util";

interface P {
  drive: string;
  index: number;
  of: number;
  files: number;
  dirs: number;
  bytes: number;
  denied: number;
  current: string;
  elapsedMs: number;
}

export default function ScanOverlay() {
  const { info } = useApp();
  const [p, setP] = useState<P | null>(null);
  const [saving, setSaving] = useState<string | null>(null);

  useEffect(() => {
    const a = listen<P>("scan-progress", (e) => {
      setP(e.payload);
      setSaving(null);
    });
    const b = listen<string>("scan-saving", (e) => setSaving(e.payload));
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  }, []);

  const d = info?.drives.find((x) => x.letter === p?.drive);
  const used = d ? d.total - d.free : 0;
  const pct = used > 0 && p ? Math.min(99, (p.bytes / used) * 100) : 0;
  const secs = (p?.elapsedMs ?? 0) / 1000;

  return (
    <div className="modal-back">
      <div className="modal scan-modal">
        <div className="scan-anim">
          {Array.from({ length: 12 }).map((_, i) => (
            <i key={i} style={{ animationDelay: `${i * 0.11}s` }} />
          ))}
        </div>
        <h2>
          Scanning {p?.drive ?? "…"}
          {p && p.of > 1 && (
            <span className="muted">
              {" "}
              ({p.index + 1} of {p.of})
            </span>
          )}
        </h2>
        <div className="bar big">
          <i style={{ width: `${pct}%` }} />
        </div>
        <div className="scan-stats">
          <div>
            <b>{fmtNum(p?.files ?? 0)}</b>
            <span>files</span>
          </div>
          <div>
            <b>{fmtNum(p?.dirs ?? 0)}</b>
            <span>folders</span>
          </div>
          <div>
            <b>{fmtBytes(p?.bytes ?? 0)}</b>
            <span>of {fmtBytes(used)} used</span>
          </div>
          <div>
            <b>{secs.toFixed(0)}s</b>
            <span>elapsed</span>
          </div>
        </div>
        <p className="path-line">{saving ? `Saving results for ${saving}…` : p?.current || "Starting…"}</p>
        <p className="muted small">
          Hidden and system folders are included. Junctions and symbolic links are skipped so nothing is counted twice; hard-linked Windows files are counted once.
        </p>
        <button className="btn ghost" onClick={() => api.cancel()}>
          Cancel scan
        </button>
      </div>
    </div>
  );
}
