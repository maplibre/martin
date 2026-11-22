import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { act, cleanup, renderHook } from '@testing-library/react';
import type { Map as MapLibre } from 'maplibre-gl';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSourceSchema } from '@/components/style-editor/source-schema';

type Features = Record<string, { properties: Record<string, unknown> }[]>;

function fakeMap(features: Features) {
  const listeners: (() => void)[] = [];
  const queries: unknown[] = [];
  const map = {
    getSource: (id: string) =>
      id in features || Object.keys(features).some((k) => k.startsWith(`${id}|`)) ? {} : undefined,
    off: vi.fn(),
    on: (_: string, listener: () => void) => listeners.push(listener),
    querySourceFeatures: (source: string, options?: { sourceLayer?: string }) => {
      queries.push([source, options]);
      return features[options?.sourceLayer ? `${source}|${options.sourceLayer}` : source] ?? [];
    },
  };
  return {
    idle: () =>
      act(() => {
        for (const listener of listeners) listener();
      }),
    map: map as unknown as MapLibre,
    queries,
  };
}

describe('useSourceSchema', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(new Response('{}', { status: 404 }))),
    );
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('samples attributes of GeoJSON sources, which have no source layer', () => {
    const style: StyleSpecification = {
      layers: [{ id: 'pins', source: 'pins', type: 'circle' }],
      sources: { pins: { data: { features: [], type: 'FeatureCollection' }, type: 'geojson' } },
      version: 8,
    };
    const { map, idle } = fakeMap({
      pins: [{ properties: { kind: 'cafe' } }, { properties: { kind: 'bar' } }],
    });

    const { result } = renderHook(() => useSourceSchema(style, map));
    idle();

    expect(result.current.attributes('pins', undefined)).toEqual([
      {
        description: undefined,
        kind: 'string',
        name: 'kind',
        range: undefined,
        values: ['cafe', 'bar'],
      },
    ]);
  });

  it('keeps source and source layer names that contain slashes apart', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'roads', source: 'vendor/roads', 'source-layer': 'major/minor', type: 'line' },
      ],
      sources: { 'vendor/roads': { type: 'vector', url: 'https://example.com/roads' } },
      version: 8,
    };
    const { map, idle, queries } = fakeMap({
      'vendor/roads|major/minor': [{ properties: { lanes: 2 } }],
    });

    const { result } = renderHook(() => useSourceSchema(style, map));
    idle();

    expect(queries).toEqual([['vendor/roads', { sourceLayer: 'major/minor' }]]);
    expect(result.current.sourceLayers('vendor/roads')).toEqual(['major/minor']);
    expect(result.current.attributes('vendor/roads', 'major/minor').map((a) => a.name)).toEqual([
      'lanes',
    ]);
  });

  it('forgets what it learned about a source once the source definition changes', () => {
    const before: StyleSpecification = {
      layers: [{ id: 'roads', source: 'omt', 'source-layer': 'transportation', type: 'line' }],
      sources: { omt: { type: 'vector', url: 'https://example.com/v1' } },
      version: 8,
    };
    const after: StyleSpecification = {
      ...before,
      sources: { omt: { type: 'vector', url: 'https://example.com/v2' } },
    };
    const { map, idle } = fakeMap({
      'omt|transportation': [{ properties: { class: 'motorway' } }],
    });

    const { result, rerender } = renderHook(({ style }) => useSourceSchema(style, map), {
      initialProps: { style: before },
    });
    idle();
    rerender({ style: after });

    expect(result.current.attributes('omt', 'transportation')).toEqual([]);
    expect(result.current.sourceLayers('omt')).toEqual([]);
  });
});
