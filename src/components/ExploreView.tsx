import { useCallback, useEffect, useState } from "react";
import { useApp } from "../App";
import { api, type Detail } from "../api";
import Treemap, { type ColorMode } from "./Treemap";
import ItemList from "./ItemList";
import Details from "./Details";
import { SAFETY, TYPE_COLORS, TYPE_LABELS, fmtBytes } from "../util";

export default function ExploreView() {
  const { drive, nav, version, info } = useApp();
  const [root, setRoot] = useState(0);
  const [selected, setSelected] = useState<number | null>(0);
  const [history, setHistory] = useState<number[]>([]);
  const [mode, setMode] = useState<ColorMode>(() => {
    try {
      return (localStorage.getItem("colorMode") as ColorMode) || "safety";
    } catch {
      return "safety";
    }
  });
  const [crumbs, setCrumbs] = useState<[number, string][]>([]);
  const [rootDetail, setRootDetail] = useState<Detail | null>(null);

  // Drive switched from the header
  useEffect(() => {
    if (nav && nav.drive === drive) return;
    setRoot(0);
    setSelected(0);
    setHistory([]);
  }, [drive]); // eslint-disable-line react-hooks/exhaustive-deps

  // "Show in explorer" from other tabs
  useEffect(() => {
    if (!nav) return;
    setRoot(nav.root);
    setSelected(nav.select ?? nav.root);
    setHistory([]);
  }, [nav?.seq]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!drive) return;
    let alive = true;
    api
      .detail(drive, root)
      .then((d) => {
        if (!alive) return;
        setCrumbs(d.crumbs);
        setRootDetail(d);
      })
      .catch(() => {
        if (!alive) return;
        setRoot(0);
        setSelected(0);
      });
    return () => {
      alive = false;
    };
  }, [drive, root, version]);

  const open = useCallback(
    (id: number) => {
      setHistory((h) => [...h, root]);
      setRoot(id);
      setSelected(id);
    },
    [root],
  );

  const select = useCallback((id: number) => setSelected(id), []);

  const up = useCallback(() => {
    if (crumbs.length >= 2) {
      const parent = crumbs[crumbs.length - 2][0];
      setSelected(root);
      setRoot(parent);
    }
  }, [crumbs, root]);

  const back = () => {
    const h = [...history];
    const prev = h.pop();
    if (prev !== undefined) {
      setHistory(h);
      setSelected(root);
      setRoot(prev);
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      if (e.key === "Backspace" || (e.altKey && e.key === "ArrowUp")) {
        e.preventDefault();
        up();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [up]);

  const setColor = (m: ColorMode) => {
    setMode(m);
    try {
      localStorage.setItem("colorMode", m);
    } catch {
      /* ignore */
    }
  };

  if (!drive) return null;
  const d = info?.drives.find((x) => x.letter === drive);
  if (!d?.scan) {
    return <div className="empty">Drive {drive} hasn't been scanned yet. Tick it in the header and press Scan.</div>;
  }

  return (
    <div className="explore">
      <div className="explore-left">
        <div className="toolbar">
          <button className="icon-btn" disabled={history.length === 0} onClick={back} title="Back">
            ←
          </button>
          <button className="icon-btn" disabled={crumbs.length < 2} onClick={up} title="Up one level (Backspace)">
            ↑
          </button>
          <div className="crumbs">
            {crumbs.map(([id, name], i) => (
              <span key={id}>
                {i > 0 && <span className="sep">›</span>}
                <button
                  className={i === crumbs.length - 1 ? "crumb cur" : "crumb"}
                  onClick={() => {
                    if (id !== root) {
                      setHistory((h) => [...h, root]);
                      setRoot(id);
                      setSelected(id);
                    }
                  }}
                >
                  {name}
                </button>
              </span>
            ))}
            {rootDetail && <span className="crumb-size">{fmtBytes(rootDetail.node.alloc)}</span>}
          </div>
          <div className="seg">
            <button className={mode === "safety" ? "on" : ""} onClick={() => setColor("safety")} title="Colour by whether it can be deleted">
              Can I delete it?
            </button>
            <button className={mode === "type" ? "on" : ""} onClick={() => setColor("type")} title="Colour by kind of file">
              File type
            </button>
          </div>
        </div>
        <Legend mode={mode} />
        <Treemap drive={drive} root={root} mode={mode} selected={selected} onSelect={select} onOpen={open} />
        <ItemList drive={drive} parent={root} selected={selected} onSelect={select} onOpen={open} />
      </div>
      <Details drive={drive} id={selected ?? root} onOpen={open} onSelect={select} />
    </div>
  );
}

function Legend({ mode }: { mode: ColorMode }) {
  if (mode === "safety") {
    return (
      <div className="legend">
        {(["safe", "caution", "keep", "system", "unknown"] as const).map((s) => (
          <span key={s}>
            <i style={{ background: SAFETY[s].color }} />
            {SAFETY[s].label}
          </span>
        ))}
        <span className="muted small">Click to inspect · double-click a folder to open · Backspace to go up</span>
      </div>
    );
  }
  const show = ["video", "image", "audio", "document", "design", "archive", "installer", "program", "system", "code", "temp", "ai", "vm", "game", "other"];
  return (
    <div className="legend">
      {show.map((k) => (
        <span key={k}>
          <i style={{ background: TYPE_COLORS[k] }} />
          {TYPE_LABELS[k]}
        </span>
      ))}
    </div>
  );
}
