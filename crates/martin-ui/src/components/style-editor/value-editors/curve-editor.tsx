import { useState } from 'react';
import type { AttributeInfo } from '../source-schema';
import type { PropertySpec } from '../style-spec';
import type { CurveValue, Interpolation } from '../value-model';
import { CurveChart } from './curve/chart';
import { describeCurve } from './curve/describe';
import {
  type Domain,
  isColorCurve,
  isNumericCurve,
  isPosition,
  MAX_ZOOM,
  xDomain,
  yDomain,
} from './curve/geometry';
import { CurveGradient } from './curve/gradient';
import { StopsTable } from './curve/stops-table';

/** Props of the zoom / numeric-attribute curve editor. */
export interface CurveEditorProps {
  /** Definition of the property. */
  spec: PropertySpec;
  /** The curve. */
  value: CurveValue;
  /** Attributes of the layer's source layer, for picking the `ramp` input. */
  attributes: AttributeInfo[];
  /** Current map zoom, marked on zoom curves. */
  zoom: number;
  /** Called with the new curve; `coalesce` merges rapid edits into one history entry. */
  onChange: (next: CurveValue, coalesce?: string) => void;
}

const controlClass =
  'h-7 min-w-0 rounded-md border bg-background px-1.5 text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

const KINDS = [
  { id: 'linear', label: 'Linear' },
  { id: 'exponential', label: 'Exponential' },
  { id: 'step', label: 'Step' },
] as const;

/** Stops table plus a chart for numeric outputs or a gradient for colors. */
export function CurveEditor({ spec, value, attributes, zoom, onChange }: CurveEditorProps) {
  const [selected, setSelected] = useState<number | null>(null);
  const interpolated = spec.expression?.interpolated ?? false;
  const numeric = attributes.filter((a) => a.kind === 'number');
  const info = attributes.find((a) => a.name === value.attribute);
  const range = info?.range;
  const [x0, x1] = xDomain(value, range);
  const [y0, y1] = isNumericCurve(value) ? yDomain(value, spec) : [0, 1];
  const domain: Domain = { x0, x1, y0, y1 };
  const finite = value.stops.map((s) => s.input).filter(isPosition);
  const last = finite.at(-1);
  const nextInput =
    value.mode === 'zoom'
      ? Math.min(MAX_ZOOM, (last ?? 10) + 2) > (last ?? Number.NEGATIVE_INFINITY)
        ? Math.min(MAX_ZOOM, (last ?? 10) + 2)
        : (last ?? 0) + 1
      : (last ?? x0) + (x1 - x0) * 0.1;

  const setInterpolation = (kind: Interpolation['kind']) => {
    if (kind === value.interpolation.kind) return;
    const interpolation: Interpolation = kind === 'exponential' ? { base: 1.5, kind } : { kind };
    let stops = value.stops;
    if (kind === 'step' && stops.length > 0) {
      stops = [{ input: Number.NEGATIVE_INFINITY, output: stops[0].output }, ...stops.slice(1)];
    } else if (value.interpolation.kind === 'step' && stops.length > 0) {
      const second = stops[1]?.input;
      const gap = value.mode === 'zoom' ? 2 : (x1 - x0) * 0.1;
      const input =
        second === undefined ? x0 : Math.max(value.mode === 'zoom' ? 0 : -Infinity, second - gap);
      stops = [{ ...stops[0], input }, ...stops.slice(1)];
    }
    onChange({ ...value, interpolation, stops });
  };

  const summary = describeCurve(value, spec);
  const listId = `curve-attrs-${spec.type}-${value.attribute ?? 'zoom'}`;
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex items-center gap-2">
        {value.mode === 'ramp' ? (
          <>
            <input
              aria-label="Attribute"
              className={`${controlClass} flex-1 font-mono`}
              defaultValue={value.attribute ?? ''}
              key={value.attribute}
              list={listId}
              onBlur={(e) => {
                const attribute = e.currentTarget.value.trim();
                if (attribute && attribute !== value.attribute) onChange({ ...value, attribute });
              }}
              onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
              placeholder="attribute"
              title={value.attribute}
            />
            <datalist id={listId}>
              {numeric.map((a) => (
                <option key={a.name} value={a.name}>
                  {a.range ? `${a.range[0]} to ${a.range[1]}` : ''}
                </option>
              ))}
            </datalist>
          </>
        ) : (
          <span className="flex-1 text-xs font-medium">Zoom</span>
        )}
        {interpolated ? (
          <>
            {value.interpolation.kind === 'exponential' && (
              <input
                aria-label="Exponent base"
                className={`${controlClass} w-14 font-mono tabular-nums`}
                defaultValue={value.interpolation.base}
                key={value.interpolation.base}
                min={0}
                onBlur={(e) => {
                  const base = e.currentTarget.valueAsNumber;
                  if (!Number.isNaN(base) && base > 0)
                    onChange({ ...value, interpolation: { base, kind: 'exponential' } });
                }}
                onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                step="0.1"
                title="Exponent base"
                type="number"
              />
            )}
            <select
              aria-label="Interpolation"
              className={`${controlClass} w-[104px]`}
              onChange={(e) => setInterpolation(e.target.value as Interpolation['kind'])}
              value={value.interpolation.kind}
            >
              {KINDS.map((k) => (
                <option key={k.id} value={k.id}>
                  {k.label}
                </option>
              ))}
            </select>
          </>
        ) : null}
      </div>
      {value.mode === 'ramp' && range && (
        <div
          className="truncate font-mono text-[11px] tabular-nums text-muted-foreground"
          title="Observed range"
        >
          observed {range[0]} to {range[1]}
        </div>
      )}
      {isNumericCurve(value) && (
        <CurveChart
          domain={domain}
          onChange={onChange}
          onSelect={setSelected}
          selected={selected}
          spec={spec}
          value={value}
          zoom={zoom}
        />
      )}
      {isColorCurve(value, spec) && (
        <CurveGradient
          domain={domain}
          onSelect={setSelected}
          selected={selected}
          value={value}
          zoom={zoom}
        />
      )}
      {summary && (
        <p className="text-[11px] leading-snug text-muted-foreground" title={summary}>
          {summary}
        </p>
      )}
      <StopsTable
        nextInput={nextInput}
        onChange={onChange}
        selected={selected}
        spec={spec}
        value={value}
      />
    </div>
  );
}
