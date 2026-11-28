import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { TooltipProvider } from '@radix-ui/react-tooltip';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import type { Map as MapLibre } from 'maplibre-gl';
import { afterEach, describe, expect, it } from 'vitest';
import { StyleSettings } from '@/components/style-editor/style-settings/style-settings';

function setup(style: StyleSpecification, map: MapLibre | null = null) {
  const edits: StyleSpecification[] = [];
  const apply = (_label: string, recipe: (draft: StyleSpecification) => void) => {
    const draft = structuredClone(style);
    recipe(draft);
    edits.push(draft);
  };
  render(<StyleSettings apply={apply} map={map} style={style} />, { wrapper: TooltipProvider });
  return edits;
}

const empty: StyleSpecification = { layers: [], sources: {}, version: 8 };

describe('Style settings general, rendering and metadata', () => {
  afterEach(cleanup);

  it('says the map opens at 0,0 z0 when no camera is set', () => {
    setup(empty);

    expect(screen.getByText('Not set: the map opens at 0,0 z0')).toBeTruthy();
  });

  it('copies center, zoom, bearing and pitch of the live map in one edit', () => {
    const map = {
      getBearing: () => 12.345,
      getCenter: () => ({ lat: 52.3702157, lng: 4.8951679 }),
      getPitch: () => 40.04,
      getZoom: () => 14.2345,
    } as unknown as MapLibre;
    const edits = setup(empty, map);

    fireEvent.click(screen.getByRole('button', { name: /use current map view/i }));

    expect(edits).toEqual([
      {
        ...empty,
        bearing: 12.3,
        center: [4.89517, 52.37022],
        pitch: 40,
        zoom: 14.23,
      },
    ]);
  });

  it('writes the globe projection when Globe is chosen', () => {
    const edits = setup(empty);

    fireEvent.click(screen.getByRole('button', { name: 'Projection group' }));
    fireEvent.click(screen.getByRole('button', { name: 'globe' }));

    expect(edits).toEqual([{ ...empty, projection: { type: 'globe' } }]);
  });

  it('removes the projection group', () => {
    const edits = setup({ ...empty, projection: { type: 'globe' } });

    fireEvent.click(within(screen.getByRole('group', { name: 'Projection' })).getByText('Remove'));

    expect(edits).toEqual([empty]);
  });

  it('offers only raster-dem sources for terrain', () => {
    const style: StyleSpecification = {
      ...empty,
      sources: {
        dem: { tiles: ['https://x/{z}/{x}/{y}.png'], type: 'raster-dem' },
        osm: { tiles: ['https://x/{z}/{x}/{y}.png'], type: 'raster' },
      },
      terrain: { source: 'dem' },
    };
    setup(style);

    expect(
      within(screen.getByRole('combobox', { name: 'Terrain source' }))
        .getAllByRole('option')
        .map((o) => o.textContent),
    ).toEqual(['dem']);
  });

  it('explains how to enable terrain when there is no raster-dem source', () => {
    setup(empty);

    fireEvent.click(screen.getByRole('button', { name: 'Terrain group' }));

    expect(screen.getByText('Add a raster-dem source in Sources first')).toBeTruthy();
  });

  it('commits metadata only when the JSON is valid', () => {
    const edits = setup(empty);
    const box = screen.getByRole('textbox', { name: 'metadata' });

    fireEvent.change(box, { target: { value: '{"a":' } });
    fireEvent.blur(box);
    expect(edits).toEqual([]);
    expect(box.getAttribute('aria-invalid')).toBe('true');

    fireEvent.change(box, { target: { value: '{"a": 1}' } });
    fireEvent.blur(box);
    expect(edits).toEqual([{ ...empty, metadata: { a: 1 } }]);
  });

  it('commits state on Ctrl+Enter', () => {
    const edits = setup(empty);
    const box = screen.getByRole('textbox', { name: 'state' });

    fireEvent.change(box, { target: { value: '{"lang":{"default":"en"}}' } });
    fireEvent.keyDown(box, { ctrlKey: true, key: 'Enter' });

    expect(edits).toEqual([{ ...empty, state: { lang: { default: 'en' } } }]);
  });
});
