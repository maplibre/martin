import { useEffect, useRef, useState } from 'react';
import type { PropertySpec } from '../../style-spec';
import { type CurveValue, evaluateCurve } from '../../value-model';
import { clampSpec, type Domain, fmt, isPosition, snap, snapUnit, ticks } from './geometry';

const DEFAULT_W = 340;
const H = 148;
const ML = 34;
const MR = 12;
const MT = 12;
const MB = 22;

/** Props of the numeric curve chart. */
export interface CurveChartProps {
  /** Definition of the property, for clamping outputs. */
  spec: PropertySpec;
  /** The curve being drawn. */
  value: CurveValue;
  /** X and Y range to draw, when no drag has frozen it. */
  domain: Domain;
  /** Current map zoom, marked on zoom curves. */
  zoom: number;
  /** Called with the edited curve and the coalesce key of the drag. */
  onChange: (next: CurveValue, coalesce: string) => void;
  /** Index of the highlighted stop, if any. */
  selected?: number | null;
  /** Called when a stop handle is pressed. */
  onSelect?: (index: number) => void;
}

interface Drag {
  index: number;
  key: string;
  domain: Domain;
}

let dragCounter = 0;

/** A marked point on the chart, read off both axes. */
interface Crosshair {
  x: number;
  y: number;
  tone: 'accent' | 'muted';
}

/** Draws a numeric curve with draggable stop handles. */
export function CurveChart({
  spec,
  value,
  domain,
  zoom,
  onChange,
  selected,
  onSelect,
}: CurveChartProps) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const [W, setW] = useState(DEFAULT_W);
  useEffect(() => {
    const el = svgRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(Math.max(200, Math.round(el.clientWidth))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const d = drag?.domain ?? domain;
  const isStep = value.interpolation.kind === 'step';
  const pw = W - ML - MR;
  const ph = H - MT - MB;
  const px = (x: number) => ML + ((x - d.x0) / (d.x1 - d.x0)) * pw;
  const py = (y: number) => MT + (1 - (y - d.y0) / (d.y1 - d.y0)) * ph;
  const stopX = (i: number) => (isPosition(value.stops[i].input) ? value.stops[i].input : d.x0);
  const evalAt = (x: number) => evaluateCurve(value, x);

  const toData = (e: { clientX: number; clientY: number }) => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    const vx = ((e.clientX - rect.left) * W) / rect.width;
    const vy = ((e.clientY - rect.top) * H) / rect.height;
    return {
      x: d.x0 + ((vx - ML) / pw) * (d.x1 - d.x0),
      y: d.y0 + (1 - (vy - MT) / ph) * (d.y1 - d.y0),
    };
  };

  const path = (() => {
    if (isStep) {
      const parts = value.stops.map((s, i) => {
        const x = px(stopX(i));
        const y = py(s.output as number);
        return i === 0 ? `M${x},${y}` : `H${x}V${y}`;
      });
      return `${parts.join('')}H${px(d.x1)}`;
    }
    const n = 160;
    return Array.from({ length: n + 1 }, (_, i) => {
      const x = d.x0 + ((d.x1 - d.x0) * i) / n;
      const y = evalAt(x) ?? 0;
      return `${i === 0 ? 'M' : 'L'}${px(x).toFixed(1)},${py(y).toFixed(1)}`;
    }).join('');
  })();

  const move = (e: React.PointerEvent, i: number) => {
    if (!drag || drag.index !== i) return;
    const { x, y } = toData(e);
    const unitX = snapUnit(value.mode, d.x1 - d.x0);
    const unitY = snapUnit('ramp', d.y1 - d.y0);
    const stops = value.stops.map((s) => ({ ...s }));
    const prev = stops[i - 1]?.input;
    const next = stops[i + 1]?.input;
    const lo = prev !== undefined && isPosition(prev) ? prev + unitX : d.x0;
    const hi = next !== undefined ? next - unitX : d.x1;
    if (isPosition(stops[i].input)) stops[i].input = Math.min(hi, Math.max(lo, snap(x, unitX)));
    stops[i].output = clampSpec(snap(y, unitY), spec);
    onChange({ ...value, stops }, drag.key);
  };

  const prefix = value.mode === 'zoom' ? 'z' : '';
  const crosshairs: Crosshair[] = [];
  const zoomY = value.mode === 'zoom' ? evalAt(zoom) : undefined;
  if (zoomY !== undefined && zoom >= d.x0 && zoom <= d.x1) {
    crosshairs.push({ tone: 'accent', x: zoom, y: zoomY });
  }
  const hoverY = hover === null ? undefined : evalAt(hover);
  if (hover !== null && hoverY !== undefined && !drag) {
    crosshairs.push({ tone: 'muted', x: hover, y: hoverY });
  }
  const nearX = (x: number) => crosshairs.some((c) => Math.abs(px(c.x) - px(x)) < 20);
  const nearY = (y: number) => crosshairs.some((c) => Math.abs(py(c.y) - py(y)) < 10);

  return (
    <div className="flex flex-col gap-1">
      {/* biome-ignore lint/a11y/noSvgWithoutTitle: decorative chart; stops are edited in the table below */}
      <svg
        className="block w-full touch-none select-none rounded-md border bg-background"
        height={H}
        onPointerLeave={() => setHover(null)}
        onPointerMove={(e) => !drag && setHover(Math.min(d.x1, Math.max(d.x0, toData(e).x)))}
        ref={svgRef}
        viewBox={`0 0 ${W} ${H}`}
      >
        {ticks(d.y0, d.y1, 3).map((y) => (
          <g key={`y${y}`}>
            <line
              className="stroke-muted-foreground/25"
              x1={ML}
              x2={W - MR}
              y1={py(y)}
              y2={py(y)}
            />
            {nearY(y) ? null : (
              <text
                className="fill-muted-foreground font-mono text-[10.5px]"
                textAnchor="end"
                x={ML - 4}
                y={py(y) + 3.5}
              >
                {fmt(y)}
              </text>
            )}
          </g>
        ))}
        {ticks(d.x0, d.x1, value.mode === 'zoom' ? 7 : 5).map((x) => (
          <g key={`x${x}`}>
            <line
              className="stroke-muted-foreground/25"
              x1={px(x)}
              x2={px(x)}
              y1={MT}
              y2={H - MB}
            />
            {nearX(x) ? null : (
              <text
                className="fill-muted-foreground font-mono text-[10.5px]"
                textAnchor="middle"
                x={px(x)}
                y={H - 6}
              >
                {fmt(x)}
              </text>
            )}
          </g>
        ))}
        <path className="fill-none stroke-foreground" d={path} strokeWidth={1.5} />
        {crosshairs.map((c) => {
          const x = px(c.x);
          const y = py(c.y);
          const accent = c.tone === 'accent';
          const stroke = accent ? 'stroke-editor-accent' : 'stroke-muted-foreground/60';
          const fill = accent ? 'fill-editor-accent' : 'fill-muted-foreground';
          const xLabel = `${prefix}${fmt(c.x)}`;
          const yLabel = fmt(c.y);
          return (
            <g className="pointer-events-none" key={c.tone}>
              <line className={stroke} strokeDasharray="3 3" x1={x} x2={x} y1={MT} y2={H - MB} />
              <line className={stroke} strokeDasharray="3 3" x1={ML} x2={x} y1={y} y2={y} />
              <circle className={fill} cx={x} cy={y} r={3} />
              <rect
                className="fill-background"
                height={14}
                width={xLabel.length * 6.4 + 6}
                x={x - (xLabel.length * 6.4 + 6) / 2}
                y={H - MB + 3}
              />
              <text
                className={`${fill} font-mono text-[10.5px] font-semibold`}
                textAnchor="middle"
                x={x}
                y={H - 6}
              >
                {xLabel}
              </text>
              <rect className="fill-background" height={14} width={ML - 2} x={0} y={y - 7} />
              <text
                className={`${fill} font-mono text-[10.5px] font-semibold`}
                textAnchor="end"
                x={ML - 4}
                y={y + 3.5}
              >
                {yLabel}
              </text>
            </g>
          );
        })}
        {value.stops.map((s, i) => (
          <circle
            className={`cursor-grab stroke-editor-accent active:cursor-grabbing ${selected === i ? 'fill-editor-accent' : 'fill-background'}`}
            cx={px(stopX(i))}
            cy={py(s.output as number)}
            // biome-ignore lint/suspicious/noArrayIndexKey: the key must survive the input changing mid-drag
            key={i}
            onLostPointerCapture={() => setDrag(null)}
            onPointerDown={(e) => {
              e.currentTarget.setPointerCapture(e.pointerId);
              onSelect?.(i);
              dragCounter += 1;
              setDrag({ domain, index: i, key: `curve-drag-${dragCounter}` });
            }}
            onPointerMove={(e) => move(e, i)}
            onPointerUp={() => setDrag(null)}
            r={5}
            strokeWidth={2}
          />
        ))}
      </svg>
    </div>
  );
}
