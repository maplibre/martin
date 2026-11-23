import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import {
  deleteLayer,
  duplicateLayer,
  moveLayer,
  renameLayer,
  setProperty,
  setVisibility,
  setZoomRange,
  uniqueLayerId,
} from '@/components/style-editor/operations';

describe('setProperty', () => {
  it('sets a paint property', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    setProperty(style, { group: 'paint', layer: 'a', name: 'background-color' }, '#ff0000');
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "paint": {
            "background-color": "#ff0000",
          },
          "type": "background",
        },
      ]
    `);
  });

  it('replaces an existing value and keeps its siblings', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'a',
          paint: { 'background-color': '#ff0000', 'background-opacity': 0.5 },
          type: 'background',
        },
      ],
      sources: {},
      version: 8,
    };
    setProperty(style, { group: 'paint', layer: 'a', name: 'background-color' }, '#00ff00');
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "paint": {
            "background-color": "#00ff00",
            "background-opacity": 0.5,
          },
          "type": "background",
        },
      ]
    `);
  });

  it('removes a property set to undefined and keeps its siblings', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'a',
          paint: { 'background-color': '#ff0000', 'background-opacity': 0.5 },
          type: 'background',
        },
      ],
      sources: {},
      version: 8,
    };
    setProperty(style, { group: 'paint', layer: 'a', name: 'background-color' }, undefined);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "paint": {
            "background-opacity": 0.5,
          },
          "type": "background",
        },
      ]
    `);
  });

  it('removes the group when the last property is unset', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', paint: { 'background-color': '#ff0000' }, type: 'background' }],
      sources: {},
      version: 8,
    };
    setProperty(style, { group: 'paint', layer: 'a', name: 'background-color' }, undefined);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "type": "background",
        },
      ]
    `);
  });

  it('does not create an empty group when unsetting a missing property', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    setProperty(style, { group: 'layout', layer: 'a', name: 'visibility' }, undefined);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "type": "background",
        },
      ]
    `);
  });

  it('ignores an unknown layer', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    setProperty(style, { group: 'paint', layer: 'zzz', name: 'background-color' }, '#ff0000');
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "type": "background",
        },
      ]
    `);
  });
});

describe('setVisibility', () => {
  it('hides layers with layout visibility none', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
        { id: 'c', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    setVisibility(style, ['a', 'c'], false);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "layout": {
            "visibility": "none",
          },
          "type": "background",
        },
        {
          "id": "b",
          "type": "background",
        },
        {
          "id": "c",
          "layout": {
            "visibility": "none",
          },
          "type": "background",
        },
      ]
    `);
  });

  it('shows layers by removing the visibility key and the empty layout', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', layout: { visibility: 'none' }, type: 'background' }],
      sources: {},
      version: 8,
    };
    setVisibility(style, ['a'], true);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "type": "background",
        },
      ]
    `);
  });
});

describe('setZoomRange', () => {
  it('sets minzoom and maxzoom', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    setZoomRange(style, 'a', 3, 9);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "maxzoom": 9,
          "minzoom": 3,
          "type": "background",
        },
      ]
    `);
  });

  it('clears minzoom and maxzoom when undefined', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', maxzoom: 9, minzoom: 3, type: 'background' }],
      sources: {},
      version: 8,
    };
    setZoomRange(style, 'a', undefined, undefined);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "type": "background",
        },
      ]
    `);
  });

  it('clears one bound and keeps the other', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', maxzoom: 9, minzoom: 3, type: 'background' }],
      sources: {},
      version: 8,
    };
    setZoomRange(style, 'a', 4, undefined);
    expect(style.layers).toMatchInlineSnapshot(`
      [
        {
          "id": "a",
          "minzoom": 4,
          "type": "background",
        },
      ]
    `);
  });
});

describe('moveLayer', () => {
  it('moves a layer to the front', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
        { id: 'c', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    moveLayer(style, 'c', 0);
    expect(style.layers.map((l) => l.id)).toEqual(['c', 'a', 'b']);
  });

  it('moves a layer to the end', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
        { id: 'c', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    moveLayer(style, 'a', 2);
    expect(style.layers.map((l) => l.id)).toEqual(['b', 'c', 'a']);
  });

  it('clamps an index past the end', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    moveLayer(style, 'a', 99);
    expect(style.layers.map((l) => l.id)).toEqual(['b', 'a']);
  });

  it('clamps a negative index', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    moveLayer(style, 'b', -5);
    expect(style.layers.map((l) => l.id)).toEqual(['b', 'a']);
  });

  it('ignores an unknown layer', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    moveLayer(style, 'zzz', 0);
    expect(style.layers.map((l) => l.id)).toEqual(['a', 'b']);
  });
});

describe('uniqueLayerId', () => {
  it('returns the base when it is free', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    expect(uniqueLayerId(style, 'b')).toBe('b');
  });

  it('appends the first free number when the base is taken', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'a-2', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    expect(uniqueLayerId(style, 'a')).toBe('a-3');
  });
});

describe('duplicateLayer', () => {
  it('inserts a deep copy right above the original and returns its id', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', paint: { 'background-color': '#ff0000' }, type: 'background' },
        { id: 'b', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    const id = duplicateLayer(style, 'a');
    (style.layers[1] as unknown as { paint: Record<string, string> }).paint['background-color'] =
      '#00ff00';
    expect({ id, layers: style.layers }).toMatchInlineSnapshot(`
      {
        "id": "a-copy",
        "layers": [
          {
            "id": "a",
            "paint": {
              "background-color": "#ff0000",
            },
            "type": "background",
          },
          {
            "id": "a-copy",
            "paint": {
              "background-color": "#00ff00",
            },
            "type": "background",
          },
          {
            "id": "b",
            "type": "background",
          },
        ],
      }
    `);
  });

  it('picks unique ids when duplicating twice', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    const ids = [duplicateLayer(style, 'a'), duplicateLayer(style, 'a')];
    expect({ ids, order: style.layers.map((l) => l.id) }).toEqual({
      ids: ['a-copy', 'a-copy-2'],
      order: ['a', 'a-copy-2', 'a-copy'],
    });
  });

  it('returns undefined for an unknown layer', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    expect(duplicateLayer(style, 'zzz')).toBeUndefined();
  });
});

describe('deleteLayer', () => {
  it('removes the layer', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    deleteLayer(style, 'a');
    expect(style.layers).toEqual([{ id: 'b', type: 'background' }]);
  });
});

describe('renameLayer', () => {
  it('renames a layer', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    const ok = renameLayer(style, 'a', 'z');
    expect({ ids: style.layers.map((l) => l.id), ok }).toEqual({ ids: ['z'], ok: true });
  });

  it('refuses an id that is taken', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', type: 'background' },
      ],
      sources: {},
      version: 8,
    };
    const ok = renameLayer(style, 'a', 'b');
    expect({ ids: style.layers.map((l) => l.id), ok }).toEqual({ ids: ['a', 'b'], ok: false });
  });

  it('refuses an empty id', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    const ok = renameLayer(style, 'a', '');
    expect({ ids: style.layers.map((l) => l.id), ok }).toEqual({ ids: ['a'], ok: false });
  });

  it('refuses an unknown layer', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'a', type: 'background' }],
      sources: {},
      version: 8,
    };
    expect(renameLayer(style, 'zzz', 'y')).toBe(false);
  });
});
