import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import {
  countXrayFeatures,
  entryKeyOfXrayLayer,
  xrayEntries,
  xrayStyle,
} from '@/components/style-editor/map-modes/xray';

describe('xrayEntries', () => {
  it('lists every source layer of vector sources plus whole geojson and raster sources', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'water', source: 'omt', 'source-layer': 'water', type: 'fill' },
        { id: 'water-outline', source: 'omt', 'source-layer': 'water', type: 'line' },
        { id: 'pins', source: 'pins', type: 'circle' },
      ],
      sources: {
        hillshade: { tiles: ['https://example.com/{z}/{x}/{y}.png'], type: 'raster' },
        omt: { type: 'vector', url: 'https://example.com/omt' },
        pins: { data: { features: [], type: 'FeatureCollection' }, type: 'geojson' },
      },
      version: 8,
    };

    const entries = xrayEntries(style, (source) => (source === 'omt' ? ['poi', 'water'] : []));

    expect(entries.map(({ color: _, ...rest }) => rest)).toEqual([
      { key: 'hillshade', kind: 'raster', source: 'hillshade', usedBy: [] },
      { key: 'omt/poi', kind: 'vector', source: 'omt', sourceLayer: 'poi', usedBy: [] },
      {
        key: 'omt/water',
        kind: 'vector',
        source: 'omt',
        sourceLayer: 'water',
        usedBy: ['water', 'water-outline'],
      },
      { key: 'pins', kind: 'vector', source: 'pins', usedBy: ['pins'] },
    ]);
  });

  it('steps entry hues so neighbours in the list look different', () => {
    const style: StyleSpecification = {
      layers: [],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };

    const colors = xrayEntries(style, () => ['a', 'b', 'c', 'd']).map((e) => e.color);

    expect(colors).toEqual([
      'hsl(20 80% 62%)',
      'hsl(181 80% 62%)',
      'hsl(81 80% 62%)',
      'hsl(242 80% 62%)',
    ]);
  });
});

describe('xrayStyle', () => {
  it('replaces the layers with unstyled ones and keeps sources, sprite and glyphs', () => {
    const style: StyleSpecification = {
      glyphs: 'https://example.com/{fontstack}/{range}.pbf',
      layers: [
        {
          id: 'water',
          paint: { 'fill-color': 'blue' },
          source: 'omt',
          'source-layer': 'water',
          type: 'fill',
        },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      sprite: 'https://example.com/sprite',
      terrain: { source: 'omt' },
      version: 8,
    };
    const entries = xrayEntries(style, () => ['water']);

    const xray = xrayStyle(style, entries, { accent: '#e6005c', hidden: new Set() });

    expect(xray).toMatchInlineSnapshot(`
      {
        "glyphs": "https://example.com/{fontstack}/{range}.pbf",
        "layers": [
          {
            "id": "xray:background",
            "paint": {
              "background-color": "#0d0f12",
            },
            "type": "background",
          },
          {
            "filter": [
              "match",
              [
                "geometry-type",
              ],
              [
                "Polygon",
                "MultiPolygon",
              ],
              true,
              false,
            ],
            "id": "xray:omt/water:fill",
            "layout": {
              "visibility": "visible",
            },
            "paint": {
              "fill-color": "hsl(20 80% 62%)",
              "fill-opacity": 0.06,
            },
            "source": "omt",
            "source-layer": "water",
            "type": "fill",
          },
          {
            "filter": [
              "match",
              [
                "geometry-type",
              ],
              [
                "LineString",
                "MultiLineString",
                "Polygon",
                "MultiPolygon",
              ],
              true,
              false,
            ],
            "id": "xray:omt/water:line",
            "layout": {
              "visibility": "visible",
            },
            "paint": {
              "line-color": "hsl(20 80% 62%)",
              "line-opacity": 0.85,
              "line-width": 1,
            },
            "source": "omt",
            "source-layer": "water",
            "type": "line",
          },
          {
            "filter": [
              "match",
              [
                "geometry-type",
              ],
              [
                "Point",
                "MultiPoint",
              ],
              true,
              false,
            ],
            "id": "xray:omt/water:point",
            "layout": {
              "visibility": "visible",
            },
            "paint": {
              "circle-color": "hsl(20 80% 62%)",
              "circle-opacity": 1,
              "circle-radius": 2.5,
            },
            "source": "omt",
            "source-layer": "water",
            "type": "circle",
          },
        ],
        "sources": {
          "omt": {
            "type": "vector",
            "url": "https://example.com/omt",
          },
        },
        "sprite": "https://example.com/sprite",
        "version": 8,
      }
    `);
  });

  it('keeps hidden entries as invisible layers and draws areas under lines under points', () => {
    const style: StyleSpecification = {
      layers: [],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };
    const entries = xrayEntries(style, () => ['poi', 'water']);

    const xray = xrayStyle(style, entries, { accent: '#e6005c', hidden: new Set(['omt/poi']) });

    expect(xray.layers.map((l) => [l.id, l.layout?.visibility])).toEqual([
      ['xray:background', undefined],
      ['xray:omt/poi:fill', 'none'],
      ['xray:omt/water:fill', 'visible'],
      ['xray:omt/poi:line', 'none'],
      ['xray:omt/water:line', 'visible'],
      ['xray:omt/poi:point', 'none'],
      ['xray:omt/water:point', 'visible'],
    ]);
  });

  it('draws the features a focused line layer matches on top, with its filter and zoom range', () => {
    const style: StyleSpecification = {
      layers: [
        {
          filter: ['==', ['get', 'class'], 'minor'],
          id: 'road_minor',
          maxzoom: 20,
          minzoom: 13,
          source: 'omt',
          'source-layer': 'transportation',
          type: 'line',
        },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };
    const entries = xrayEntries(style, () => ['transportation']);

    const xray = xrayStyle(style, entries, {
      accent: '#e6005c',
      focus: style.layers[0],
      hidden: new Set(),
    });

    expect(xray.layers.at(-1)).toEqual({
      filter: ['==', ['get', 'class'], 'minor'],
      id: 'xray:focus:line',
      maxzoom: 20,
      minzoom: 13,
      paint: { 'line-color': '#e6005c', 'line-width': 2.5 },
      source: 'omt',
      'source-layer': 'transportation',
      type: 'line',
    });
  });

  it('dims every entry except the emphasised one', () => {
    const style: StyleSpecification = {
      layers: [],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };
    const entries = xrayEntries(style, () => ['poi', 'water']);

    const xray = xrayStyle(style, entries, {
      accent: '#e6005c',
      emphasis: 'omt/water',
      hidden: new Set(),
    });

    expect(
      xray.layers
        .filter((l) => l.type === 'line')
        .map((l) => [l.id, l.type === 'line' ? l.paint?.['line-opacity'] : undefined]),
    ).toEqual([
      ['xray:omt/poi:line', 0.15],
      ['xray:omt/water:line', 0.85],
    ]);
  });
});

describe('entryKeyOfXrayLayer', () => {
  it('maps the layers of an entry back to its key and ignores background and focus layers', () => {
    expect(
      [
        'xray:omt/water:fill',
        'xray:omt/water:point',
        'xray:hillshade',
        'xray:background',
        'xray:focus:line',
        'water',
      ].map(entryKeyOfXrayLayer),
    ).toEqual(['omt/water', 'omt/water', 'hillshade', undefined, undefined, undefined]);
  });
});

describe('countXrayFeatures', () => {
  it('counts a feature once even when tiles and layers return it several times', () => {
    const counts = countXrayFeatures([
      { id: 1, layerId: 'xray:omt/water:fill', properties: {} },
      { id: 1, layerId: 'xray:omt/water:line', properties: {} },
      { id: 2, layerId: 'xray:omt/water:fill', properties: {} },
      { layerId: 'xray:omt/poi:point', properties: { name: 'Dam' } },
      { layerId: 'xray:omt/poi:point', properties: { name: 'Dam' } },
      { layerId: 'xray:omt/poi:point', properties: { name: 'Rokin' } },
      { id: 9, layerId: 'xray:background', properties: {} },
    ]);

    expect(Object.fromEntries(counts)).toEqual({ 'omt/poi': 2, 'omt/water': 2 });
  });
});
