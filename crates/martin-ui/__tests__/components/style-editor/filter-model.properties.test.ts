import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import {
  parseFilter,
  serializeFilter,
  validateFilter,
} from '@/components/style-editor/inspector/filter-model';

describe('filter model properties', () => {
  it('reads back every structured filter it writes', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.oneof(
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('is' as const, 'isNot' as const),
              value: fc.oneof(
                fc.string().filter((s) => !(s in Object.prototype)),
                fc.integer(),
                fc.double({ noDefaultInfinity: true, noNaN: true }),
                fc.boolean(),
              ),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom(
                'lt' as const,
                'lte' as const,
                'gt' as const,
                'gte' as const,
              ),
              value: fc.oneof(fc.integer(), fc.double({ noDefaultInfinity: true, noNaN: true })),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('oneOf' as const, 'notOneOf' as const),
              values: fc.uniqueArray(
                fc.oneof(
                  fc.string().filter((s) => !(s in Object.prototype)),
                  fc.integer(),
                  fc.double({ noDefaultInfinity: true, noNaN: true }),
                  fc.boolean(),
                ),
                { maxLength: 4, minLength: 1 },
              ),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('exists' as const, 'notExists' as const),
            }),
            fc.record({
              attribute: fc.constant('$type'),
              operator: fc.constantFrom('is' as const, 'isNot' as const),
              value: fc.constantFrom('Point', 'LineString', 'Polygon'),
            }),
            fc.record({
              attribute: fc.constant('$type'),
              operator: fc.constantFrom('oneOf' as const, 'notOneOf' as const),
              values: fc.uniqueArray(fc.constantFrom('Point', 'LineString', 'Polygon'), {
                minLength: 1,
              }),
            }),
          ),
          { maxLength: 4 },
        ),
        (conditions) => {
          const model = { conditions, mode: 'structured' as const };
          expect(parseFilter(serializeFilter(model))).toEqual(model);
        },
      ),
    );
  });

  it('writes only filters the style spec accepts', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.oneof(
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('is' as const, 'isNot' as const),
              value: fc.oneof(
                fc.string().filter((s) => !(s in Object.prototype)),
                fc.integer(),
                fc.double({ noDefaultInfinity: true, noNaN: true }),
                fc.boolean(),
              ),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom(
                'lt' as const,
                'lte' as const,
                'gt' as const,
                'gte' as const,
              ),
              value: fc.oneof(fc.integer(), fc.double({ noDefaultInfinity: true, noNaN: true })),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('oneOf' as const, 'notOneOf' as const),
              values: fc.uniqueArray(
                fc.oneof(
                  fc.string().filter((s) => !(s in Object.prototype)),
                  fc.integer(),
                  fc.double({ noDefaultInfinity: true, noNaN: true }),
                  fc.boolean(),
                ),
                { maxLength: 4, minLength: 1 },
              ),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('exists' as const, 'notExists' as const),
            }),
            fc.record({
              attribute: fc.constant('$type'),
              operator: fc.constantFrom('is' as const, 'isNot' as const),
              value: fc.constantFrom('Point', 'LineString', 'Polygon'),
            }),
            fc.record({
              attribute: fc.constant('$type'),
              operator: fc.constantFrom('oneOf' as const, 'notOneOf' as const),
              values: fc.uniqueArray(fc.constantFrom('Point', 'LineString', 'Polygon'), {
                minLength: 1,
              }),
            }),
          ),
          { maxLength: 4 },
        ),
        (conditions) => {
          expect(validateFilter(serializeFilter({ conditions, mode: 'structured' }))).toEqual([]);
        },
      ),
    );
  });

  it('writes the same filter again after reading its own output', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.oneof(
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('is' as const, 'isNot' as const),
              value: fc.oneof(
                fc.string().filter((s) => !(s in Object.prototype)),
                fc.integer(),
                fc.double({ noDefaultInfinity: true, noNaN: true }),
                fc.boolean(),
              ),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom(
                'lt' as const,
                'lte' as const,
                'gt' as const,
                'gte' as const,
              ),
              value: fc.oneof(fc.integer(), fc.double({ noDefaultInfinity: true, noNaN: true })),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('oneOf' as const, 'notOneOf' as const),
              values: fc.uniqueArray(
                fc.oneof(
                  fc.string().filter((s) => !(s in Object.prototype)),
                  fc.integer(),
                  fc.double({ noDefaultInfinity: true, noNaN: true }),
                  fc.boolean(),
                ),
                { maxLength: 4, minLength: 1 },
              ),
            }),
            fc.record({
              attribute: fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/),
              operator: fc.constantFrom('exists' as const, 'notExists' as const),
            }),
            fc.record({
              attribute: fc.constant('$type'),
              operator: fc.constantFrom('is' as const, 'isNot' as const),
              value: fc.constantFrom('Point', 'LineString', 'Polygon'),
            }),
            fc.record({
              attribute: fc.constant('$type'),
              operator: fc.constantFrom('oneOf' as const, 'notOneOf' as const),
              values: fc.uniqueArray(fc.constantFrom('Point', 'LineString', 'Polygon'), {
                minLength: 1,
              }),
            }),
          ),
          { maxLength: 4 },
        ),
        (conditions) => {
          const once = serializeFilter({ conditions, mode: 'structured' });
          expect(serializeFilter(parseFilter(once))).toEqual(once);
        },
      ),
    );
  });
});
