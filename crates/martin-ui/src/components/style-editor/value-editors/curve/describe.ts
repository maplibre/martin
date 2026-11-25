import { Color, colorKey } from '../../colors';
import type { PropertySpec } from '../../style-spec';
import type { CurveValue } from '../../value-model';
import { fmt, isPosition } from './geometry';

/** One plain-language sentence describing what the curve does, or an empty string. */
export function describeCurve(value: CurveValue, spec: PropertySpec): string {
  const stops = value.stops.filter((s) => isPosition(s.input));
  if (stops.length === 0) return '';
  const first = stops[0];
  const last = stops[stops.length - 1];
  const at = (n: number) =>
    value.mode === 'zoom' ? `z${fmt(n)}` : `${value.attribute ?? 'value'} ${fmt(n)}`;
  const out = (o: unknown) => {
    if (typeof o === 'number') {
      const unit = spec.units === 'pixels' ? ' px' : '';
      return `${fmt(o)}${unit}`;
    }
    const color = typeof o === 'string' ? Color.parse(o) : undefined;
    return color ? colorKey(color) : String(o);
  };
  const kind = value.interpolation.kind;
  if (kind === 'step')
    return `Steps through ${stops.length} values, starting at ${out(first.output)} until ${at(stops.length > 1 ? stops[1].input : first.input)}.`;
  if (stops.length < 2) return '';
  const verb =
    typeof first.output === 'number' && typeof last.output === 'number'
      ? last.output > first.output
        ? 'grows'
        : last.output < first.output
          ? 'shrinks'
          : 'stays'
      : 'changes';
  const how = kind === 'exponential' ? `exponential ${fmt(value.interpolation.base)}` : 'linear';
  return `${verb[0].toUpperCase()}${verb.slice(1)} from ${out(first.output)} at ${at(first.input)} to ${out(last.output)} at ${at(last.input)} (${how}).`;
}
