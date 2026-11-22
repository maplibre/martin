import { describe, expect, it } from 'vitest';
import { propertySpec } from '@/components/style-editor/style-spec';
import {
  allowedModes,
  type CurveValue,
  convertValue,
  evaluateCurve,
  initialValue,
  parseValue,
  serializeValue,
} from '@/components/style-editor/value-model';

const lineColor = () => {
  const spec = propertySpec('line', 'paint', 'line-color');
  if (!spec) throw new Error('line-color missing');
  return spec;
};
const lineWidth = () => {
  const spec = propertySpec('line', 'paint', 'line-width');
  if (!spec) throw new Error('line-width missing');
  return spec;
};

describe('parseValue', () => {
  it('parses a string into a constant', () => {
    expect(parseValue('#ff0000', lineColor())).toMatchInlineSnapshot(`
      {
        "mode": "constant",
        "value": "#ff0000",
      }
    `);
  });

  it('parses a number into a constant', () => {
    expect(parseValue(3, lineWidth())).toMatchInlineSnapshot(`
      {
        "mode": "constant",
        "value": 3,
      }
    `);
  });

  it('unwraps a literal array into a constant', () => {
    const spec = propertySpec('line', 'paint', 'line-dasharray');
    if (!spec) throw new Error('missing');
    expect(parseValue(['literal', [2, 1]], spec)).toMatchInlineSnapshot(`
      {
        "mode": "constant",
        "value": [
          2,
          1,
        ],
      }
    `);
  });

  it('parses a zoom interpolate into stops', () => {
    expect(
      parseValue(['interpolate', ['linear'], ['zoom'], 5, 1, 10, 4], lineWidth()),
    ).toMatchInlineSnapshot(`
      {
        "attribute": undefined,
        "interpolation": {
          "kind": "linear",
        },
        "mode": "zoom",
        "stops": [
          {
            "input": 5,
            "output": 1,
          },
          {
            "input": 10,
            "output": 4,
          },
        ],
      }
    `);
  });

  it('parses an exponential zoom interpolate with its base', () => {
    expect(
      parseValue(['interpolate', ['exponential', 1.5], ['zoom'], 5, 1, 10, 4], lineWidth()),
    ).toMatchInlineSnapshot(`
      {
        "attribute": undefined,
        "interpolation": {
          "base": 1.5,
          "kind": "exponential",
        },
        "mode": "zoom",
        "stops": [
          {
            "input": 5,
            "output": 1,
          },
          {
            "input": 10,
            "output": 4,
          },
        ],
      }
    `);
  });

  it('parses a zoom step with a first stop at minus infinity', () => {
    expect(parseValue(['step', ['zoom'], 1, 8, 2, 12, 5], lineWidth())).toMatchInlineSnapshot(`
      {
        "attribute": undefined,
        "interpolation": {
          "kind": "step",
        },
        "mode": "zoom",
        "stops": [
          {
            "input": -Infinity,
            "output": 1,
          },
          {
            "input": 8,
            "output": 2,
          },
          {
            "input": 12,
            "output": 5,
          },
        ],
      }
    `);
  });

  it('parses an attribute interpolate into a ramp', () => {
    expect(
      parseValue(
        ['interpolate', ['linear'], ['get', 'height'], 0, '#000000', 100, '#ffffff'],
        lineColor(),
      ),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "height",
        "interpolation": {
          "kind": "linear",
        },
        "mode": "ramp",
        "stops": [
          {
            "input": 0,
            "output": "#000000",
          },
          {
            "input": 100,
            "output": "#ffffff",
          },
        ],
      }
    `);
  });

  it('parses a match with single labels', () => {
    expect(
      parseValue(
        ['match', ['get', 'class'], 'motorway', '#ff0000', 'trunk', '#00ff00', '#0000ff'],
        lineColor(),
      ),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "class",
        "cases": [
          {
            "labels": [
              "motorway",
            ],
            "output": "#ff0000",
          },
          {
            "labels": [
              "trunk",
            ],
            "output": "#00ff00",
          },
        ],
        "fallback": "#0000ff",
        "mode": "match",
      }
    `);
  });

  it('parses a match with an array label', () => {
    expect(
      parseValue(
        ['match', ['get', 'class'], ['motorway', 'trunk'], '#ff0000', '#0000ff'],
        lineColor(),
      ),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "class",
        "cases": [
          {
            "labels": [
              "motorway",
              "trunk",
            ],
            "output": "#ff0000",
          },
        ],
        "fallback": "#0000ff",
        "mode": "match",
      }
    `);
  });

  it('migrates a legacy zoom function into stops', () => {
    expect(
      parseValue(
        {
          base: 1,
          stops: [
            [5, 1],
            [10, 4],
          ],
        },
        lineWidth(),
      ),
    ).toMatchInlineSnapshot(`
      {
        "attribute": undefined,
        "interpolation": {
          "kind": "linear",
        },
        "mode": "zoom",
        "stops": [
          {
            "input": 5,
            "output": 1,
          },
          {
            "input": 10,
            "output": 4,
          },
        ],
      }
    `);
  });

  it('migrates a legacy categorical function into a match', () => {
    expect(
      parseValue(
        {
          property: 'class',
          stops: [
            ['motorway', '#ff0000'],
            ['trunk', '#00ff00'],
          ],
          type: 'categorical',
        },
        lineColor(),
      ),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "class",
        "cases": [
          {
            "labels": [
              "motorway",
            ],
            "output": "#ff0000",
          },
          {
            "labels": [
              "trunk",
            ],
            "output": "#00ff00",
          },
        ],
        "fallback": "#000000",
        "mode": "match",
      }
    `);
  });

  it('keeps an unknown expression verbatim', () => {
    expect(
      parseValue(['case', ['has', 'name'], '#ff0000', '#0000ff'], lineColor()),
    ).toMatchInlineSnapshot(`
      {
        "mode": "expression",
        "value": [
          "case",
          [
            "has",
            "name",
          ],
          "#ff0000",
          "#0000ff",
        ],
      }
    `);
  });

  it('keeps an interpolate with a non-literal stop output as an expression', () => {
    expect(
      parseValue(['interpolate', ['linear'], ['zoom'], 5, ['get', 'a'], 10, 4], lineWidth()),
    ).toMatchInlineSnapshot(`
      {
        "mode": "expression",
        "value": [
          "interpolate",
          [
            "linear",
          ],
          [
            "zoom",
          ],
          5,
          [
            "get",
            "a",
          ],
          10,
          4,
        ],
      }
    `);
  });
});

describe('serializeValue', () => {
  it('round-trips a constant', () => {
    const spec = lineColor();
    expect(serializeValue(parseValue('#ff0000', spec))).toEqual('#ff0000');
  });

  it('serializes a literal array constant as the bare array', () => {
    const spec = propertySpec('line', 'paint', 'line-dasharray');
    if (!spec) throw new Error('missing');
    expect(serializeValue(parseValue(['literal', [2, 1]], spec))).toEqual([2, 1]);
  });

  it('round-trips a linear zoom interpolate', () => {
    const input = ['interpolate', ['linear'], ['zoom'], 5, 1, 10, 4];
    expect(serializeValue(parseValue(input, lineWidth()))).toEqual(input);
  });

  it('round-trips an exponential zoom interpolate', () => {
    const input = ['interpolate', ['exponential', 1.5], ['zoom'], 5, 1, 10, 4];
    expect(serializeValue(parseValue(input, lineWidth()))).toEqual(input);
  });

  it('round-trips a zoom step', () => {
    const input = ['step', ['zoom'], 1, 8, 2, 12, 5];
    expect(serializeValue(parseValue(input, lineWidth()))).toEqual(input);
  });

  it('round-trips an attribute ramp', () => {
    const input = ['interpolate', ['linear'], ['get', 'height'], 0, '#000000', 100, '#ffffff'];
    expect(serializeValue(parseValue(input, lineColor()))).toEqual(input);
  });

  it('round-trips a match with single and array labels', () => {
    const input = [
      'match',
      ['get', 'class'],
      'motorway',
      '#ff0000',
      ['trunk', 'primary'],
      '#00ff00',
      '#0000ff',
    ];
    expect(serializeValue(parseValue(input, lineColor()))).toEqual(input);
  });

  it('round-trips a match with a literal array output', () => {
    const spec = propertySpec('line', 'paint', 'line-dasharray');
    if (!spec) throw new Error('missing');
    const input = ['match', ['get', 'class'], 'a', ['literal', [1, 2]], ['literal', [3, 4]]];
    expect(serializeValue(parseValue(input, spec))).toEqual(input);
  });

  it('round-trips a legacy zoom function as its migrated expression', () => {
    expect(
      serializeValue(
        parseValue(
          {
            base: 1,
            stops: [
              [5, 1],
              [10, 4],
            ],
          },
          lineWidth(),
        ),
      ),
    ).toMatchInlineSnapshot(`
      [
        "interpolate",
        [
          "linear",
        ],
        [
          "zoom",
        ],
        5,
        1,
        10,
        4,
      ]
    `);
  });

  it('round-trips a legacy categorical function as its migrated match', () => {
    expect(
      serializeValue(
        parseValue(
          {
            property: 'class',
            stops: [
              ['motorway', '#ff0000'],
              ['trunk', '#00ff00'],
            ],
            type: 'categorical',
          },
          lineColor(),
        ),
      ),
    ).toMatchInlineSnapshot(`
      [
        "match",
        [
          "get",
          "class",
        ],
        "motorway",
        "#ff0000",
        "trunk",
        "#00ff00",
        "#000000",
      ]
    `);
  });

  it('round-trips an unknown expression', () => {
    const input = ['case', ['has', 'name'], '#ff0000', '#0000ff'];
    expect(serializeValue(parseValue(input, lineColor()))).toEqual(input);
  });
});

describe('initialValue', () => {
  it('uses the spec default when the property has one', () => {
    expect(initialValue(lineColor())).toBe('#000000');
  });

  it('starts properties without a default at the simplest valid value of their type', () => {
    expect(
      ['icon-image', 'text-field', 'icon-offset', 'symbol-sort-key'].map((name) =>
        initialValue(propertySpec('symbol', 'layout', name) ?? lineColor()),
      ),
    ).toMatchInlineSnapshot(`
      [
        "",
        "",
        [
          0,
          0,
        ],
        0,
      ]
    `);
  });
});

describe('convertValue', () => {
  it('keeps the first literal output when switching an expression to a fixed icon', () => {
    expect(
      convertValue(
        {
          mode: 'expression',
          value: ['match', ['get', 'class'], 'bus', ['get', 'subclass'], 'airport_11'],
        },
        'constant',
        { spec: propertySpec('symbol', 'layout', 'icon-image') ?? lineColor() },
      ),
    ).toEqual({ mode: 'constant', value: 'airport_11' });
  });

  it('falls back to an empty icon name when an expression has no literal output', () => {
    expect(
      convertValue({ mode: 'expression', value: ['get', 'icon'] }, 'constant', {
        spec: propertySpec('symbol', 'layout', 'icon-image') ?? lineColor(),
      }),
    ).toEqual({ mode: 'constant', value: '' });
  });

  it('turns a zoom curve into a constant holding its last output', () => {
    expect(
      convertValue(
        parseValue(['interpolate', ['linear'], ['zoom'], 5, 1, 10, 4], lineWidth()),
        'constant',
        {
          spec: lineWidth(),
        },
      ),
    ).toEqual({ mode: 'constant', value: 4 });
  });

  it('turns a constant into a linear zoom curve with the same output', () => {
    expect(
      convertValue({ mode: 'constant', value: 3 }, 'zoom', { spec: lineWidth() }),
    ).toMatchInlineSnapshot(`
      {
        "interpolation": {
          "kind": "linear",
        },
        "mode": "zoom",
        "stops": [
          {
            "input": 6,
            "output": 3,
          },
          {
            "input": 16,
            "output": 3,
          },
        ],
      }
    `);
  });

  it('turns a constant into a step zoom curve when the property is not interpolated', () => {
    const spec = propertySpec('line', 'layout', 'line-cap');
    if (!spec) throw new Error('missing');
    expect(
      convertValue({ mode: 'constant', value: 'round' }, 'zoom', { spec }),
    ).toMatchInlineSnapshot(`
      {
        "interpolation": {
          "kind": "step",
        },
        "mode": "zoom",
        "stops": [
          {
            "input": -Infinity,
            "output": "round",
          },
          {
            "input": 12,
            "output": "round",
          },
        ],
      }
    `);
  });

  it('turns a constant into a ramp over the sampled range', () => {
    expect(
      convertValue({ mode: 'constant', value: '#ff0000' }, 'ramp', {
        attribute: 'height',
        sampleRange: [10, 50],
        spec: lineColor(),
      }),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "height",
        "interpolation": {
          "kind": "linear",
        },
        "mode": "ramp",
        "stops": [
          {
            "input": 10,
            "output": "#ff0000",
          },
          {
            "input": 50,
            "output": "#ff0000",
          },
        ],
      }
    `);
  });

  it('turns a constant into a ramp over 0 to 100 without a sampled range', () => {
    expect(
      convertValue({ mode: 'constant', value: 3 }, 'ramp', { attribute: 'h', spec: lineWidth() }),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "h",
        "interpolation": {
          "kind": "linear",
        },
        "mode": "ramp",
        "stops": [
          {
            "input": 0,
            "output": 3,
          },
          {
            "input": 100,
            "output": 3,
          },
        ],
      }
    `);
  });

  it('turns a color constant into a match with a distinct color per sample label', () => {
    expect(
      convertValue({ mode: 'constant', value: '#ff0000' }, 'match', {
        attribute: 'class',
        sampleLabels: ['a', 'b', 'c'],
        spec: lineColor(),
      }),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "class",
        "cases": [
          {
            "labels": [
              "a",
            ],
            "output": "#7b9900",
          },
          {
            "labels": [
              "b",
            ],
            "output": "#00a9db",
          },
          {
            "labels": [
              "c",
            ],
            "output": "#a34fff",
          },
        ],
        "fallback": "#ff0000",
        "mode": "match",
      }
    `);
  });

  it('turns a number constant into a match that repeats the value for each sample label', () => {
    expect(
      convertValue({ mode: 'constant', value: 2 }, 'match', {
        attribute: 'class',
        sampleLabels: ['a', 'b'],
        spec: lineWidth(),
      }),
    ).toMatchInlineSnapshot(`
      {
        "attribute": "class",
        "cases": [
          {
            "labels": [
              "a",
            ],
            "output": 2,
          },
          {
            "labels": [
              "b",
            ],
            "output": 2,
          },
        ],
        "fallback": 2,
        "mode": "match",
      }
    `);
  });

  it('keeps the fallback of a match when converting to a constant', () => {
    expect(
      convertValue(
        {
          attribute: 'class',
          cases: [{ labels: ['a'], output: '#111111' }],
          fallback: '#222222',
          mode: 'match',
        },
        'constant',
        { spec: lineColor() },
      ),
    ).toEqual({ mode: 'constant', value: '#222222' });
  });

  it('keeps the serialized value when converting to an expression', () => {
    expect(
      convertValue(
        parseValue(['interpolate', ['linear'], ['zoom'], 5, 1, 10, 4], lineWidth()),
        'expression',
        {
          spec: lineWidth(),
        },
      ),
    ).toEqual({ mode: 'expression', value: ['interpolate', ['linear'], ['zoom'], 5, 1, 10, 4] });
  });

  it('falls back to the spec default when a constant is unset', () => {
    expect(
      convertValue({ mode: 'constant', value: undefined }, 'zoom', { spec: lineWidth() }),
    ).toMatchInlineSnapshot(`
      {
        "interpolation": {
          "kind": "linear",
        },
        "mode": "zoom",
        "stops": [
          {
            "input": 6,
            "output": 1,
          },
          {
            "input": 16,
            "output": 1,
          },
        ],
      }
    `);
  });
});

describe('allowedModes', () => {
  it('offers every mode for line-color', () => {
    expect(allowedModes(lineColor())).toMatchInlineSnapshot(`
      [
        "constant",
        "zoom",
        "match",
        "ramp",
        "expression",
      ]
    `);
  });

  it('offers constant, zoom, match and expression for line-cap', () => {
    const spec = propertySpec('line', 'layout', 'line-cap');
    if (!spec) throw new Error('missing');
    expect(allowedModes(spec)).toMatchInlineSnapshot(`
      [
        "constant",
        "zoom",
        "match",
        "expression",
      ]
    `);
  });

  it('offers constant, zoom, match and expression for text-field', () => {
    const spec = propertySpec('symbol', 'layout', 'text-field');
    if (!spec) throw new Error('missing');
    expect(allowedModes(spec)).toMatchInlineSnapshot(`
      [
        "constant",
        "zoom",
        "match",
        "expression",
      ]
    `);
  });

  it('offers constant, zoom and expression for background-color', () => {
    const spec = propertySpec('background', 'paint', 'background-color');
    if (!spec) throw new Error('missing');
    expect(allowedModes(spec)).toMatchInlineSnapshot(`
      [
        "constant",
        "zoom",
        "expression",
      ]
    `);
  });
});

describe('evaluateCurve', () => {
  it('interpolates linearly between stops', () => {
    const curve: CurveValue = {
      interpolation: { kind: 'linear' },
      mode: 'zoom',
      stops: [
        { input: 0, output: 0 },
        { input: 10, output: 100 },
      ],
    };
    expect([-1, 0, 2.5, 10, 20].map((z) => evaluateCurve(curve, z))).toEqual([0, 0, 25, 100, 100]);
  });

  it('interpolates exponentially with the base', () => {
    const curve: CurveValue = {
      interpolation: { base: 2, kind: 'exponential' },
      mode: 'zoom',
      stops: [
        { input: 0, output: 0 },
        { input: 2, output: 3 },
      ],
    };
    expect(evaluateCurve(curve, 1)).toBe(1);
  });

  it('holds the previous output between steps', () => {
    const curve: CurveValue = {
      interpolation: { kind: 'step' },
      mode: 'zoom',
      stops: [
        { input: Number.NEGATIVE_INFINITY, output: 1 },
        { input: 8, output: 2 },
        { input: 12, output: 5 },
      ],
    };
    expect([0, 7.9, 8, 11, 12, 20].map((z) => evaluateCurve(curve, z))).toEqual([1, 1, 2, 2, 5, 5]);
  });

  it('returns undefined for non-numeric outputs', () => {
    const curve: CurveValue = {
      interpolation: { kind: 'linear' },
      mode: 'zoom',
      stops: [
        { input: 0, output: '#000000' },
        { input: 10, output: '#ffffff' },
      ],
    };
    expect(evaluateCurve(curve, 5)).toBeUndefined();
  });

  it('returns undefined without stops', () => {
    expect(
      evaluateCurve({ interpolation: { kind: 'linear' }, mode: 'zoom', stops: [] }, 5),
    ).toBeUndefined();
  });
});
