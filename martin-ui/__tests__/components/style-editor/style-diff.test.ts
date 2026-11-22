import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import { diffStyles } from '@/components/style-editor/map-modes/style-diff';

describe('diffStyles', () => {
  it('names changed paint and layout properties and other changed keys', () => {
    const base: StyleSpecification = {
      layers: [
        {
          filter: ['==', ['get', 'class'], 'minor'],
          id: 'road',
          layout: { 'line-cap': 'round' },
          paint: { 'line-color': '#fff', 'line-width': 2 },
          source: 'omt',
          'source-layer': 'transportation',
          type: 'line',
        },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };
    const next: StyleSpecification = {
      ...base,
      layers: [
        {
          filter: ['==', ['get', 'class'], 'service'],
          id: 'road',
          layout: { 'line-cap': 'round', visibility: 'none' },
          minzoom: 12,
          paint: { 'line-color': '#ffd400', 'line-width': 2 },
          source: 'omt',
          'source-layer': 'transportation',
          type: 'line',
        },
      ],
    };

    expect(diffStyles(base, next).layers).toEqual([
      {
        id: 'road',
        kind: 'changed',
        properties: ['filter', 'visibility', 'line-color', 'minzoom'],
      },
    ]);
  });

  it('reports changed and added layers, removed ids and changes outside layers', () => {
    const base: StyleSpecification = {
      layers: [
        { id: 'background', paint: { 'background-color': '#fff' }, type: 'background' },
        {
          id: 'water',
          paint: { 'fill-color': '#9cf' },
          source: 'omt',
          'source-layer': 'water',
          type: 'fill',
        },
        { id: 'parks', source: 'omt', 'source-layer': 'park', type: 'fill' },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };
    const next: StyleSpecification = {
      glyphs: 'https://example.com/{fontstack}/{range}.pbf',
      layers: [
        { id: 'background', paint: { 'background-color': '#fff' }, type: 'background' },
        {
          id: 'water',
          paint: { 'fill-color': '#26d' },
          source: 'omt',
          'source-layer': 'water',
          type: 'fill',
        },
        { id: 'roads', source: 'omt', 'source-layer': 'transportation', type: 'line' },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };

    expect(diffStyles(base, next)).toEqual({
      changed: new Set(['water', 'roads']),
      layers: [
        { id: 'water', kind: 'changed', properties: ['fill-color'] },
        { id: 'roads', kind: 'added', properties: [] },
      ],
      removed: ['parks'],
      root: true,
    });
  });

  it('does not count reordering as a change', () => {
    const base: StyleSpecification = {
      layers: [
        { id: 'a', type: 'background' },
        { id: 'b', source: 'omt', 'source-layer': 'water', type: 'fill' },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };
    const next: StyleSpecification = { ...base, layers: [...base.layers].reverse() };

    expect(diffStyles(base, next)).toEqual({
      changed: new Set(),
      layers: [],
      removed: [],
      root: false,
    });
  });
});
