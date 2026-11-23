import { describe, expect, it } from 'vitest';
import {
  changeOperator,
  coerceValue,
  type FilterCondition,
  operatorAllowed,
  parseFilter,
  serializeFilter,
  validateFilter,
} from '@/components/style-editor/inspector/filter-model';

describe('parseFilter', () => {
  it('reads a missing filter as no conditions', () => {
    expect(parseFilter(undefined)).toEqual({ conditions: [], mode: 'structured' });
  });

  it('reads a modern equality as one condition', () => {
    expect(parseFilter(['==', ['get', 'class'], 'motorway'])).toEqual({
      conditions: [{ attribute: 'class', operator: 'is', value: 'motorway' }],
      mode: 'structured',
    });
  });

  it('reads an all of modern conditions', () => {
    expect(
      parseFilter([
        'all',
        ['!=', ['get', 'class'], 'path'],
        ['>=', ['get', 'rank'], 3],
        ['!', ['has', 'name']],
      ]),
    ).toEqual({
      conditions: [
        { attribute: 'class', operator: 'isNot', value: 'path' },
        { attribute: 'rank', operator: 'gte', value: 3 },
        { attribute: 'name', operator: 'notExists' },
      ],
      mode: 'structured',
    });
  });

  it('reads a match with true first as is one of', () => {
    expect(parseFilter(['match', ['get', 'class'], ['a', 'b'], true, false])).toEqual({
      conditions: [{ attribute: 'class', operator: 'oneOf', values: ['a', 'b'] }],
      mode: 'structured',
    });
  });

  it('reads a match with false first as is not one of', () => {
    expect(parseFilter(['match', ['get', 'brunnel'], ['bridge', 'tunnel'], false, true])).toEqual({
      conditions: [{ attribute: 'brunnel', operator: 'notOneOf', values: ['bridge', 'tunnel'] }],
      mode: 'structured',
    });
  });

  it('reads a match on a single label as is one of that label', () => {
    expect(parseFilter(['match', ['get', 'class'], 'a', true, false])).toEqual({
      conditions: [{ attribute: 'class', operator: 'oneOf', values: ['a'] }],
      mode: 'structured',
    });
  });

  it('reads an in with a literal haystack as is one of', () => {
    expect(parseFilter(['in', ['get', 'subclass'], ['literal', [true, 'x']]])).toEqual({
      conditions: [{ attribute: 'subclass', operator: 'oneOf', values: [true, 'x'] }],
      mode: 'structured',
    });
  });

  it('reads the geometry type as the special attribute', () => {
    expect(parseFilter(['==', ['geometry-type'], 'Polygon'])).toEqual({
      conditions: [{ attribute: '$type', operator: 'is', value: 'Polygon' }],
      mode: 'structured',
    });
  });

  it('converts every legacy operator', () => {
    expect(
      parseFilter([
        'all',
        ['==', 'a', 1],
        ['!=', 'b', 'x'],
        ['in', 'c', 'p', 'q'],
        ['!in', 'd', 'r'],
        ['has', 'e'],
        ['!has', 'f'],
        ['<', 'g', 1],
        ['<=', 'h', 2],
        ['>', 'i', 3],
        ['>=', 'j', 4],
      ]),
    ).toMatchInlineSnapshot(`
      {
        "conditions": [
          {
            "attribute": "a",
            "operator": "is",
            "value": 1,
          },
          {
            "attribute": "b",
            "operator": "isNot",
            "value": "x",
          },
          {
            "attribute": "c",
            "operator": "oneOf",
            "values": [
              "p",
              "q",
            ],
          },
          {
            "attribute": "d",
            "operator": "notOneOf",
            "values": [
              "r",
            ],
          },
          {
            "attribute": "e",
            "operator": "exists",
          },
          {
            "attribute": "f",
            "operator": "notExists",
          },
          {
            "attribute": "g",
            "operator": "lt",
            "value": 1,
          },
          {
            "attribute": "h",
            "operator": "lte",
            "value": 2,
          },
          {
            "attribute": "i",
            "operator": "gt",
            "value": 3,
          },
          {
            "attribute": "j",
            "operator": "gte",
            "value": 4,
          },
        ],
        "mode": "structured",
      }
    `);
  });

  it('converts a legacy polygon and list filter', () => {
    expect(
      parseFilter(['all', ['==', '$type', 'Polygon'], ['in', 'class', 'residential', 'suburb']]),
    ).toEqual({
      conditions: [
        { attribute: '$type', operator: 'is', value: 'Polygon' },
        { attribute: 'class', operator: 'oneOf', values: ['residential', 'suburb'] },
      ],
      mode: 'structured',
    });
  });

  it('keeps an any filter as an expression', () => {
    const filter = ['any', ['==', ['get', 'a'], 1], ['==', ['get', 'b'], 2]];
    expect(parseFilter(filter)).toEqual({ expression: filter, mode: 'expression' });
  });

  it('keeps a filter with an unreadable condition as the whole expression', () => {
    const filter = [
      'all',
      ['match', ['get', 'brunnel'], ['bridge', 'tunnel'], false, true],
      ['==', ['get', 'class'], 'motorway'],
      ['<', ['zoom'], 5],
    ];
    expect(parseFilter(filter)).toEqual({ expression: filter, mode: 'expression' });
  });

  it('keeps an ordering on the geometry type as an expression', () => {
    const filter = ['<', ['geometry-type'], 'Polygon'];
    expect(parseFilter(filter)).toEqual({ expression: filter, mode: 'expression' });
  });

  it('keeps a legacy filter on feature id as an expression', () => {
    const filter = ['==', '$id', 7];
    expect(parseFilter(filter)).toEqual({ expression: filter, mode: 'expression' });
  });

  it('keeps a legacy in without values as an expression', () => {
    const filter = ['in', 'class'];
    expect(parseFilter(filter)).toEqual({ expression: filter, mode: 'expression' });
  });
});

describe('serializeFilter', () => {
  it('serializes no conditions to undefined', () => {
    expect(serializeFilter({ conditions: [], mode: 'structured' })).toBeUndefined();
  });

  it('serializes one condition without an all', () => {
    expect(
      serializeFilter({
        conditions: [{ attribute: 'class', operator: 'is', value: 'motorway' }],
        mode: 'structured',
      }),
    ).toEqual(['==', ['get', 'class'], 'motorway']);
  });

  it('serializes every operator to modern syntax', () => {
    expect(
      serializeFilter({
        conditions: [
          { attribute: 'a', operator: 'is', value: 1 },
          { attribute: 'b', operator: 'isNot', value: 'x' },
          { attribute: 'c', operator: 'oneOf', values: ['p', 'q'] },
          { attribute: 'd', operator: 'notOneOf', values: [1, 2] },
          { attribute: 'e', operator: 'exists' },
          { attribute: 'f', operator: 'notExists' },
          { attribute: 'g', operator: 'lt', value: 1 },
          { attribute: 'h', operator: 'lte', value: 2 },
          { attribute: 'i', operator: 'gt', value: 3 },
          { attribute: 'j', operator: 'gte', value: 4 },
          { attribute: '$type', operator: 'oneOf', values: ['Point', 'LineString'] },
          { attribute: '$type', operator: 'isNot', value: 'Polygon' },
        ],
        mode: 'structured',
      }),
    ).toMatchInlineSnapshot(`
      [
        "all",
        [
          "==",
          [
            "get",
            "a",
          ],
          1,
        ],
        [
          "!=",
          [
            "get",
            "b",
          ],
          "x",
        ],
        [
          "match",
          [
            "get",
            "c",
          ],
          [
            "p",
            "q",
          ],
          true,
          false,
        ],
        [
          "match",
          [
            "get",
            "d",
          ],
          [
            1,
            2,
          ],
          false,
          true,
        ],
        [
          "has",
          "e",
        ],
        [
          "!",
          [
            "has",
            "f",
          ],
        ],
        [
          "<",
          [
            "get",
            "g",
          ],
          1,
        ],
        [
          "<=",
          [
            "get",
            "h",
          ],
          2,
        ],
        [
          ">",
          [
            "get",
            "i",
          ],
          3,
        ],
        [
          ">=",
          [
            "get",
            "j",
          ],
          4,
        ],
        [
          "match",
          [
            "geometry-type",
          ],
          [
            "Point",
            "LineString",
          ],
          true,
          false,
        ],
        [
          "!=",
          [
            "geometry-type",
          ],
          "Polygon",
        ],
      ]
    `);
  });

  it('serializes a list with mixed types through in', () => {
    expect(
      serializeFilter({
        conditions: [{ attribute: 'a', operator: 'notOneOf', values: [true, 'x'] }],
        mode: 'structured',
      }),
    ).toEqual(['!', ['in', ['get', 'a'], ['literal', [true, 'x']]]]);
  });

  it('leaves out rows without an attribute or without values', () => {
    expect(
      serializeFilter({
        conditions: [
          { attribute: '', operator: 'exists' },
          { attribute: 'a', operator: 'oneOf', values: [] },
          { attribute: 'b', operator: 'exists' },
        ],
        mode: 'structured',
      }),
    ).toEqual(['has', 'b']);
  });

  it('returns an expression filter unchanged', () => {
    const filter = ['any', ['has', 'a'], ['has', 'b']];
    expect(serializeFilter({ expression: filter, mode: 'expression' })).toEqual(filter);
  });
});

describe('round trips', () => {
  it('turns a legacy filter into the equivalent modern one that parses back to the same conditions', () => {
    const legacy = ['all', ['==', '$type', 'Polygon'], ['in', 'class', 'residential', 'suburb']];
    const modern = serializeFilter(parseFilter(legacy));
    expect(modern).toEqual([
      'all',
      ['==', ['geometry-type'], 'Polygon'],
      ['match', ['get', 'class'], ['residential', 'suburb'], true, false],
    ]);
    expect(parseFilter(modern)).toEqual(parseFilter(legacy));
  });

  it('reads the Liberty bridge filter as conditions and writes it back unchanged', () => {
    const filter = [
      'all',
      ['match', ['get', 'brunnel'], ['bridge', 'tunnel'], false, true],
      ['==', ['get', 'class'], 'motorway'],
    ];
    expect(parseFilter(filter)).toEqual({
      conditions: [
        { attribute: 'brunnel', operator: 'notOneOf', values: ['bridge', 'tunnel'] },
        { attribute: 'class', operator: 'is', value: 'motorway' },
      ],
      mode: 'structured',
    });
    expect(serializeFilter(parseFilter(filter))).toEqual(filter);
  });

  it('writes a Liberty filter with a coalesce back unchanged', () => {
    const filter = [
      'all',
      ['==', ['get', 'class'], 'path'],
      ['!=', ['coalesce', ['get', 'brunnel'], ''], 'tunnel'],
    ];
    expect(serializeFilter(parseFilter(filter))).toEqual(filter);
    expect(parseFilter(filter).mode).toBe('expression');
  });
});

describe('validateFilter', () => {
  it('accepts a modern filter', () => {
    expect(validateFilter(['==', ['get', 'class'], 'motorway'])).toEqual([]);
  });

  it('accepts a legacy filter after conversion', () => {
    expect(validateFilter(['all', ['==', '$type', 'Polygon'], ['in', 'class', 'a', 'b']])).toEqual(
      [],
    );
  });

  it('accepts a missing filter', () => {
    expect(validateFilter(undefined)).toEqual([]);
  });

  it('rejects a filter that is not boolean', () => {
    expect(validateFilter(['+', 1, 2])).toMatchInlineSnapshot(`
      [
        "Expected boolean but found number instead.",
      ]
    `);
  });

  it('rejects an unknown operator', () => {
    expect(validateFilter(['frobnicate', 1])).toMatchInlineSnapshot(`
      [
        "[0]: Unknown expression "frobnicate". If you wanted a literal array, use ["literal", [...]].",
      ]
    `);
  });
});

describe('operatorAllowed', () => {
  it('forbids ordering on the geometry type', () => {
    expect(operatorAllowed('$type', 'lt')).toBe(false);
  });

  it('forbids a presence test on the geometry type', () => {
    expect(operatorAllowed('$type', 'exists')).toBe(false);
  });

  it('allows ordering on an attribute', () => {
    expect(operatorAllowed('rank', 'gte')).toBe(true);
  });
});

describe('coerceValue', () => {
  const attribute = (
    kind: 'string' | 'number' | 'boolean' | 'mixed',
    values: (string | number)[],
  ) => ({
    kind,
    name: 'a',
    values,
  });

  it('reads digits as a number for a numeric attribute', () => {
    expect(coerceValue('12', attribute('number', []))).toBe(12);
  });

  it('keeps non-digits as text for a numeric attribute', () => {
    expect(coerceValue('twelve', attribute('number', []))).toBe('twelve');
  });

  it('reads true as a boolean for a boolean attribute', () => {
    expect(coerceValue('true', attribute('boolean', []))).toBe(true);
  });

  it('keeps digits as text for a string attribute', () => {
    expect(coerceValue('12', attribute('string', ['12']))).toBe('12');
  });

  it('reads digits as a number when a number was observed for a mixed attribute', () => {
    expect(coerceValue('3', attribute('mixed', [3, 'x']))).toBe(3);
  });

  it('keeps text when the attribute is unknown', () => {
    expect(coerceValue('3', undefined)).toBe('3');
  });
});

describe('changeOperator', () => {
  const oneOf: FilterCondition = { attribute: 'a', operator: 'oneOf', values: ['x', 'y'] };

  it('keeps the first value when going from a list to a single value', () => {
    expect(changeOperator(oneOf, 'is')).toEqual({ attribute: 'a', operator: 'is', value: 'x' });
  });

  it('starts an ordering at zero when the value is not a number', () => {
    expect(changeOperator(oneOf, 'gt')).toEqual({ attribute: 'a', operator: 'gt', value: 0 });
  });

  it('keeps a single value as a one element list', () => {
    expect(changeOperator({ attribute: 'a', operator: 'is', value: 'x' }, 'notOneOf')).toEqual({
      attribute: 'a',
      operator: 'notOneOf',
      values: ['x'],
    });
  });

  it('drops values when the attribute only needs to exist', () => {
    expect(changeOperator(oneOf, 'exists')).toEqual({ attribute: 'a', operator: 'exists' });
  });
});
