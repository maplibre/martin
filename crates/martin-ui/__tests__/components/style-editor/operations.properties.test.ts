import { type StyleSpecification, validateStyleMin } from '@maplibre/maplibre-gl-style-spec';
import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import {
  type FilterCondition,
  serializeFilter,
} from '@/components/style-editor/inspector/filter-model';
import {
  deleteLayer,
  duplicateLayer,
  moveLayer,
  renameLayer,
  setFilter,
  setVisibility,
  setZoomRange,
} from '@/components/style-editor/operations';

const STYLE: StyleSpecification = {
  layers: [
    { id: 'background', paint: { 'background-color': '#f2efe9' }, type: 'background' },
    { id: 'relief', source: 'relief', type: 'raster' },
    {
      id: 'areas',
      paint: { 'fill-color': '#ffb800' },
      source: 'features',
      'source-layer': 'places',
      type: 'fill',
    },
    { id: 'lines', source: 'features', 'source-layer': 'roads', type: 'line' },
    {
      id: 'labels',
      layout: { 'text-field': ['get', 'name'] },
      source: 'features',
      'source-layer': 'places',
      type: 'symbol',
    },
  ],
  sources: {
    features: { tiles: ['http://localhost/features/{z}/{x}/{y}'], type: 'vector' },
    relief: { tiles: ['http://localhost/relief/{z}/{x}/{y}'], type: 'raster' },
  },
  version: 8,
};

const zoom = fc.option(fc.integer({ max: 24, min: 0 }), { nil: undefined });
const condition: fc.Arbitrary<FilterCondition> = fc.oneof(
  fc.record({
    attribute: fc.constantFrom('name', 'rank'),
    operator: fc.constantFrom('is' as const, 'isNot' as const),
    value: fc.oneof(fc.string(), fc.integer()),
  }),
  fc.record({
    attribute: fc.constant('rank'),
    operator: fc.constantFrom('lt' as const, 'gte' as const),
    value: fc.integer(),
  }),
  fc.record({
    attribute: fc.constantFrom('name', 'class'),
    operator: fc.constantFrom('exists' as const, 'notExists' as const),
  }),
);

const edit = fc.oneof(
  fc.record({ kind: fc.constant('visibility' as const), pick: fc.nat(), visible: fc.boolean() }),
  fc.record({ kind: fc.constant('zoom' as const), pick: fc.nat(), range: fc.tuple(zoom, zoom) }),
  fc.record({
    kind: fc.constant('move' as const),
    pick: fc.nat(),
    to: fc.integer({ max: 8, min: -2 }),
  }),
  fc.record({ kind: fc.constant('duplicate' as const), pick: fc.nat() }),
  fc.record({ kind: fc.constant('delete' as const), pick: fc.nat() }),
  fc.record({
    kind: fc.constant('rename' as const),
    name: fc.stringMatching(/^[a-z][a-z0-9_-]{0,8}$/),
    pick: fc.nat(),
  }),
  fc.record({
    conditions: fc.array(condition, { maxLength: 3 }),
    kind: fc.constant('filter' as const),
    pick: fc.nat(),
  }),
);

type Edit = typeof edit extends fc.Arbitrary<infer T> ? T : never;

function apply(style: StyleSpecification, e: Edit) {
  if (style.layers.length === 0) return;
  const layer = style.layers[e.pick % style.layers.length];
  switch (e.kind) {
    case 'visibility':
      return setVisibility(style, [layer.id], e.visible);
    case 'zoom': {
      const [a, b] = e.range;
      const ordered = a !== undefined && b !== undefined && a > b ? [b, a] : [a, b];
      return setZoomRange(style, layer.id, ordered[0], ordered[1]);
    }
    case 'move':
      return moveLayer(style, layer.id, e.to);
    case 'duplicate':
      return duplicateLayer(style, layer.id);
    case 'delete':
      return deleteLayer(style, layer.id);
    case 'rename':
      return renameLayer(style, layer.id, e.name);
    case 'filter':
      if (!('source-layer' in layer)) return;
      return setFilter(
        style,
        layer.id,
        serializeFilter({ conditions: e.conditions, mode: 'structured' }),
      );
  }
}

describe('style operation properties', () => {
  it('keeps the style valid after any sequence of edits', () => {
    fc.assert(
      fc.property(fc.array(edit, { maxLength: 12 }), (edits) => {
        const style = structuredClone(STYLE);
        for (const e of edits) apply(style, e);
        expect(validateStyleMin(style)).toEqual([]);
      }),
    );
  });

  it('keeps layer ids unique after any sequence of edits', () => {
    fc.assert(
      fc.property(fc.array(edit, { maxLength: 12 }), (edits) => {
        const style = structuredClone(STYLE);
        for (const e of edits) apply(style, e);
        const ids = style.layers.map((l) => l.id);
        expect(new Set(ids).size).toBe(ids.length);
      }),
    );
  });

  it('leaves every other layer untouched by a single edit', () => {
    fc.assert(
      fc.property(
        edit.filter((e) => e.kind !== 'move' && e.kind !== 'duplicate' && e.kind !== 'delete'),
        (e) => {
          const style = structuredClone(STYLE);
          const index = e.pick % style.layers.length;
          apply(style, e);
          expect(style.layers.filter((_, i) => i !== index)).toEqual(
            STYLE.layers.filter((_, i) => i !== index),
          );
        },
      ),
    );
  });

  it('only reorders layers when moving one', () => {
    fc.assert(
      fc.property(fc.nat(), fc.integer({ max: 8, min: -2 }), (pick, to) => {
        const style = structuredClone(STYLE);
        moveLayer(style, style.layers[pick % style.layers.length].id, to);
        const byId = (a: { id: string }, b: { id: string }) => a.id.localeCompare(b.id);
        expect([...style.layers].sort(byId)).toEqual([...STYLE.layers].sort(byId));
      }),
    );
  });
});
