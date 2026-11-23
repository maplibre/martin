import { Plus, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { PropertySpec } from '../../style-spec';
import type { CurveValue } from '../../value-model';
import { LiteralEditor } from '../literal-editor';
import { fmt, isPosition, sortStops } from './geometry';

/** Props of the stops table. */
export interface StopsTableProps {
  /** Definition of the property, passed to each output editor. */
  spec: PropertySpec;
  /** The curve whose stops are listed. */
  value: CurveValue;
  /** Input of a stop appended after the last one. */
  nextInput: number;
  /** Index of the highlighted stop, if any. */
  selected?: number | null;
  /** Called with the edited curve and an optional coalesce key. */
  onChange: (next: CurveValue, coalesce?: string) => void;
}

const inputClass =
  'h-7 w-full min-w-0 rounded-md border bg-background px-1.5 font-mono text-xs tabular-nums focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

/** Lists every stop with an input field, an output editor and a remove button. */
export function StopsTable({ spec, value, nextInput, selected, onChange }: StopsTableProps) {
  const isStep = value.interpolation.kind === 'step';
  const prefix = value.mode === 'zoom' ? 'z' : '';
  const canRemove = value.stops.length > (isStep ? 1 : 2);
  const setStops = (stops: CurveValue['stops'], coalesce?: string) =>
    onChange({ ...value, stops: sortStops(stops) }, coalesce);

  return (
    <div className="flex flex-col gap-1.5">
      {value.stops.map((stop, i) => {
        const base = isStep && !isPosition(stop.input);
        const next = value.stops[i + 1];
        const baseLabel = next ? `Below ${prefix}${fmt(next.input)}` : 'All';
        return (
          <div
            className={cn(
              'grid grid-cols-[56px_minmax(0,1fr)_20px] items-center gap-x-2 rounded-md',
              selected === i && 'bg-editor-accent-soft ring-1 ring-editor-accent',
            )}
            key={String(stop.input)}
          >
            {base ? (
              <span className="truncate text-[11px] text-muted-foreground" title={baseLabel}>
                {baseLabel}
              </span>
            ) : (
              <input
                aria-label={`Stop ${i + 1} input`}
                className={inputClass}
                data-stop={i}
                defaultValue={stop.input}
                onBlur={(e) => {
                  const n = e.currentTarget.valueAsNumber;
                  if (Number.isNaN(n) || n === stop.input) {
                    e.currentTarget.value = String(stop.input);
                    return;
                  }
                  setStops(value.stops.map((s, j) => (j === i ? { ...s, input: n } : s)));
                }}
                onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                ref={(el) => {
                  if (el && selected === i && document.activeElement?.tagName !== 'INPUT')
                    el.focus({ preventScroll: false });
                }}
                step="any"
                type="number"
              />
            )}
            <div className="min-w-0">
              <LiteralEditor
                compact
                onChange={(output, coalesce) =>
                  setStops(
                    value.stops.map((s, j) => (j === i ? { ...s, output } : s)),
                    coalesce,
                  )
                }
                spec={spec}
                value={stop.output}
              />
            </div>
            <button
              aria-label={`Remove stop ${i + 1}`}
              className="flex size-5 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent disabled:pointer-events-none disabled:opacity-30"
              disabled={!canRemove || base}
              onClick={() =>
                setStops(
                  value.stops.filter((_, j) => j !== i),
                  undefined,
                )
              }
              type="button"
            >
              <X className="size-3.5" />
            </button>
          </div>
        );
      })}
      <button
        className="flex h-7 items-center gap-1 self-start rounded-md px-1.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
        onClick={() =>
          setStops([...value.stops, { input: nextInput, output: value.stops.at(-1)?.output }])
        }
        type="button"
      >
        <Plus className="size-3.5" /> Add stop
      </button>
    </div>
  );
}
