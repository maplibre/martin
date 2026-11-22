import { describe, expect, it } from 'vitest';
import {
  type PropertySpec,
  propertySpec,
  validateProperty,
} from '@/components/style-editor/style-spec';

const spec = (type: 'line' | 'fill' | 'symbol', group: 'paint' | 'layout', name: string) => {
  const s = propertySpec(type, group, name);
  if (!s) throw new Error(`${name} missing`);
  return s;
};

describe('validateProperty', () => {
  it('accepts a literal dasharray written as a bare array', () => {
    expect(
      validateProperty(
        [3, 3],
        'line-dasharray',
        propertySpec('line', 'paint', 'line-dasharray') as PropertySpec,
      ),
    ).toEqual([]);
  });

  it('accepts a literal font stack written as a bare array', () => {
    expect(
      validateProperty(
        ['Noto Sans Regular'],
        'text-font',
        propertySpec('symbol', 'layout', 'text-font') as PropertySpec,
      ),
    ).toEqual([]);
  });

  it('rejects a bare dasharray that holds strings', () => {
    expect(
      validateProperty(
        ['a', 'b'],
        'line-dasharray',
        propertySpec('line', 'paint', 'line-dasharray') as PropertySpec,
      ),
    ).toMatchInlineSnapshot(`
      [
        "Expected array<number> but found array<string, 2> instead.",
      ]
    `);
  });

  it('rejects a text offset with one element instead of two', () => {
    expect(
      validateProperty(
        [1],
        'text-offset',
        propertySpec('symbol', 'layout', 'text-offset') as PropertySpec,
      ),
    ).toMatchInlineSnapshot(`
      [
        "Expected array<number, 2> but found array<number, 1> instead.",
      ]
    `);
  });

  it('accepts a valid color literal', () => {
    expect(validateProperty('#ff0000', 'line-color', spec('line', 'paint', 'line-color'))).toEqual(
      [],
    );
  });

  it('accepts a valid number literal', () => {
    expect(validateProperty(3, 'line-width', spec('line', 'paint', 'line-width'))).toEqual([]);
  });

  it('accepts an unset value', () => {
    expect(validateProperty(undefined, 'line-width', spec('line', 'paint', 'line-width'))).toEqual(
      [],
    );
  });

  it('accepts a zoom interpolate', () => {
    expect(
      validateProperty(
        ['interpolate', ['linear'], ['zoom'], 5, 1, 10, 4],
        'line-width',
        spec('line', 'paint', 'line-width'),
      ),
    ).toEqual([]);
  });

  it('accepts a match expression', () => {
    expect(
      validateProperty(
        ['match', ['get', 'class'], 'motorway', '#ff0000', '#0000ff'],
        'line-color',
        spec('line', 'paint', 'line-color'),
      ),
    ).toEqual([]);
  });

  it('accepts a legacy stops function', () => {
    expect(
      validateProperty(
        {
          stops: [
            [5, 1],
            [10, 4],
          ],
        },
        'line-width',
        spec('line', 'paint', 'line-width'),
      ),
    ).toEqual([]);
  });

  it('accepts a legacy categorical function', () => {
    expect(
      validateProperty(
        {
          property: 'class',
          stops: [
            ['motorway', '#ff0000'],
            ['trunk', '#00ff00'],
          ],
          type: 'categorical',
        },
        'line-color',
        spec('line', 'paint', 'line-color'),
      ),
    ).toEqual([]);
  });

  it('rejects an invalid color with a readable message', () => {
    expect(
      validateProperty('not-a-color', 'line-color', spec('line', 'paint', 'line-color')),
    ).toMatchInlineSnapshot(`
      [
        "Could not parse color from value 'not-a-color'",
      ]
    `);
  });

  it('rejects a string where a number is expected', () => {
    expect(
      validateProperty('wide', 'line-width', spec('line', 'paint', 'line-width')),
    ).toMatchInlineSnapshot(`
      [
        "Expected number but found string instead.",
      ]
    `);
  });

  it('rejects an unknown enum value', () => {
    expect(validateProperty('pointy', 'line-cap', spec('line', 'layout', 'line-cap'))).toHaveLength(
      1,
    );
  });

  it('rejects a malformed expression', () => {
    expect(
      validateProperty(
        ['interpolate', ['linear'], ['zoom'], 5],
        'line-width',
        spec('line', 'paint', 'line-width'),
      ),
    ).toMatchInlineSnapshot(`
      [
        "Expected at least 4 arguments, but found only 3.",
      ]
    `);
  });

  it('rejects a data expression on a property that does not accept data', () => {
    expect(
      validateProperty(['get', 'width'], 'fill-antialias', spec('fill', 'paint', 'fill-antialias')),
    ).toMatchInlineSnapshot(`
      [
        "data expressions not supported",
      ]
    `);
  });
});
