import { validateStyleMin } from '@maplibre/maplibre-gl-style-spec';
import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { propertySpec } from '@/components/style-editor/style-spec';
import {
  type EditableValue,
  parseValue,
  serializeValue,
} from '@/components/style-editor/value-model';

const width = fc.double({ max: 64, min: 0, noNaN: true });
const stopInputs = fc
  .uniqueArray(fc.integer({ max: 24, min: 0 }), { maxLength: 5, minLength: 1 })
  .map((inputs) => inputs.sort((a, b) => a - b));
const attribute = fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/);

const curve: fc.Arbitrary<EditableValue> = fc
  .record({
    attribute,
    inputs: stopInputs,
    interpolation: fc.oneof(
      fc.constant({ kind: 'linear' as const }),
      fc.record({
        base: fc.double({ max: 2, min: 1, noNaN: true }),
        kind: fc.constant('exponential' as const),
      }),
      fc.constant({ kind: 'step' as const }),
    ),
    mode: fc.constantFrom('zoom' as const, 'ramp' as const),
    outputs: fc.array(width, { maxLength: 6, minLength: 6 }),
  })
  .map(({ attribute, inputs, interpolation, mode, outputs }) => ({
    attribute: mode === 'zoom' ? undefined : attribute,
    interpolation,
    mode,
    stops:
      interpolation.kind === 'step'
        ? [Number.NEGATIVE_INFINITY, ...inputs].map((input, i) => ({ input, output: outputs[i] }))
        : inputs.map((input, i) => ({ input, output: outputs[i] })),
  }));

const match: fc.Arbitrary<EditableValue> = fc
  .record({
    attribute,
    fallback: width,
    labels: fc.uniqueArray(fc.string({ minLength: 1 }), { maxLength: 6, minLength: 1 }),
    outputs: fc.array(width, { maxLength: 6, minLength: 6 }),
  })
  .map(({ attribute, fallback, labels, outputs }) => ({
    attribute,
    cases: labels.map((label, i) => ({ labels: [label], output: outputs[i] })),
    fallback,
    mode: 'match' as const,
  }));

const value = fc.oneof(
  width.map((w) => ({ mode: 'constant' as const, value: w })),
  curve,
  match,
);

const lineWidth = () => {
  const spec = propertySpec('line', 'paint', 'line-width');
  if (!spec) throw new Error('line-width missing');
  return spec;
};

describe('value model properties', () => {
  it('reads back every line width it writes', () => {
    fc.assert(
      fc.property(value, (v) => {
        expect(parseValue(serializeValue(v), lineWidth())).toEqual(v);
      }),
    );
  });

  it('writes only line widths the style spec accepts', () => {
    fc.assert(
      fc.property(value, (v) => {
        const style = {
          layers: [
            {
              id: 'roads',
              paint: { 'line-width': serializeValue(v) },
              source: 'features',
              'source-layer': 'roads',
              type: 'line',
            },
          ],
          sources: { features: { tiles: ['http://localhost/{z}/{x}/{y}'], type: 'vector' } },
          version: 8,
        };
        expect(validateStyleMin(style as never)).toEqual([]);
      }),
    );
  });
});
