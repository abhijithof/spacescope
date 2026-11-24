import { useEffect, useMemo, useRef, useState } from "react";
import { hierarchy, treemap, treemapSquarify, type HierarchyRectangularNode } from "d3-hierarchy";
import { api, type Tm } from "../api";
import { useApp } from "../App";
import { fmtBytes, SAFETY, TYPE_COLORS, TYPE_LABELS, CAT_LABELS } from "../util";

export type ColorMode = "safety" | "type";

interface Props {
  drive: string;
  root: number;
  mode: ColorMode;
  selected: number | null;
  onSelect: (id: number) => void;
  onOpen: (id: number) => void;
}

function colorOf(n: Tm, mode: ColorMode): string {
  if (n.id < 0) return "var(--mixed)";
  if (mode === "type") return TYPE_COLORS[n.tcat] ?? TYPE_COLORS.other;
  return SAFETY[n.safety]?.color ?? "var(--unknown)";
}

export default function Treemap({ drive, root, mode, selected, onSelect, onOpen }: Props) {
  const { version, toast } = useApp();
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [data, setData] = useState<Tm | null>(null);
  const [loading, setLoading] = useState(false);
  const [hover, setHover] = useState<{ x: number; y: number; n: HierarchyRectangularNode<Tm> } | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    api
      .treemap(drive, root, 3, mode === "type")
      .then((d) => alive && setData(d))
      .catch((e) => toast(String(e), "err"))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [drive, root, mode, version, toast]);

  const nodes = useMemo(() => {
    if (!data || size.w < 10 || size.h < 10) return [];
    const h = hierarchy<Tm>(data)
      .sum((d) => (d.children ? 0 : Math.max(d.alloc, 0)))
      .sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
    const laid = treemap<Tm>()
      .size([size.w, size.h])
      .tile(treemapSquarify.ratio(1.2))
      .paddingOuter((d) => (d.depth === 0 ? 0 : 3))
      .paddingTop((d) => (d.depth === 0 ? 0 : d.depth === 1 ? 22 : 17))
      .paddingInner(2)
      .round(true)(h);
    return laid.descendants().slice(1).filter((n) => n.x1 - n.x0 >= 2 && n.y1 - n.y0 >= 2);
  }, [data, size]);

  const total = data?.alloc ?? 1;

  // Memoised so hover updates (tooltip) don't re-render thousands of cells.
  const cells = useMemo(
    () =>
      nodes.map((n) => {
        const d = n.data;
        const w = n.x1 - n.x0;
        const h = n.y1 - n.y0;
        const frame = !!n.children;
        const color = colorOf(d, mode);
        const key = `${d.id}-${n.depth}-${n.x0}-${n.y0}`;
        const style = { left: n.x0, top: n.y0, width: w, height: h, "--c": color } as React.CSSProperties;
        const sel = d.id >= 0 && d.id === selected;
        const common = {
          onMouseMove: (e: React.MouseEvent) => {
            e.stopPropagation();
            setHover({ x: e.clientX, y: e.clientY, n });
          },
          onClick: (e: React.MouseEvent) => {
            e.stopPropagation();
            if (d.id >= 0) onSelect(d.id);
            else if (n.parent && n.parent.data.id >= 0) onSelect(n.parent.data.id);
          },
          onDoubleClick: (e: React.MouseEvent) => {
            e.stopPropagation();
            if (d.id >= 0 && d.dir) onOpen(d.id);
            else if (d.id < 0 && n.parent && n.parent.data.id >= 0 && n.parent.data.id !== root) onOpen(n.parent.data.id);
          },
        };
        if (frame) {
          return (
            <div key={key} className={`tm-frame d${n.depth}${sel ? " sel" : ""}`} style={style} {...common}>
              {h > 16 && w > 40 && (
                <div className="tm-head">
                  <span className="ell">{d.name}</span>
                  {w > 110 && <em>{fmtBytes(d.alloc)}</em>}
                </div>
              )}
            </div>
          );
        }
        const showLabel = w > 46 && h > 26;
        return (
          <div key={key} className={`tm-cell${d.id < 0 ? " other" : ""}${d.dir ? " dir" : ""}${sel ? " sel" : ""}`} style={style} {...common}>
            {showLabel && (
              <div className="tm-label">
                <span className="ell">{d.name}</span>
                {h > 40 && <em>{fmtBytes(d.alloc)}</em>}
              </div>
            )}
          </div>
        );
      }),
    [nodes, selected, mode, root, onSelect, onOpen],
  );

  return (
    <div className="treemap" ref={ref} onMouseLeave={() => setHover(null)}>
      {cells}
      {loading && !data && <div className="tm-loading">Laying out…</div>}
      {hover && <Tooltip h={hover} total={total} mode={mode} />}
    </div>
  );
}

function Tooltip({ h, total, mode }: { h: { x: number; y: number; n: HierarchyRectangularNode<Tm> }; total: number; mode: ColorMode }) {
  const d = h.n.data;
  const path: string[] = [];
  let p = h.n.parent;
  while (p && p.depth > 0) {
    path.unshift(p.data.name);
    p = p.parent;
  }
  const left = Math.min(h.x + 16, window.innerWidth - 340);
  const top = Math.min(h.y + 16, window.innerHeight - 150);
  return (
    <div className="tm-tip" style={{ left, top }}>
      {path.length > 0 && <div className="tip-path">{path.join(" › ")} ›</div>}
      <div className="tip-name">{d.name}</div>
      <div className="tip-row">
        <b>{fmtBytes(d.alloc)}</b>
        <span className="muted">{((d.alloc / total) * 100).toFixed(1)}% of view</span>
        {d.dir && d.count > 0 && <span className="muted">{d.count.toLocaleString()} files</span>}
      </div>
      {d.id >= 0 && (
        <div className="tip-row">
          <span className={`pill sm s-${d.safety}`}>{SAFETY[d.safety]?.short}</span>
          <span className="muted">
            {d.title ??
              (mode === "type" ? TYPE_LABELS[d.tcat] ?? d.tcat : CAT_LABELS[d.cat] ?? (d.dir ? "Folder" : TYPE_LABELS[d.tcat] ?? "File"))}
          </span>
        </div>
      )}
      {d.id < 0 && <div className="muted small">Too small to draw individually. Open the parent folder to see them.</div>}
      {d.dir && d.id >= 0 && <div className="muted small">Double-click to open</div>}
    </div>
  );
}
