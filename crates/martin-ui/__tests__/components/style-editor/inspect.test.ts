import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import {
  dedupeFeatures,
  filterClauses,
  filterKeys,
  type InspectedFeature,
  layerMatches,
} from '@/components/style-editor/map-modes/inspect';

const motorway: InspectedFeature = {
  geometry: {
    coordinates: [
      [4.89, 52.37],
      [4.9, 52.38],
    ],
    type: 'LineString',
  },
  id: 42,
  key: 'omt/transportation/LineString/42',
  kind: 'LineString',
  properties: { brunnel: 'bridge', class: 'motorway', oneway: 1 },
  source: 'omt',
  sourceLayer: 'transportation',
};

describe('layerMatches', () => {
  it('says for every layer on the source layer of the feature whether it draws it, topmost first', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'water', source: 'omt', 'source-layer': 'water', type: 'fill' },
        {
          filter: ['==', ['get', 'class'], 'motorway'],
          id: 'road_motorway',
          source: 'omt',
          'source-layer': 'transportation',
          type: 'line',
        },
        {
          filter: ['==', 'class', 'minor'],
          id: 'road_minor',
          source: 'omt',
          'source-layer': 'transportation',
          type: 'line',
        },
        {
          id: 'road_hidden',
          layout: { visibility: 'none' },
          source: 'omt',
          'source-layer': 'transportation',
          type: 'line',
        },
        {
          id: 'road_label',
          minzoom: 16,
          source: 'omt',
          'source-layer': 'transportation',
          type: 'symbol',
        },
        {
          id: 'road_area',
          source: 'omt',
          'source-layer': 'transportation',
          type: 'fill',
        },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };

    expect(layerMatches(style, motorway, 14.5)).toEqual([
      { id: 'road_area', type: 'fill', verdict: { status: 'geometry' } },
      { id: 'road_label', type: 'symbol', verdict: { maxzoom: 24, minzoom: 16, status: 'zoom' } },
      { id: 'road_hidden', type: 'line', verdict: { status: 'hidden' } },
      {
        id: 'road_minor',
        type: 'line',
        verdict: { failing: 'class is minor', status: 'filter' },
      },
      { id: 'road_motorway', type: 'line', verdict: { status: 'drawn' } },
    ]);
  });

  it('evaluates zoom in filters at the integer zoom tiles are built for', () => {
    const style: StyleSpecification = {
      layers: [
        {
          filter: ['>=', ['zoom'], 15],
          id: 'from_15',
          source: 'omt',
          'source-layer': 'transportation',
          type: 'line',
        },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };

    expect(layerMatches(style, motorway, 14.9)[0].verdict).toEqual({
      failing: '[">=",["zoom"],15]',
      status: 'filter',
    });
    expect(layerMatches(style, motorway, 15)[0].verdict).toEqual({ status: 'drawn' });
  });
});

describe('filterClauses', () => {
  it('splits an all filter and reads each condition as a sentence with the properties it checks', () => {
    const layer: StyleSpecification['layers'][number] = {
      filter: [
        'all',
        ['match', ['geometry-type'], ['LineString', 'MultiLineString'], true, false],
        ['match', ['get', 'brunnel'], ['bridge', 'tunnel'], false, true],
        ['==', ['get', 'class'], 'motorway'],
      ],
      id: 'road_motorway',
      source: 'omt',
      'source-layer': 'transportation',
      type: 'line',
    };

    expect(filterClauses(layer, motorway, 14).map(({ clause: _, ...rest }) => rest)).toEqual([
      { keys: [], label: 'geometry is one of LineString, MultiLineString', pass: true },
      { keys: ['brunnel'], label: 'brunnel is not one of bridge, tunnel', pass: false },
      { keys: ['class'], label: 'class is motorway', pass: true },
    ]);
  });

  it('reads legacy filters the same way', () => {
    const layer: StyleSpecification['layers'][number] = {
      filter: ['all', ['==', '$type', 'LineString'], ['in', 'class', 'minor', 'service']],
      id: 'road_minor',
      source: 'omt',
      'source-layer': 'transportation',
      type: 'line',
    };

    expect(filterClauses(layer, motorway, 14).map(({ clause: _, ...rest }) => rest)).toEqual([
      { keys: [], label: 'geometry is LineString', pass: true },
      { keys: ['class'], label: 'class is one of minor, service', pass: false },
    ]);
  });

  it('falls back to the JSON of a condition with no simple reading', () => {
    const layer: StyleSpecification['layers'][number] = {
      filter: ['>', ['length', ['get', 'class']], 3],
      id: 'long_class',
      source: 'omt',
      'source-layer': 'transportation',
      type: 'line',
    };

    expect(filterClauses(layer, motorway, 14)).toEqual([
      {
        clause: ['>', ['length', ['get', 'class']], 3],
        keys: ['class'],
        label: '[">",["length",["get","class"]],3]',
        pass: true,
      },
    ]);
  });

  it('returns nothing for a layer without a filter', () => {
    expect(
      filterClauses(
        { id: 'all_roads', source: 'omt', 'source-layer': 'transportation', type: 'line' },
        motorway,
        14,
      ),
    ).toEqual([]);
  });
});

describe('filterClauses with booleans', () => {
  it('passes and fails boolean conditions the way MapLibre draws them', () => {
    const footway: InspectedFeature = {
      geometry: {
        coordinates: [
          [4.89, 52.37],
          [4.9, 52.38],
        ],
        type: 'LineString',
      },
      key: 'omt/transportation/LineString/footway',
      kind: 'LineString',
      properties: { class: 'path', lit: true, oneway: 1, ramp: false },
      source: 'omt',
      sourceLayer: 'transportation',
    };
    const layer: StyleSpecification['layers'][number] = {
      filter: [
        'all',
        ['==', ['get', 'lit'], true],
        ['==', 'ramp', false],
        ['==', ['get', 'oneway'], true],
        ['!', ['has', 'brunnel']],
        ['match', ['get', 'brunnel'], ['bridge', 'tunnel'], false, true],
        ['match', ['get', 'class'], ['path'], false, true],
      ],
      id: 'paths',
      source: 'omt',
      'source-layer': 'transportation',
      type: 'line',
    };

    expect(filterClauses(layer, footway, 16).map(({ clause: _, ...rest }) => rest)).toEqual([
      { keys: ['lit'], label: 'lit is true', pass: true },
      { keys: ['ramp'], label: 'ramp is false', pass: true },
      {
        keys: ['oneway'],
        label: 'oneway is true',
        pass: false,
        typeMismatch: { actual: 'number', expected: 'boolean', key: 'oneway' },
      },
      { keys: ['brunnel'], label: 'brunnel does not exist', pass: true },
      { keys: ['brunnel'], label: 'brunnel is not one of bridge, tunnel', pass: true },
      { keys: ['class'], label: 'class is not one of path', pass: false },
    ]);
  });
});

describe('filterKeys', () => {
  it('finds properties read by get, has and legacy comparisons but not geometry or id', () => {
    expect(
      filterKeys([
        'any',
        ['==', ['get', 'class'], 'path'],
        ['has', 'name'],
        ['in', 'subclass', 'footway'],
        ['==', '$type', 'Point'],
        ['==', ['id'], 1],
      ]),
    ).toEqual(['class', 'name', 'subclass']);
  });
});

describe('dedupeFeatures', () => {
  it('merges copies of a feature by id, and by properties and geometry when there is no id', () => {
    const { key: _, ...base } = motorway;
    const elsewhere = {
      coordinates: [
        [4.91, 52.39],
        [4.92, 52.4],
      ],
      type: 'LineString' as const,
    };

    const features = dedupeFeatures([
      base,
      { ...base, properties: { class: 'motorway' } },
      { ...base, id: undefined, properties: { class: 'path' } },
      { ...base, id: undefined, properties: { class: 'path' } },
      { ...base, geometry: elsewhere, id: undefined, properties: { class: 'path' } },
      { ...base, sourceLayer: 'transportation_name' },
    ]);

    expect(features.map((f) => [f.sourceLayer, f.id, f.properties, f.geometry])).toEqual([
      ['transportation', 42, base.properties, base.geometry],
      ['transportation', undefined, { class: 'path' }, base.geometry],
      ['transportation', undefined, { class: 'path' }, elsewhere],
      ['transportation_name', 42, base.properties, base.geometry],
    ]);
  });
});
