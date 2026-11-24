import { useEffect, useState } from "react";
import { useApp } from "../App";
import { api } from "../api";
import { fmtAge, fmtBytes, fmtNum } from "../util";

export default function Header({ onScan, scanning }: { onScan: (drives: string[], gentle: boolean) => void; scanning: boolean }) {
  const { info, drive, setDrive, busy, toast } = useApp();
  const [picked, setPicked] = useState<string[]>([]);
  const [gentle, setGentle] = useState(() => {
    try {
      return localStorage.getItem("gentle") !== "0";
    } catch {
      return true;
    }
  });

  useEffect(() => {
    if (info && picked.length === 0) setPicked(info.drives.map((d) => d.letter));
  }, [info]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (l: string) => setPicked((p) => (p.includes(l) ? p.filter((x) => x !== l) : [...p, l]));
  const denied = info?.drives.reduce((a, d) => a + (d.scan?.denied ?? 0), 0) ?? 0;

  return (
    <header className="header">
      <div className="brand">
        <svg width="26" height="26" viewBox="0 0 32 32" aria-hidden>
          <rect x="2" y="2" width="16" height="17" rx="3" fill="var(--keep)" />
          <rect x="20" y="2" width="10" height="10" rx="3" fill="var(--safe)" />
          <rect x="20" y="14" width="10" height="16" rx="3" fill="var(--caution)" />
          <rect x="2" y="21" width="16" height="9" rx="3" fill="var(--system)" />
        </svg>
        <div>
          <div className="brand-name">SpaceScope</div>
          <div className="brand-sub">storage analyzer</div>
        </div>
      </div>

      <div className="drives">
        {info?.drives.map((d) => {
          const used = d.total - d.free;
          const pct = (used / d.total) * 100;
          const active = drive === d.letter;
          return (
            <div key={d.letter} className={`drive ${active ? "active" : ""} ${d.scan ? "" : "unscanned"}`} onClick={() => d.scan && setDrive(d.letter)}>
              <label className="drive-check" onClick={(e) => e.stopPropagation()} title="Include in next scan">
                <input type="checkbox" checked={picked.includes(d.letter)} onChange={() => toggle(d.letter)} />
              </label>
              <div className="drive-body">
                <div className="drive-top">
                  <b>{d.letter}</b>
                  <span className="muted ell">{d.label || (d.kind === "removable" ? "Removable" : "Local Disk")}</span>
                  <span className="drive-free">{fmtBytes(d.free)} free</span>
                </div>
                <div className={`bar ${pct > 92 ? "danger" : pct > 80 ? "warn" : ""}`}>
                  <i style={{ width: `${pct}%` }} />
                </div>
                <div className="drive-meta muted">
                  {fmtBytes(used)} of {fmtBytes(d.total)} · {d.fs}
                  {d.scan ? ` · scanned ${fmtAge(d.scan.scannedAt)}` : " · not scanned"}
                </div>
              </div>
            </div>
          );
        })}
      </div>

      <div className="scan-box">
        <button className="btn primary big" disabled={scanning || !!busy || picked.length === 0} onClick={() => onScan(picked, gentle)}>
          {scanning ? "Scanning…" : `Scan ${picked.length === info?.drives.length ? "all drives" : picked.join(" ")}`}
        </button>
        <label className="check-line" title="Runs the scan at low CPU and disk priority so it never slows down what you're doing.">
          <input
            type="checkbox"
            checked={gentle}
            onChange={(e) => {
              setGentle(e.target.checked);
              try {
                localStorage.setItem("gentle", e.target.checked ? "1" : "0");
              } catch {
                /* ignore */
              }
            }}
          />
          Low-priority scan
        </label>
        {info &&
          (info.elevated ? (
            <span className="admin-chip ok" title="Running as administrator: every folder can be read, including System Volume Information and other users.">
              Full access
            </span>
          ) : (
            <button
              className="admin-chip"
              title={`Without administrator rights, Windows hides some system folders${denied ? ` (${fmtNum(denied)} folders couldn't be read last time)` : ""}. Restart as admin to see absolutely everything.`}
              onClick={() => api.relaunchAdmin().catch((e) => toast(String(e), "err"))}
            >
              Limited access{denied ? ` · ${fmtNum(denied)} locked` : ""} — run as admin
            </button>
          ))}
      </div>
    </header>
  );
}
