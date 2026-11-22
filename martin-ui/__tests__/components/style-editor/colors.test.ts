import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import {
  Color,
  colorKey,
  colorSites,
  contrastRatio,
  formatColor,
  writeColorSites,
} from '@/components/style-editor/colors';

describe('colorKey', () => {
  it('writes an opaque color as six hex digits', () => {
    expect(colorKey(Color.parse('rgb(255, 128, 0)') as Color)).toBe('#ff8000');
  });

  it('writes a translucent color as eight hex digits', () => {
    expect(colorKey(Color.parse('rgba(255, 0, 0, 0.5)') as Color)).toBe('#ff000080');
  });
});

describe('formatColor', () => {
  it('writes an opaque color as hex', () => {
    expect(formatColor(Color.parse('red') as Color)).toBe('#ff0000');
  });

  it('writes a translucent color as rgba', () => {
    expect(formatColor(Color.parse('rgba(255, 255, 255, 0.75)') as Color)).toBe(
      'rgba(255,255,255,0.75)',
    );
  });
});

describe('contrastRatio', () => {
  it('is 21 for black on white', () => {
    expect(contrastRatio(Color.black, Color.white)).toBeCloseTo(21, 5);
  });

  it('is 1 for identical colors', () => {
    expect(
      contrastRatio(Color.parse('#336699') as Color, Color.parse('#336699') as Color),
    ).toBeCloseTo(1, 5);
  });

  it('is symmetric', () => {
    const a = Color.parse('#336699') as Color;
    expect(contrastRatio(a, Color.white)).toBeCloseTo(contrastRatio(Color.white, a), 10);
  });
});

describe('colorSites', () => {
  it('finds a plain literal color', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'bg', paint: { 'background-color': '#ff0000' }, type: 'background' }],
      sources: {},
      version: 8,
    };
    expect(colorSites(style)).toMatchInlineSnapshot(`
      [
        {
          "color": Color {
            "a": 1,
            "b": 0,
            "g": 0,
            "r": 1,
          },
          "group": "paint",
          "key": "#ff0000",
          "layer": "bg",
          "name": "background-color",
          "path": [],
        },
      ]
    `);
  });

  it('finds match outputs but not match labels that are color names', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'roads',
          paint: {
            'line-color': ['match', ['get', 'class'], 'red', '#00f', 'green', '#0f0', '#000'],
          },
          source: 's',
          type: 'line',
        },
      ],
      sources: {},
      version: 8,
    };
    expect(colorSites(style).map((s) => ({ key: s.key, path: s.path }))).toMatchInlineSnapshot(`
      [
        {
          "key": "#0000ff",
          "path": [
            3,
          ],
        },
        {
          "key": "#00ff00",
          "path": [
            5,
          ],
        },
        {
          "key": "#000000",
          "path": [
            6,
          ],
        },
      ]
    `);
  });

  it('finds interpolate outputs but not stop inputs', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'roads',
          paint: {
            'line-color': ['interpolate', ['linear'], ['zoom'], 5, '#ff0000', 10, '#0000ff'],
          },
          source: 's',
          type: 'line',
        },
      ],
      sources: {},
      version: 8,
    };
    expect(colorSites(style).map((s) => ({ key: s.key, path: s.path }))).toMatchInlineSnapshot(`
      [
        {
          "key": "#ff0000",
          "path": [
            4,
          ],
        },
        {
          "key": "#0000ff",
          "path": [
            6,
          ],
        },
      ]
    `);
  });

  it('finds colors in a legacy stops function', () => {
    const style = {
      layers: [
        {
          id: 'roads',
          paint: {
            'line-color': {
              stops: [
                [5, '#ff0000'],
                [10, '#0000ff'],
              ],
            },
          },
          source: 's',
          type: 'line',
        },
      ],
      sources: {},
      version: 8,
    } as unknown as StyleSpecification;
    expect(colorSites(style).map((s) => ({ key: s.key, path: s.path }))).toMatchInlineSnapshot(`
      [
        {
          "key": "#ff0000",
          "path": [
            "stops",
            0,
            1,
          ],
        },
        {
          "key": "#0000ff",
          "path": [
            "stops",
            1,
            1,
          ],
        },
      ]
    `);
  });

  it('ignores non-color properties', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'roads', paint: { 'line-width': 3 }, source: 's', type: 'line' }],
      sources: {},
      version: 8,
    };
    expect(colorSites(style)).toEqual([]);
  });
});

describe('colors bound with let', () => {
  it('finds a color bound with let through its var and rewrites the binding', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'water',
          paint: { 'fill-color': ['let', 'c', '#ff0000', ['var', 'c']] },
          source: 'omt',
          'source-layer': 'water',
          type: 'fill',
        },
      ],
      sources: {},
      version: 8,
    };

    const sites = colorSites(style);
    writeColorSites(style, sites, () => Color.parse('#0000ff') ?? Color.red);

    expect(sites.map((s) => [s.key, s.path])).toEqual([['#ff0000', [2]]]);
    expect(style.layers[0]).toMatchObject({
      paint: { 'fill-color': ['let', 'c', '#0000ff', ['var', 'c']] },
    });
  });

  it('reports a binding used twice once', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'water',
          paint: {
            'fill-color': [
              'let',
              'c',
              '#ff0000',
              ['case', ['has', 'deep'], ['var', 'c'], ['var', 'c']],
            ],
          },
          source: 'omt',
          'source-layer': 'water',
          type: 'fill',
        },
      ],
      sources: {},
      version: 8,
    };

    expect(colorSites(style).map((s) => s.path)).toEqual([[2]]);
  });
});

describe('writeColorSites', () => {
  it('rewrites only the output colors of a match, leaving labels', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'roads',
          paint: {
            'line-color': ['match', ['get', 'class'], 'red', '#00f', 'green', '#0f0', '#000'],
            'line-width': 2,
          },
          source: 's',
          type: 'line',
        },
      ],
      sources: {},
      version: 8,
    };
    writeColorSites(style, colorSites(style), () => Color.parse('#123456') as Color);
    expect(style).toMatchInlineSnapshot(`
      {
        "layers": [
          {
            "id": "roads",
            "paint": {
              "line-color": [
                "match",
                [
                  "get",
                  "class",
                ],
                "red",
                "#123456",
                "green",
                "#123456",
                "#123456",
              ],
              "line-width": 2,
            },
            "source": "s",
            "type": "line",
          },
        ],
        "sources": {},
        "version": 8,
      }
    `);
  });

  it('rewrites only the sites passed in', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', paint: { 'background-color': '#ff0000' }, type: 'background' },
        { id: 'b', paint: { 'line-color': '#ff0000' }, source: 's', type: 'line' },
      ],
      sources: {},
      version: 8,
    };
    writeColorSites(
      style,
      colorSites(style).filter((s) => s.layer === 'b'),
      () => Color.parse('#00ff00') as Color,
    );
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "paint": {
            "background-color": "#ff0000",
          },
          "type": "background",
        },
        {
          "id": "b",
          "paint": {
            "line-color": "#00ff00",
          },
          "source": "s",
          "type": "line",
        },
      ]
    `);
  });

  it('writes a translucent replacement as rgba', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', paint: { 'background-color': '#ff0000' }, type: 'background' }],
      sources: {},
      version: 8,
    };
    writeColorSites(style, colorSites(style), () => Color.parse('rgba(0, 0, 255, 0.5)') as Color);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "paint": {
            "background-color": "rgba(0,0,255,0.5)",
          },
          "type": "background",
        },
      ]
    `);
  });
});
