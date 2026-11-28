import type { LayerSpecification } from '@maplibre/maplibre-gl-style-spec';
import { TooltipProvider } from '@radix-ui/react-tooltip';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  LayerInspector,
  type LayerInspectorProps,
} from '@/components/style-editor/inspector/layer-inspector';
import { StyleResourcesProvider } from '@/components/style-editor/style-resources';

const SOURCE_LAYERS: Record<string, string[]> = {
  extra: ['water', 'places'],
  openmaptiles: ['water', 'transportation'],
  other: ['roads', 'places'],
};

function renderInspector(layer: LayerSpecification, overrides: Partial<LayerInspectorProps> = {}) {
  const props: LayerInspectorProps = {
    attributes: [],
    layer,
    onFilter: vi.fn(),
    onRename: vi.fn(() => true),
    onSelectColor: vi.fn(),
    onSet: vi.fn(),
    onSource: vi.fn(),
    onZoomRange: vi.fn(),
    zoom: 14,
    ...overrides,
  };
  render(
    <TooltipProvider>
      <StyleResourcesProvider
        value={{
          sourceLayers: (id) => SOURCE_LAYERS[id] ?? [],
          sources: {
            extra: { type: 'vector', url: 'http://localhost/extra' },
            openmaptiles: { type: 'vector', url: 'http://localhost/openmaptiles' },
            other: { type: 'vector', url: 'http://localhost/other' },
            photos: { type: 'raster', url: 'http://localhost/photos' },
          },
        }}
      >
        <LayerInspector {...props} />
      </StyleResourcesProvider>
    </TooltipProvider>,
  );
  return props;
}

const road: LayerSpecification = {
  id: 'road',
  source: 'openmaptiles',
  'source-layer': 'transportation',
  type: 'line',
};

describe('layer header source fields', () => {
  afterEach(cleanup);

  it('offers only the sources a line layer can draw', () => {
    renderInspector(road);

    fireEvent.click(screen.getByRole('button', { name: 'Source' }));

    expect(screen.getAllByRole('option').map((o) => o.textContent)).toMatchInlineSnapshot(`
      [
        "extravector",
        "openmaptilesvector",
        "othervector",
      ]
    `);
  });

  it('keeps the source layer when the new source has it', () => {
    const props = renderInspector({ ...road, source: 'other', 'source-layer': 'places' });

    fireEvent.click(screen.getByRole('button', { name: 'Source' }));
    fireEvent.click(screen.getByRole('option', { name: /^extra/ }));

    expect(props.onSource).toHaveBeenCalledWith('extra', 'places');
  });

  it('picks the first listed source layer when the new source lacks the current one', () => {
    const props = renderInspector(road);

    fireEvent.click(screen.getByRole('button', { name: 'Source' }));
    fireEvent.click(screen.getByRole('option', { name: /^other/ }));

    expect(props.onSource).toHaveBeenCalledWith('other', 'roads');
  });

  it('changes only the source layer from the source layer dropdown', () => {
    const props = renderInspector(road);

    fireEvent.click(screen.getByRole('button', { name: 'Source layer' }));
    fireEvent.click(screen.getByRole('option', { name: 'water' }));

    expect(props.onSource).toHaveBeenCalledWith('openmaptiles', 'water');
  });

  it('accepts a typed source layer name', () => {
    const props = renderInspector(road);

    fireEvent.click(screen.getByRole('button', { name: 'Source layer' }));
    fireEvent.change(screen.getByPlaceholderText('Search or type a name'), {
      target: { value: 'poi' },
    });
    fireEvent.keyDown(screen.getByPlaceholderText('Search or type a name'), { key: 'Enter' });

    expect(props.onSource).toHaveBeenCalledWith('openmaptiles', 'poi');
  });

  it('shows no source fields for a background layer', () => {
    renderInspector({ id: 'bg', type: 'background' });

    expect(screen.getByText('No source')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Source' })).toBeNull();
  });

  it('hints when the source layer has no features loaded yet', () => {
    renderInspector({ ...road, 'source-layer': 'poi' });

    expect(screen.getByText(/has no features loaded yet/).textContent).toBe(
      'poi has no features loaded yet',
    );
  });
});

describe('layer header zoom range', () => {
  afterEach(cleanup);

  it('says all zooms when neither end is set', () => {
    renderInspector(road);

    expect(screen.getByText('All zooms')).toBeTruthy();
  });

  it('warns when the current zoom is outside the range', () => {
    renderInspector({ ...road, maxzoom: 9, minzoom: 0 });

    expect(screen.getByText('Hidden at z14 (outside range)')).toBeTruthy();
  });

  it('says nothing when the current zoom is inside the range', () => {
    renderInspector({ ...road, maxzoom: 20, minzoom: 5 });

    expect(screen.queryByText(/All zooms|Hidden/)).toBeNull();
  });

  it('moves the minimum by half a zoom per arrow key', () => {
    const props = renderInspector({ ...road, maxzoom: 20, minzoom: 5 });

    fireEvent.keyDown(screen.getByRole('slider', { name: 'Minimum zoom' }), { key: 'ArrowRight' });

    expect(props.onZoomRange).toHaveBeenCalledWith(5.5, 20);
  });

  it('moves the maximum by two zooms per PageDown', () => {
    const props = renderInspector({ ...road, maxzoom: 20, minzoom: 5 });

    fireEvent.keyDown(screen.getByRole('slider', { name: 'Maximum zoom' }), { key: 'PageDown' });

    expect(props.onZoomRange).toHaveBeenCalledWith(5, 18);
  });

  it('unsets the maximum when End moves it to the top of the track', () => {
    const props = renderInspector({ ...road, maxzoom: 20, minzoom: 5 });

    fireEvent.keyDown(screen.getByRole('slider', { name: 'Maximum zoom' }), { key: 'End' });

    expect(props.onZoomRange).toHaveBeenCalledWith(5, undefined);
  });

  it('commits a typed minimum and clears it when emptied', () => {
    const props = renderInspector({ ...road, maxzoom: 20, minzoom: 5 });
    const field = screen.getByRole('textbox', { name: 'Minimum zoom' });

    fireEvent.change(field, { target: { value: '' } });
    fireEvent.blur(field);

    expect(props.onZoomRange).toHaveBeenCalledWith(undefined, 20);
  });
});
