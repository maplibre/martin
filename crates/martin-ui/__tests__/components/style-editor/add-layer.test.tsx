import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AddLayerDialog } from '@/components/style-editor/add-layer/dialog';
import { StyleResourcesProvider } from '@/components/style-editor/style-resources';

vi.mock('lucide-react', async (importOriginal) => importOriginal());

const style: StyleSpecification = {
  layers: [
    {
      id: 'roads-line',
      paint: { 'line-color': '#3366cc' },
      source: 'osm',
      'source-layer': 'roads',
      type: 'line',
    },
  ],
  sources: { osm: { tiles: ['https://tiles.example.com/{z}/{x}/{y}.pbf'], type: 'vector' } },
  version: 8,
};

describe('AddLayerDialog', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        Response.json({
          vector_layers: [{ fields: { name: 'String', pop: 'Number' }, id: 'cities' }],
        }),
      ),
    );
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('creates a line layer from the picked source layer', () => {
    const onAdd = vi.fn();
    render(
      <StyleResourcesProvider value={{ sourceLayers: () => ['roads', 'water'] }}>
        <AddLayerDialog onAdd={onAdd} onClose={vi.fn()} style={style} />
      </StyleResourcesProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: /osm/ }));
    fireEvent.click(screen.getByRole('button', { name: 'roads' }));
    fireEvent.click(screen.getByRole('button', { name: /^Line/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(onAdd.mock.calls).toMatchInlineSnapshot(`
      [
        [
          {
            "layer": {
              "id": "roads-line-2",
              "paint": {
                "line-color": "#3366cc",
                "line-width": 2,
              },
              "source": "osm",
              "source-layer": "roads",
              "type": "line",
            },
          },
        ],
      ]
    `);
  });

  it('adds the Martin source along with the layer when it is not in the style yet', async () => {
    const onAdd = vi.fn();
    render(
      <StyleResourcesProvider
        value={{
          martinSources: [
            {
              id: 'world_cities',
              name: 'world_cities',
              spec: { type: 'vector', url: 'http://martin.test/world_cities' },
            },
          ],
        }}
      >
        <AddLayerDialog onAdd={onAdd} onClose={vi.fn()} style={style} />
      </StyleResourcesProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: /world_cities/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'cities' }));
    fireEvent.click(screen.getByRole('button', { name: /^Circle/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(onAdd.mock.calls).toMatchInlineSnapshot(`
      [
        [
          {
            "layer": {
              "id": "cities-circle",
              "paint": {
                "circle-color": "#3366cc",
                "circle-radius": 5,
                "circle-stroke-color": "#ffffff",
                "circle-stroke-width": 1,
              },
              "source": "world_cities",
              "source-layer": "cities",
              "type": "circle",
            },
            "source": {
              "id": "world_cities",
              "spec": {
                "type": "vector",
                "url": "http://martin.test/world_cities",
              },
            },
          },
        ],
      ]
    `);
  });

  it('refuses an id that another layer already uses', () => {
    const onAdd = vi.fn();
    render(
      <StyleResourcesProvider value={{ sourceLayers: () => ['roads'] }}>
        <AddLayerDialog onAdd={onAdd} onClose={vi.fn()} style={style} />
      </StyleResourcesProvider>,
    );

    fireEvent.change(screen.getByRole('textbox', { name: 'Layer id' }), {
      target: { value: 'roads-line' },
    });

    expect(screen.getByText('A layer "roads-line" already exists')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Create' }).hasAttribute('disabled')).toBe(true);
  });

  it('closes on Escape', async () => {
    const onClose = vi.fn();
    render(
      <StyleResourcesProvider value={{}}>
        <AddLayerDialog onAdd={vi.fn()} onClose={onClose} style={style} />
      </StyleResourcesProvider>,
    );

    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
