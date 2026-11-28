import { validateStyleMin } from '@maplibre/maplibre-gl-style-spec';
import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { propertySpec } from '@/components/style-editor/style-spec';
import { parseValue, serializeValue } from '@/components/style-editor/value-model';

describe('value model properties', () => {
  it('reads back every line width it writes', () => {
    const lineWidth = propertySpec('line', 'paint', 'line-width');
    if (!lineWidth) throw new Error('line-width missing');
    fc.assert(
      fc.property(
        fc.oneof(
          fc.record({
            mode: fc.constant('constant' as const),
            value: fc.double({ max: 64, min: 0, noNaN: true }),
          }),
          fc
            .record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              inputs: fc
                .uniqueArray(fc.integer({ max: 24, min: 0 }), { maxLength: 5, minLength: 1 })
                .map((inputs) => inputs.sort((a, b) => a - b)),
              interpolation: fc.oneof(
                fc.constant({ kind: 'linear' as const }),
                fc.record({
                  base: fc.double({ max: 2, min: 1, noNaN: true }),
                  kind: fc.constant('exponential' as const),
                }),
              ),
              mode: fc.constantFrom('zoom' as const, 'ramp' as const),
              outputs: fc.array(fc.double({ max: 64, min: 0, noNaN: true }), {
                maxLength: 5,
                minLength: 5,
              }),
            })
            .map(({ attribute, inputs, interpolation, mode, outputs }) => ({
              attribute: mode === 'zoom' ? undefined : attribute,
              interpolation,
              mode,
              stops: inputs.map((input, i) => ({ input, output: outputs[i] })),
            })),
          fc
            .record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              inputs: fc
                .uniqueArray(fc.integer({ max: 24, min: 0 }), { maxLength: 5, minLength: 1 })
                .map((inputs) => inputs.sort((a, b) => a - b)),
              mode: fc.constantFrom('zoom' as const, 'ramp' as const),
              outputs: fc.array(fc.double({ max: 64, min: 0, noNaN: true }), {
                maxLength: 6,
                minLength: 6,
              }),
            })
            .map(({ attribute, inputs, mode, outputs }) => ({
              attribute: mode === 'zoom' ? undefined : attribute,
              interpolation: { kind: 'step' as const },
              mode,
              stops: [Number.NEGATIVE_INFINITY, ...inputs].map((input, i) => ({
                input,
                output: outputs[i],
              })),
            })),
          fc
            .record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              fallback: fc.double({ max: 64, min: 0, noNaN: true }),
              labels: fc.uniqueArray(
                fc.string({ minLength: 1 }).filter((s) => !(s in Object.prototype)),
                { maxLength: 6, minLength: 1 },
              ),
              outputs: fc.array(fc.double({ max: 64, min: 0, noNaN: true }), {
                maxLength: 6,
                minLength: 6,
              }),
            })
            .map(({ attribute, fallback, labels, outputs }) => ({
              attribute,
              cases: labels.map((label, i) => ({ labels: [label], output: outputs[i] })),
              fallback,
              mode: 'match' as const,
            })),
        ),
        (value) => {
          expect(parseValue(serializeValue(value), lineWidth)).toEqual(value);
        },
      ),
    );
  });

  it('writes only line widths the style spec accepts', () => {
    fc.assert(
      fc.property(
        fc.oneof(
          fc.record({
            mode: fc.constant('constant' as const),
            value: fc.double({ max: 64, min: 0, noNaN: true }),
          }),
          fc
            .record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              inputs: fc
                .uniqueArray(fc.integer({ max: 24, min: 0 }), { maxLength: 5, minLength: 1 })
                .map((inputs) => inputs.sort((a, b) => a - b)),
              interpolation: fc.oneof(
                fc.constant({ kind: 'linear' as const }),
                fc.record({
                  base: fc.double({ max: 2, min: 1, noNaN: true }),
                  kind: fc.constant('exponential' as const),
                }),
              ),
              mode: fc.constantFrom('zoom' as const, 'ramp' as const),
              outputs: fc.array(fc.double({ max: 64, min: 0, noNaN: true }), {
                maxLength: 5,
                minLength: 5,
              }),
            })
            .map(({ attribute, inputs, interpolation, mode, outputs }) => ({
              attribute: mode === 'zoom' ? undefined : attribute,
              interpolation,
              mode,
              stops: inputs.map((input, i) => ({ input, output: outputs[i] })),
            })),
          fc
            .record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              inputs: fc
                .uniqueArray(fc.integer({ max: 24, min: 0 }), { maxLength: 5, minLength: 1 })
                .map((inputs) => inputs.sort((a, b) => a - b)),
              mode: fc.constantFrom('zoom' as const, 'ramp' as const),
              outputs: fc.array(fc.double({ max: 64, min: 0, noNaN: true }), {
                maxLength: 6,
                minLength: 6,
              }),
            })
            .map(({ attribute, inputs, mode, outputs }) => ({
              attribute: mode === 'zoom' ? undefined : attribute,
              interpolation: { kind: 'step' as const },
              mode,
              stops: [Number.NEGATIVE_INFINITY, ...inputs].map((input, i) => ({
                input,
                output: outputs[i],
              })),
            })),
          fc
            .record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              fallback: fc.double({ max: 64, min: 0, noNaN: true }),
              labels: fc.uniqueArray(
                fc.string({ minLength: 1 }).filter((s) => !(s in Object.prototype)),
                { maxLength: 6, minLength: 1 },
              ),
              outputs: fc.array(fc.double({ max: 64, min: 0, noNaN: true }), {
                maxLength: 6,
                minLength: 6,
              }),
            })
            .map(({ attribute, fallback, labels, outputs }) => ({
              attribute,
              cases: labels.map((label, i) => ({ labels: [label], output: outputs[i] })),
              fallback,
              mode: 'match' as const,
            })),
        ),
        (value) => {
          expect(
            validateStyleMin({
              layers: [
                {
                  id: 'roads',
                  paint: { 'line-width': serializeValue(value) as number },
                  source: 'features',
                  'source-layer': 'roads',
                  type: 'line',
                },
              ],
              sources: { features: { tiles: ['http://localhost/{z}/{x}/{y}'], type: 'vector' } },
              version: 8,
            }),
          ).toEqual([]);
        },
      ),
    );
  });
});
