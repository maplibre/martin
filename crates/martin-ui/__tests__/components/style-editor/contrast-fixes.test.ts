import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import { contrastFixes, labelIssues } from '@/components/style-editor/palette/model';

describe('contrastFixes', () => {
  it('offers darker text that keeps the halo, and a black halo, each above 4.5:1', () => {
    const style: StyleSpecification = {
      layers: [
        {
          id: 'water-label',
          layout: { 'text-field': ['get', 'name'] },
          paint: {
            'text-color': '#74aee9',
            'text-halo-color': 'rgba(255,255,255,0.7)',
            'text-halo-width': 1.5,
          },
          source: 's',
          'source-layer': 'water_name',
          type: 'symbol',
        },
      ],
      sources: { s: { type: 'vector', url: 'https://example.com/tiles.json' } },
      version: 8,
    };
    const [issue] = labelIssues(style);

    expect(
      contrastFixes(style, issue).map((f) => ({
        changes: f.changes,
        label: f.label,
        meetsTarget: f.ratio >= 4.5,
      })),
    ).toMatchInlineSnapshot(`
      [
        {
          "changes": [
            {
              "name": "text-color",
              "value": "#3f78af",
            },
          ],
          "label": "Darker text",
          "meetsTarget": true,
        },
        {
          "changes": [
            {
              "name": "text-halo-color",
              "value": "#000000",
            },
          ],
          "label": "Black halo",
          "meetsTarget": true,
        },
      ]
    `);
  });

  it('adds a halo width when the label has no halo yet', () => {
    const style: StyleSpecification = {
      layers: [
        { id: 'bg', paint: { 'background-color': '#ffffff' }, type: 'background' },
        {
          id: 'pale-label',
          layout: { 'text-field': 'Hello' },
          paint: { 'text-color': '#dddddd' },
          source: 's',
          'source-layer': 'place',
          type: 'symbol',
        },
      ],
      sources: { s: { type: 'vector', url: 'https://example.com/tiles.json' } },
      version: 8,
    };
    const [issue] = labelIssues(style);

    expect(contrastFixes(style, issue).find((f) => f.label.endsWith('halo'))?.changes).toEqual([
      { name: 'text-halo-color', value: '#000000' },
      { name: 'text-halo-width', value: 1.5 },
    ]);
  });
});
