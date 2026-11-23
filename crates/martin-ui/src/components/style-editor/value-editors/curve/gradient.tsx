import type { CurveValue } from '../../value-model';
import { type Domain, fmt, isPosition } from './geometry';

/** Props of the color gradient bar. */
export interface CurveGradientProps {
  /** The curve whose outputs are color strings. */
  value: CurveValue;
  /** X range shown across the bar. */
  domain: Pick<Domain, 'x0' | 'x1'>;
  /** Current map zoom, marked on zoom curves. */
  zoom: number;
  /** Index of the highlighted stop, if any. */
  selected?: number | null;
  /** Called when a stop marker is clicked. */
  onSelect?: (index: number) => void;
}

/** Renders color stops as a gradient (hard edges for step) with a marker per stop. */
export function CurveGradient({ value, domain, zoom, selected, onSelect }: CurveGradientProps) {
  const pct = (x: number) =>
    Math.min(100, Math.max(0, ((x - domain.x0) / (domain.x1 - domain.x0)) * 100));
  const stopPct = (i: number) => (isPosition(value.stops[i].input) ? pct(value.stops[i].input) : 0);
  const colors = value.stops.map((s) => String(s.output));
  const parts =
    value.interpolation.kind === 'step'
      ? colors.flatMap((c, i) => {
          const end = i + 1 < colors.length ? stopPct(i + 1) : 100;
          return [`${c} ${stopPct(i)}%`, `${c} ${end}%`];
        })
      : colors.map((c, i) => `${c} ${stopPct(i)}%`);
  const css = `linear-gradient(to right, ${parts.join(', ')})`;
  const prefix = value.mode === 'zoom' ? 'z' : '';
  const lanes: number[] = [];
  const laneOf = value.stops.map((_, i) => {
    const x = stopPct(i);
    let lane = 0;
    while (lanes[lane] !== undefined && x - lanes[lane] < 12) lane += 1;
    lanes[lane] = x;
    return lane;
  });
  const zoomPct = value.mode === 'zoom' ? pct(zoom) : undefined;
  return (
    <div className="px-2 pt-3 pb-1">
      <div className="relative h-6 rounded-md border" style={{ backgroundImage: css }}>
        {zoomPct !== undefined && (
          <div
            className="absolute -top-1 -bottom-1 w-px border-l border-dashed border-editor-accent"
            style={{ left: `${zoomPct}%` }}
            title={`Current zoom ${zoom.toFixed(1)}`}
          >
            <span className="absolute -top-4 left-1 whitespace-nowrap font-mono text-[10px] text-editor-accent">
              z{fmt(zoom)}
            </span>
          </div>
        )}
        {value.stops.map((s, i) => (
          <button
            aria-label={`Select stop ${i + 1}`}
            className={`absolute size-3 -translate-x-1/2 cursor-pointer rounded-full border-2 border-editor-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent ${selected === i ? 'bg-editor-accent' : 'bg-background'}`}
            key={String(s.input)}
            onClick={() => onSelect?.(i)}
            style={{ left: `${stopPct(i)}%`, top: `calc(100% - 4px + ${laneOf[i] * 8}px)` }}
            type="button"
          />
        ))}
      </div>
      <div className="relative mt-2 h-7 text-[11px] tabular-nums text-muted-foreground">
        {value.stops.map((s, i) => (
          <span
            className={`absolute -translate-x-1/2 font-mono ${selected === i ? 'font-semibold text-editor-accent' : ''}`}
            key={String(s.input)}
            style={{ left: `${stopPct(i)}%`, top: `${laneOf[i] * 12}px` }}
          >
            {isPosition(s.input) ? `${prefix}${fmt(s.input)}` : ''}
          </span>
        ))}
      </div>
      {value.mode === 'zoom' ? (
        <div className="relative h-3 border-t border-border/60 font-mono text-[10px] text-muted-foreground/70 tabular-nums">
          {[0, 4, 8, 12, 16, 20, 24]
            .filter((z) => z >= domain.x0 && z <= domain.x1)
            .map((z) => (
              <span className="absolute -translate-x-1/2" key={z} style={{ left: `${pct(z)}%` }}>
                {z}
              </span>
            ))}
        </div>
      ) : null}
    </div>
  );
}
