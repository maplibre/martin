import { type KeyboardEvent, type PointerEvent, useRef } from 'react';
import { cn } from '@/lib/utils';

/** Props of the zoom-range control. */
export interface ZoomRangeProps {
  /** The layer's `minzoom`; `undefined` is unset. */
  min: number | undefined;
  /** The layer's `maxzoom`; `undefined` is unset. */
  max: number | undefined;
  /** Current map zoom. */
  zoom: number;
  /** Sets the range; called on every drag step and key press. */
  onZoomRange: (min: number | undefined, max: number | undefined) => void;
}

type Handle = 'min' | 'max';

const MAX_ZOOM = 24;
const STEP = 0.5;
const TICKS = [0, 4, 8, 12, 16, 20, 24];

const pct = (z: number) => `${(Math.min(MAX_ZOOM, Math.max(0, z)) / MAX_ZOOM) * 100}%`;
const fmt = (z: number) => `z${+z.toFixed(1)}`;

function parse(text: string): number | undefined {
  if (text.trim() === '') return undefined;
  const n = Number(text);
  return Number.isFinite(n) ? Math.min(MAX_ZOOM, Math.max(0, n)) : undefined;
}

function NumberField({
  label,
  value,
  placeholder,
  onCommit,
}: {
  label: string;
  value: number | undefined;
  placeholder: string;
  onCommit: (next: number | undefined) => void;
}) {
  return (
    <input
      aria-label={label}
      className="h-6 w-9 shrink-0 rounded border bg-background px-1 text-center font-mono text-[11px] tabular-nums placeholder:text-muted-foreground/60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
      defaultValue={value ?? ''}
      inputMode="decimal"
      key={String(value)}
      onBlur={(e) => {
        const next = parse(e.currentTarget.value);
        if (next !== value) onCommit(next);
      }}
      onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
      placeholder={placeholder}
      type="text"
    />
  );
}

/** One compact control for a layer's zoom range: draggable track, end fields and the current zoom marker. */
export function ZoomRange({ min, max, zoom, onZoomRange }: ZoomRangeProps) {
  const track = useRef<HTMLDivElement>(null);
  const dragging = useRef<Handle | undefined>(undefined);
  const handles = useRef<Record<Handle, HTMLDivElement | null>>({ max: null, min: null });
  const lo = min ?? 0;
  const hi = max ?? MAX_ZOOM;

  const move = (handle: Handle, raw: number) => {
    const z = Math.round(Math.min(MAX_ZOOM, Math.max(0, raw)) / STEP) * STEP;
    if (handle === 'min') {
      const next = Math.min(z, hi);
      onZoomRange(next === 0 ? undefined : next, max);
    } else {
      const next = Math.max(z, lo);
      onZoomRange(min, next === MAX_ZOOM ? undefined : next);
    }
  };

  const zoomAt = (clientX: number) => {
    const rect = track.current?.getBoundingClientRect();
    if (!rect || rect.width === 0) return undefined;
    return ((clientX - rect.left) / rect.width) * MAX_ZOOM;
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    const z = zoomAt(e.clientX);
    if (z === undefined) return;
    const nearer =
      Math.abs(z - lo) === Math.abs(z - hi) ? z > hi : Math.abs(z - lo) < Math.abs(z - hi);
    const handle: Handle = nearer ? 'min' : 'max';
    dragging.current = handle;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    handles.current[handle]?.focus();
    move(handle, z);
    e.preventDefault();
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const z = zoomAt(e.clientX);
    if (dragging.current && z !== undefined) move(dragging.current, z);
  };

  const onKeyDown = (handle: Handle) => (e: KeyboardEvent) => {
    const current = handle === 'min' ? lo : hi;
    const delta: Record<string, number> = {
      ArrowDown: -STEP,
      ArrowLeft: -STEP,
      ArrowRight: STEP,
      ArrowUp: STEP,
      PageDown: -2,
      PageUp: 2,
    };
    if (e.key in delta) move(handle, current + delta[e.key]);
    else if (e.key === 'Home') move(handle, handle === 'min' ? 0 : lo);
    else if (e.key === 'End') move(handle, handle === 'min' ? hi : MAX_ZOOM);
    else return;
    e.preventDefault();
  };

  const unset = min === undefined && max === undefined;
  const hidden = zoom < lo || zoom >= hi;

  const handle = (kind: Handle) => {
    const value = kind === 'min' ? lo : hi;
    const isSet = (kind === 'min' ? min : max) !== undefined;
    return (
      <div
        aria-label={kind === 'min' ? 'Minimum zoom' : 'Maximum zoom'}
        aria-orientation="horizontal"
        aria-valuemax={kind === 'min' ? hi : MAX_ZOOM}
        aria-valuemin={kind === 'min' ? 0 : lo}
        aria-valuenow={value}
        aria-valuetext={fmt(value)}
        className={cn(
          'absolute top-[15px] size-3.5 -translate-x-1/2 cursor-grab rounded-full border-2 bg-background shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-editor-accent',
          isSet ? 'border-editor-accent' : 'border-muted-foreground/50',
        )}
        onKeyDown={onKeyDown(kind)}
        ref={(el) => {
          handles.current[kind] = el;
        }}
        role="slider"
        style={{ left: pct(value) }}
        tabIndex={0}
        title={fmt(value)}
      />
    );
  };

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2">
        <NumberField
          label="Minimum zoom"
          onCommit={(next) =>
            onZoomRange(next, next !== undefined && max !== undefined && next > max ? next : max)
          }
          placeholder="0"
          value={min}
        />
        <div className="mx-1 min-w-0 flex-1">
          <div
            className="relative h-11 touch-none select-none"
            onPointerCancel={() => {
              dragging.current = undefined;
            }}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={() => {
              dragging.current = undefined;
            }}
            ref={track}
          >
            <div className="absolute inset-x-0 top-[20px] h-1 rounded-full bg-muted" />
            <div
              className={cn(
                'absolute top-[20px] h-1 rounded-full',
                unset ? 'bg-editor-accent/25' : 'bg-editor-accent',
              )}
              style={{ left: pct(lo), width: `${((hi - lo) / MAX_ZOOM) * 100}%` }}
            />
            <div
              className="pointer-events-none absolute top-0 -translate-x-1/2"
              style={{ left: pct(zoom) }}
              title={`Current zoom ${fmt(zoom)}`}
            >
              <div className="text-center font-mono text-[10px] leading-3 text-editor-accent tabular-nums">
                {fmt(zoom)}
              </div>
              <div className="mx-auto h-4 w-px bg-editor-accent" />
            </div>
            {TICKS.map((t) => (
              <span
                className="pointer-events-none absolute top-[31px] -translate-x-1/2 font-mono text-[9px] leading-3 text-muted-foreground/60 tabular-nums"
                key={t}
                style={{ left: pct(t) }}
              >
                {t}
              </span>
            ))}
            {handle('min')}
            {handle('max')}
          </div>
        </div>
        <NumberField
          label="Maximum zoom"
          onCommit={(next) =>
            onZoomRange(next !== undefined && min !== undefined && next < min ? next : min, next)
          }
          placeholder={String(MAX_ZOOM)}
          value={max}
        />
      </div>
      {unset ? <p className="text-[11px] text-muted-foreground">All zooms</p> : null}
      {!unset && hidden ? (
        <p className="text-[11px] text-amber-600 dark:text-amber-400">
          Hidden at {fmt(zoom)} (outside range)
        </p>
      ) : null}
    </div>
  );
}
