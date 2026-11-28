import { type StyleSpecification, validateStyleMin } from '@maplibre/maplibre-gl-style-spec';
import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { serializeFilter } from '@/components/style-editor/inspector/filter-model';
import {
  deleteLayer,
  duplicateLayer,
  moveLayer,
  renameLayer,
  setFilter,
  setVisibility,
  setZoomRange,
} from '@/components/style-editor/operations';

describe('style operation properties', () => {
  it('keeps the style valid after any sequence of edits', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.oneof(
            fc.record({
              kind: fc.constant('show' as const),
              pick: fc.nat(),
              visible: fc.boolean(),
            }),
            fc.record({
              kind: fc.constant('zoom' as const),
              pick: fc.nat(),
              range: fc
                .tuple(fc.integer({ max: 24, min: 0 }), fc.integer({ max: 24, min: 0 }))
                .map(([a, b]) => [Math.min(a, b), Math.max(a, b)]),
            }),
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
              kind: fc.constant('filter' as const),
              pick: fc.nat(),
              rank: fc.integer(),
            }),
          ),
          { maxLength: 12 },
        ),
        (edits) => {
          const style: StyleSpecification = {
            layers: [
              { id: 'background', paint: { 'background-color': '#f2efe9' }, type: 'background' },
              { id: 'relief', source: 'relief', type: 'raster' },
              { id: 'areas', source: 'features', 'source-layer': 'places', type: 'fill' },
              { id: 'lines', source: 'features', 'source-layer': 'roads', type: 'line' },
            ],
            sources: {
              features: { tiles: ['http://localhost/features/{z}/{x}/{y}'], type: 'vector' },
              relief: { tiles: ['http://localhost/relief/{z}/{x}/{y}'], type: 'raster' },
            },
            version: 8,
          };
          for (const edit of edits) {
            const layer = style.layers[edit.pick % style.layers.length];
            if (!layer) break;
            if (edit.kind === 'show') setVisibility(style, [layer.id], edit.visible);
            if (edit.kind === 'zoom') setZoomRange(style, layer.id, edit.range[0], edit.range[1]);
            if (edit.kind === 'move') moveLayer(style, layer.id, edit.to);
            if (edit.kind === 'duplicate') duplicateLayer(style, layer.id);
            if (edit.kind === 'delete') deleteLayer(style, layer.id);
            if (edit.kind === 'rename') renameLayer(style, layer.id, edit.name);
            if (edit.kind === 'filter' && 'source-layer' in layer) {
              setFilter(
                style,
                layer.id,
                serializeFilter({
                  conditions: [{ attribute: 'rank', operator: 'gte', value: edit.rank }],
                  mode: 'structured',
                }),
              );
            }
          }
          expect(validateStyleMin(style)).toEqual([]);
        },
      ),
    );
  });

  it('leaves the other layers untouched when hiding one', () => {
    fc.assert(
      fc.property(fc.integer({ max: 2, min: 0 }), (index) => {
        const style: StyleSpecification = {
          layers: [
            { id: 'background', paint: { 'background-color': '#f2efe9' }, type: 'background' },
            { id: 'areas', source: 'features', 'source-layer': 'places', type: 'fill' },
            { id: 'lines', source: 'features', 'source-layer': 'roads', type: 'line' },
          ],
          sources: { features: { tiles: ['http://localhost/{z}/{x}/{y}'], type: 'vector' } },
          version: 8,
        };
        const before = structuredClone(style.layers);

        setVisibility(style, [before[index].id], false);

        expect(style.layers.filter((_, i) => i !== index)).toEqual(
          before.filter((_, i) => i !== index),
        );
      }),
    );
  });

  it('leaves the other layers untouched when changing the zoom range of one', () => {
    fc.assert(
      fc.property(
        fc.integer({ max: 2, min: 0 }),
        fc.integer({ max: 24, min: 0 }),
        fc.integer({ max: 24, min: 0 }),
        (index, min, max) => {
          const style: StyleSpecification = {
            layers: [
              { id: 'background', paint: { 'background-color': '#f2efe9' }, type: 'background' },
              { id: 'areas', source: 'features', 'source-layer': 'places', type: 'fill' },
              { id: 'lines', source: 'features', 'source-layer': 'roads', type: 'line' },
            ],
            sources: { features: { tiles: ['http://localhost/{z}/{x}/{y}'], type: 'vector' } },
            version: 8,
          };
          const before = structuredClone(style.layers);

          setZoomRange(style, before[index].id, min, max);

          expect(style.layers.filter((_, i) => i !== index)).toEqual(
            before.filter((_, i) => i !== index),
          );
        },
      ),
    );
  });

  it('leaves the other layers untouched when renaming one', () => {
    fc.assert(
      fc.property(
        fc.integer({ max: 2, min: 0 }),
        fc.stringMatching(/^[a-z][a-z0-9_-]{0,8}$/),
        (index, name) => {
          const style: StyleSpecification = {
            layers: [
              { id: 'background', paint: { 'background-color': '#f2efe9' }, type: 'background' },
              { id: 'areas', source: 'features', 'source-layer': 'places', type: 'fill' },
              { id: 'lines', source: 'features', 'source-layer': 'roads', type: 'line' },
            ],
            sources: { features: { tiles: ['http://localhost/{z}/{x}/{y}'], type: 'vector' } },
            version: 8,
          };
          const before = structuredClone(style.layers);

          renameLayer(style, before[index].id, name);

          expect(style.layers.filter((_, i) => i !== index)).toEqual(
            before.filter((_, i) => i !== index),
          );
        },
      ),
    );
  });

  it('only reorders layers when moving one', () => {
    fc.assert(
      fc.property(fc.integer({ max: 2, min: 0 }), fc.integer({ max: 8, min: -2 }), (index, to) => {
        const style: StyleSpecification = {
          layers: [
            { id: 'background', paint: { 'background-color': '#f2efe9' }, type: 'background' },
            { id: 'areas', source: 'features', 'source-layer': 'places', type: 'fill' },
            { id: 'lines', source: 'features', 'source-layer': 'roads', type: 'line' },
          ],
          sources: { features: { tiles: ['http://localhost/{z}/{x}/{y}'], type: 'vector' } },
          version: 8,
        };
        const before = structuredClone(style.layers);

        moveLayer(style, before[index].id, to);

        expect(style.layers.map((l) => l.id).sort()).toEqual(['areas', 'background', 'lines']);
        expect(style.layers.find((l) => l.id === before[index].id)).toEqual(before[index]);
      }),
    );
  });
});
