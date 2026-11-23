import { Color, categoricalColors, formatColor } from './colors';
import { isExpression, migrateFunction, type PropertySpec } from './style-spec';

/** How a property value is authored; the inspector offers one editor per mode. */
export type ValueMode = 'constant' | 'zoom' | 'match' | 'ramp' | 'expression';

/** How outputs between two stops are blended. */
export type Interpolation =
  | { kind: 'linear' }
  | { kind: 'exponential'; base: number }
  | { kind: 'step' };

/** One input to output pair of a curve; for `step` the first stop's input is `-Infinity` and holds the output below the next stop. */
export interface Stop {
  /** Zoom level or attribute value where the output applies. */
  input: number;
  /** Literal output at this stop. */
  output: unknown;
}

/** A single literal value. */
export interface ConstantValue {
  /** Discriminant. */
  mode: 'constant';
  /** The literal. */
  value: unknown;
}

/** Output driven by zoom (`zoom`) or by a numeric feature attribute (`ramp`). */
export interface CurveValue {
  /** Discriminant. */
  mode: 'zoom' | 'ramp';
  /** Attribute read by a `ramp`; absent for `zoom`. */
  attribute?: string;
  /** Blend between stops. */
  interpolation: Interpolation;
  /** Stops ordered by ascending input. */
  stops: Stop[];
}

/** One branch of a `match`. */
export interface MatchCase {
  /** Attribute values that select this branch. */
  labels: (string | number)[];
  /** Literal output for those values. */
  output: unknown;
}

/** Output chosen by the exact value of a feature attribute. */
export interface MatchValue {
  /** Discriminant. */
  mode: 'match';
  /** Attribute that is compared. */
  attribute: string;
  /** Branches in order. */
  cases: MatchCase[];
  /** Output when no branch matches. */
  fallback: unknown;
}

/** Any expression the structured editors cannot represent, kept verbatim. */
export interface ExpressionValue {
  /** Discriminant. */
  mode: 'expression';
  /** The raw expression. */
  value: unknown;
}

/** A property value in the shape one of the editors understands. */
export type EditableValue = ConstantValue | CurveValue | MatchValue | ExpressionValue;

const isLiteral = (v: unknown) =>
  v === null ||
  ['string', 'number', 'boolean'].includes(typeof v) ||
  (Array.isArray(v) && !isExpression(v));

function unwrapLiteral(v: unknown): unknown {
  return Array.isArray(v) && v[0] === 'literal' ? v[1] : v;
}

function wrapLiteral(v: unknown): unknown {
  return Array.isArray(v) ? ['literal', v] : v;
}

function getAttribute(v: unknown): string | undefined {
  return Array.isArray(v) && v[0] === 'get' && v.length === 2 && typeof v[1] === 'string'
    ? v[1]
    : undefined;
}

function parseInterpolation(v: unknown): Interpolation | undefined {
  if (!Array.isArray(v)) return undefined;
  if (v[0] === 'linear') return { kind: 'linear' };
  if (v[0] === 'exponential' && typeof v[1] === 'number')
    return { base: v[1], kind: 'exponential' };
  return undefined;
}

function parseCurve(v: unknown[]): CurveValue | undefined {
  const input = v[0] === 'step' ? v[1] : v[2];
  const attribute = getAttribute(input);
  const zoom = Array.isArray(input) && input[0] === 'zoom' && input.length === 1;
  if (!zoom && attribute === undefined) return undefined;
  const base = { attribute, mode: zoom ? ('zoom' as const) : ('ramp' as const) };
  if (v[0] === 'step') {
    if (!isLiteral(unwrapLiteral(v[2]))) return undefined;
    const stops: Stop[] = [{ input: Number.NEGATIVE_INFINITY, output: unwrapLiteral(v[2]) }];
    for (let i = 3; i + 1 < v.length; i += 2) {
      if (typeof v[i] !== 'number' || !isLiteral(unwrapLiteral(v[i + 1]))) return undefined;
      stops.push({ input: v[i] as number, output: unwrapLiteral(v[i + 1]) });
    }
    return { ...base, interpolation: { kind: 'step' }, stops };
  }
  const interpolation = parseInterpolation(v[1]);
  if (!interpolation || v.length % 2 === 0) return undefined;
  const stops: Stop[] = [];
  for (let i = 3; i + 1 < v.length; i += 2) {
    if (typeof v[i] !== 'number' || !isLiteral(unwrapLiteral(v[i + 1]))) return undefined;
    stops.push({ input: v[i] as number, output: unwrapLiteral(v[i + 1]) });
  }
  return { ...base, interpolation, stops };
}

function parseMatch(v: unknown[]): MatchValue | undefined {
  const attribute = getAttribute(v[1]);
  if (attribute === undefined || v.length < 5 || v.length % 2 === 0) return undefined;
  const cases: MatchCase[] = [];
  for (let i = 2; i + 1 < v.length - 1; i += 2) {
    const label = v[i];
    const labels = Array.isArray(label) ? label : [label];
    if (!labels.every((l) => typeof l === 'string' || typeof l === 'number')) return undefined;
    if (!isLiteral(unwrapLiteral(v[i + 1]))) return undefined;
    cases.push({ labels: labels as (string | number)[], output: unwrapLiteral(v[i + 1]) });
  }
  const fallback = unwrapLiteral(v[v.length - 1]);
  if (!isLiteral(fallback)) return undefined;
  return { attribute, cases, fallback, mode: 'match' };
}

/** Reads any property value into an editable shape, migrating legacy functions; never throws. */
export function parseValue(raw: unknown, spec: PropertySpec): EditableValue {
  const value = migrateFunction(raw, spec);
  if (!Array.isArray(value) || !isExpression(value) || value[0] === 'literal') {
    return { mode: 'constant', value: unwrapLiteral(value) };
  }
  const parsed =
    value[0] === 'interpolate' || value[0] === 'step'
      ? parseCurve(value)
      : value[0] === 'match'
        ? parseMatch(value)
        : undefined;
  return parsed ?? { mode: 'expression', value };
}

/** Writes an editable shape back as a v8 value; `undefined` output means the property should be unset. */
export function serializeValue(v: EditableValue): unknown {
  switch (v.mode) {
    case 'constant':
      return v.value;
    case 'expression':
      return v.value;
    case 'match':
      return [
        'match',
        ['get', v.attribute],
        ...v.cases.flatMap((c) => [
          c.labels.length === 1 ? c.labels[0] : c.labels,
          wrapLiteral(c.output),
        ]),
        wrapLiteral(v.fallback),
      ];
    case 'zoom':
    case 'ramp': {
      const input = v.mode === 'zoom' ? ['zoom'] : ['get', v.attribute];
      if (v.interpolation.kind === 'step') {
        const [first, ...rest] = v.stops;
        return [
          'step',
          input,
          wrapLiteral(first?.output),
          ...rest.flatMap((s) => [s.input, wrapLiteral(s.output)]),
        ];
      }
      const interp =
        v.interpolation.kind === 'linear' ? ['linear'] : ['exponential', v.interpolation.base];
      return [
        'interpolate',
        interp,
        input,
        ...v.stops.flatMap((s) => [s.input, wrapLiteral(s.output)]),
      ];
    }
  }
}

/** A valid starting literal for a property: its spec default, else the simplest value of its type. */
export function initialValue(spec: PropertySpec): unknown {
  if (spec.default !== undefined) return spec.default;
  switch (spec.type) {
    case 'color':
      return '#000000';
    case 'number':
      return spec.minimum ?? 0;
    case 'boolean':
      return false;
    case 'enum':
      return Object.keys(spec.values ?? {})[0] ?? '';
    case 'array':
      return spec.value === 'number' ? Array.from({ length: spec.length ?? 2 }, () => 0) : [];
    default:
      return '';
  }
}

function firstLiteralOutput(value: unknown, spec: PropertySpec): unknown {
  const wanted =
    spec.type === 'color' ||
    spec.type === 'resolvedImage' ||
    spec.type === 'string' ||
    spec.type === 'enum'
      ? 'string'
      : spec.type === 'number'
        ? 'number'
        : undefined;
  if (!wanted || !Array.isArray(value)) return undefined;
  const op = value[0];
  const outputs =
    op === 'match'
      ? [...value.slice(3, -1).filter((_, i) => i % 2 === 0), value.at(-1)]
      : op === 'case'
        ? [...value.slice(2, -1).filter((_, i) => i % 2 === 0), value.at(-1)]
        : op === 'step'
          ? [value[2], ...value.slice(4).filter((_, i) => i % 2 === 0)]
          : op === 'interpolate'
            ? value.slice(4).filter((_, i) => i % 2 === 0)
            : op === 'coalesce'
              ? value.slice(1)
              : [];
  for (const out of outputs) {
    if (typeof out === wanted && !(wanted === 'string' && isExpression(out))) return out;
    const nested = firstLiteralOutput(out, spec);
    if (nested !== undefined) return nested;
  }
  return undefined;
}

/** The output a value shows at its first stop, case or literal; seeds a new mode when switching. */
export function representativeOutput(v: EditableValue, spec: PropertySpec): unknown {
  switch (v.mode) {
    case 'constant':
      return v.value ?? initialValue(spec);
    case 'zoom':
    case 'ramp':
      return v.stops.at(-1)?.output ?? initialValue(spec);
    case 'match':
      return v.fallback ?? initialValue(spec);
    case 'expression':
      return firstLiteralOutput(v.value, spec) ?? initialValue(spec);
  }
}

/** Options needed to build a value in a new mode. */
export interface ConvertContext {
  /** Definition of the property being edited. */
  spec: PropertySpec;
  /** Attribute to drive `match` and `ramp`; the caller picks a string or numeric one. */
  attribute?: string;
  /** Example attribute values for a new `match`, most frequent first. */
  sampleLabels?: (string | number)[];
  /** Observed numeric range for a new `ramp`. */
  sampleRange?: [number, number];
}

/** Rebuilds a value in another mode, keeping the current output so the map does not change on switch. */
export function convertValue(
  v: EditableValue,
  mode: ValueMode,
  ctx: ConvertContext,
): EditableValue {
  const out = representativeOutput(v, ctx.spec);
  const interpolation: Interpolation = ctx.spec.expression?.interpolated
    ? { kind: 'linear' }
    : { kind: 'step' };
  switch (mode) {
    case 'constant':
      return { mode, value: out };
    case 'expression':
      return { mode, value: serializeValue(v) ?? out };
    case 'zoom': {
      const stops =
        interpolation.kind === 'step'
          ? [
              { input: Number.NEGATIVE_INFINITY, output: out },
              { input: 12, output: out },
            ]
          : [
              { input: 6, output: out },
              { input: 16, output: out },
            ];
      return { interpolation, mode, stops };
    }
    case 'ramp': {
      const [lo, sampledHi] = ctx.sampleRange ?? [0, 100];
      const hi = sampledHi > lo ? sampledHi : lo + 1;
      const stops =
        interpolation.kind === 'step'
          ? [
              { input: Number.NEGATIVE_INFINITY, output: out },
              { input: hi, output: out },
            ]
          : [
              { input: lo, output: out },
              { input: hi, output: out },
            ];
      return { attribute: ctx.attribute ?? '', interpolation, mode, stops };
    }
    case 'match': {
      const labels = (ctx.sampleLabels ?? []).slice(0, 6);
      const base =
        ctx.spec.type === 'color'
          ? Color.parse(typeof out === 'string' ? out : undefined)
          : undefined;
      const outputs = base
        ? categoricalColors(base, labels.length + 1)
            .slice(1)
            .map(formatColor)
        : labels.map(() => out);
      return {
        attribute: ctx.attribute ?? '',
        cases: labels.map((label, i) => ({ labels: [label], output: outputs[i] })),
        fallback: out,
        mode,
      };
    }
  }
}

/** The modes a property may use according to its v8 definition, in menu order. */
export function allowedModes(spec: PropertySpec): ValueMode[] {
  const zoom = spec.expression?.parameters.includes('zoom') ?? false;
  const data =
    spec['property-type'] === 'data-driven' || spec['property-type'] === 'cross-faded-data-driven';
  const numericOrColor = ['number', 'color'].includes(spec.type);
  const modes: ValueMode[] = ['constant'];
  if (zoom) modes.push('zoom');
  if (data) modes.push('match');
  if (data && numericOrColor) modes.push('ramp');
  if (zoom || data) modes.push('expression');
  return modes;
}

/** Evaluates a numeric curve at an input, honouring exponential bases and steps; non-numeric outputs return `undefined`. */
export function evaluateCurve(v: CurveValue, input: number): number | undefined {
  const s = v.stops;
  if (s.length === 0 || !s.every((x) => typeof x.output === 'number')) return undefined;
  const out = (i: number) => s[i].output as number;
  if (input <= s[0].input) return out(0);
  for (let i = 1; i < s.length; i++) {
    if (input < s[i].input) {
      if (v.interpolation.kind === 'step') return out(i - 1);
      const span = s[i].input - s[i - 1].input;
      const base = v.interpolation.kind === 'exponential' ? v.interpolation.base : 1;
      const t =
        base === 1
          ? (input - s[i - 1].input) / span
          : (base ** (input - s[i - 1].input) - 1) / (base ** span - 1);
      return out(i - 1) + (out(i) - out(i - 1)) * t;
    }
  }
  return out(s.length - 1);
}
