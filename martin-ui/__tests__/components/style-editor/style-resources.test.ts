import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { collectFonts, useMartinCatalog } from '@/components/style-editor/style-resources';

vi.mock('@/lib/api', () => ({
  buildMartinUrl: vi.fn((path: string) => `http://localhost:3000${path}`),
}));

describe('collectFonts', () => {
  it('collects the font stacks text-font can produce, not the inputs that pick them', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'plain',
          layout: { 'text-font': ['Noto Sans Regular'] },
          source: 'omt',
          'source-layer': 'place',
          type: 'symbol',
        },
        {
          id: 'by-class',
          layout: {
            'text-font': [
              'match',
              ['get', 'class'],
              ['city', 'town'],
              ['literal', ['Noto Sans Bold']],
              'village',
              ['literal', ['Noto Sans Italic']],
              ['literal', ['Noto Sans Regular']],
            ],
          },
          source: 'omt',
          'source-layer': 'place',
          type: 'symbol',
        },
        {
          id: 'by-zoom',
          layout: {
            'text-font': [
              'step',
              ['zoom'],
              ['literal', ['Noto Sans Regular']],
              12,
              [
                'case',
                ['has', 'name'],
                ['literal', ['Noto Sans Bold']],
                ['literal', ['Open Sans Regular']],
              ],
            ],
          },
          source: 'omt',
          'source-layer': 'place',
          type: 'symbol',
        },
        {
          id: 'from-data',
          layout: {
            'text-font': ['coalesce', ['get', 'font'], ['literal', ['Noto Sans Regular']]],
          },
          source: 'omt',
          'source-layer': 'place',
          type: 'symbol',
        },
      ],
      sources: { omt: { type: 'vector', url: 'https://example.com/omt' } },
      version: 8,
    };

    expect(collectFonts(style)).toEqual([
      'Noto Sans Regular',
      'Noto Sans Bold',
      'Noto Sans Italic',
      'Open Sans Regular',
    ]);
  });
});

describe('useMartinCatalog', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('offers MLT, MVT and raster sources and marks MLT ones with the mlt encoding', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() =>
        Promise.resolve(
          new Response(
            JSON.stringify({
              tiles: {
                dem: { content_type: 'application/octet-stream', name: 'Other' },
                mlt: { content_type: 'application/vnd.maplibre-tile', name: 'MLT' },
                mlt_old: { content_type: 'application/vnd.maplibre-vector-tile', name: 'MLT old' },
                mvt: { content_type: 'application/x-protobuf', name: 'MVT' },
                png: { content_type: 'image/png', name: 'PNG' },
              },
            }),
          ),
        ),
      ),
    );

    const { result } = renderHook(() => useMartinCatalog());

    await waitFor(() => expect(result.current.tiles).toHaveLength(4));
    expect(result.current.tiles.map((t) => [t.id, t.spec])).toEqual([
      ['mlt', { encoding: 'mlt', type: 'vector', url: 'http://localhost:3000/mlt' }],
      ['mlt_old', { encoding: 'mlt', type: 'vector', url: 'http://localhost:3000/mlt_old' }],
      ['mvt', { type: 'vector', url: 'http://localhost:3000/mvt' }],
      ['png', { tileSize: 256, type: 'raster', url: 'http://localhost:3000/png' }],
    ]);
  });
});
