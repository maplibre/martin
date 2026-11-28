import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import {
  type FilterCondition,
  type FilterScalar,
  GEOMETRY_TYPE,
  GEOMETRY_TYPES,
  operatorAllowed,
  parseFilter,
  serializeFilter,
  validateFilter,
} from '@/components/style-editor/inspector/filter-model';

const notAnObjectPrototypeKey = (value: string) => !(value in Object.prototype);

const attributeName = fc.stringMatching(/^[a-z_][a-z0-9_]{0,8}$/);

function conditionOn(attribute: string, value: fc.Arbitrary<FilterScalar>) {
  return fc.oneof(
    fc.record({
      attribute: fc.constant(attribute),
      operator: fc.constantFrom('is' as const, 'isNot' as const),
      value,
    }),
    fc.record({
      attribute: fc.constant(attribute),
      operator: fc.constantFrom('lt' as const, 'lte' as const, 'gt' as const, 'gte' as const),
      value: fc.oneof(fc.integer(), fc.double({ noDefaultInfinity: true, noNaN: true })),
    }),
    fc.record({
      attribute: fc.constant(attribute),
      operator: fc.constantFrom('oneOf' as const, 'notOneOf' as const),
      values: fc.uniqueArray(value, { maxLength: 4, minLength: 1 }),
    }),
    fc.record({
      attribute: fc.constant(attribute),
      operator: fc.constantFrom('exists' as const, 'notExists' as const),
    }),
  );
}

const condition: fc.Arbitrary<FilterCondition> = fc
  .oneof(
    attributeName.chain((name) =>
      conditionOn(
        name,
        fc.oneof(
          fc.string().filter(notAnObjectPrototypeKey),
          fc.integer(),
          fc.double({ noDefaultInfinity: true, noNaN: true }),
          fc.boolean(),
        ),
      ),
    ),
    conditionOn(GEOMETRY_TYPE, fc.constantFrom(...GEOMETRY_TYPES)),
  )
  .filter((c) => operatorAllowed(c.attribute, c.operator));

const conditions = fc.array(condition, { maxLength: 4 });

describe('filter model properties', () => {
  it('reads back every structured filter it writes', () => {
    fc.assert(
      fc.property(conditions, (list) => {
        const model = { conditions: list, mode: 'structured' as const };
        expect(parseFilter(serializeFilter(model))).toEqual(model);
      }),
    );
  });

  it('writes only filters the style spec accepts', () => {
    fc.assert(
      fc.property(conditions, (list) => {
        expect(validateFilter(serializeFilter({ conditions: list, mode: 'structured' }))).toEqual(
          [],
        );
      }),
    );
  });

  it('writes the same filter again after reading its own output', () => {
    fc.assert(
      fc.property(conditions, (list) => {
        const once = serializeFilter({ conditions: list, mode: 'structured' });
        expect(serializeFilter(parseFilter(once))).toEqual(once);
      }),
    );
  });
});
