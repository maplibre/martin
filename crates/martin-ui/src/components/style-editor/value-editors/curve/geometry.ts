import type { PropertySpec } from '../../style-spec';
import type { CurveValue, Stop } from '../../value-model';

/** Visible x and y range of a curve chart. */
export interface Domain {
  /** Left edge. */
  x0: number;
  /** Right edge. */
  x1: number;
  /** Bottom edge. */
  y0: number;
  /** Top edge. */
  y1: number;
}

/** Highest zoom shown on zoom curves. */
export const MAX_ZOOM = 24;

/** Whether a stop input is a real position rather than the `-Infinity` base of a step. */
export const isPosition = (n: number) => Number.isFinite(n);

/** Whether every output of the curve is a number. */
export function isNumericCurve(v: CurveValue): boolean {
  return v.stops.length > 0 && v.stops.every((s) => typeof s.output === 'number');
}

/** Whether every output of the curve is a color string. */
export function isColorCurve(v: CurveValue, spec: PropertySpec): boolean {
  return spec.type === 'color' && v.stops.every((s) => typeof s.output === 'string');
}

/** Formats a number with at most three significant digits. */
export function fmt(n: number): string {
  return Number(n.toPrecision(3)).toString();
}

/** X range for the curve: 0-24 for zoom, the padded observed range (or stop span) for a ramp. */
export function xDomain(v: CurveValue, range: [number, number] | undefined): [number, number] {
  if (v.mode === 'zoom') return [0, MAX_ZOOM];
  const inputs = v.stops.map((s) => s.input).filter(isPosition);
  const lo = Math.min(...inputs, ...(range ? [range[0]] : []));
  const hi = Math.max(...inputs, ...(range ? [range[1]] : []));
  if (!Number.isFinite(lo) || !Number.isFinite(hi)) return [0, 100];
  const pad = (hi - lo || 1) * 0.05;
  return [lo - pad, hi + pad];
}

/** Y range covering all numeric outputs, with a zero baseline unless values are negative. */
export function yDomain(v: CurveValue, spec: PropertySpec): [number, number] {
  const outs = v.stops.map((s) => s.output).filter((o): o is number => typeof o === 'number');
  let lo = Math.min(0, ...outs);
  let hi = Math.max(...outs);
  if (spec.minimum !== undefined && lo < spec.minimum && Math.min(...outs) >= spec.minimum)
    lo = spec.minimum;
  if (hi <= lo) hi = lo + 1;
  return [lo, hi + (hi - lo) * 0.1];
}

/** Rounding step for a span, so drags land on readable numbers. */
export function snapUnit(mode: CurveValue['mode'], span: number): number {
  if (mode === 'zoom') return 0.25;
  return span >= 500 ? 1 : span >= 50 ? 0.5 : span >= 5 ? 0.1 : 0.01;
}

/** Rounds to a multiple of a unit without float noise. */
export function snap(n: number, unit: number): number {
  return Number((Math.round(n / unit) * unit).toFixed(6));
}

/** Clamps a number to the spec's minimum and maximum. */
export function clampSpec(n: number, spec: PropertySpec): number {
  return Math.min(
    spec.maximum ?? Number.POSITIVE_INFINITY,
    Math.max(spec.minimum ?? Number.NEGATIVE_INFINITY, n),
  );
}

/** Orders stops by input, keeping a step's `-Infinity` base first. */
export function sortStops(stops: Stop[]): Stop[] {
  return [...stops].sort((a, b) => a.input - b.input);
}

/** Evenly spaced tick values across a range. */
export function ticks(lo: number, hi: number, count: number): number[] {
  return Array.from({ length: count }, (_, i) => lo + ((hi - lo) * i) / (count - 1));
}
