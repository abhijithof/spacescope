import { useApp } from "../App";
import { fmtBytes } from "../util";

export default function Welcome({ onScan }: { onScan: (drives: string[], gentle: boolean) => void }) {
  const { info } = useApp();
  const drives = info?.drives ?? [];
  return (
    <div className="welcome">
      <h1>See exactly what's filling your PC</h1>
      <p className="lead">
        SpaceScope reads every folder on your drives, hidden and system ones included, and shows you what's using space. For each item it tells you what it is, what it's for, and
        whether you can delete it.
      </p>
      <div className="legend-row">
        <span className="pill s-safe">Safe</span> nothing is lost: cache, temp, logs
        <span className="pill s-caution">Caution</span> deletable, but you lose something
        <span className="pill s-keep">Keep</span> your files or installed apps
        <span className="pill s-system">System</span> Windows needs it
      </div>
      <button className="btn primary huge" onClick={() => onScan(drives.map((d) => d.letter), true)}>
        Scan {drives.map((d) => d.letter).join(" + ")} ({fmtBytes(drives.reduce((a, d) => a + d.total - d.free, 0))} used)
      </button>
      <ul className="promises">
        <li>Only scans when you press Scan. Nothing runs in the background, and opening the app never touches your disk.</li>
        <li>Results are saved, so next time they appear instantly.</li>
        <li>Low-priority mode keeps your PC responsive while scanning.</li>
        <li>Nothing is deleted without you confirming it.</li>
      </ul>
    </div>
  );
}
