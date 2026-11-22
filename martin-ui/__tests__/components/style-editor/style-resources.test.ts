import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import { collectFonts } from '@/components/style-editor/style-resources';

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
