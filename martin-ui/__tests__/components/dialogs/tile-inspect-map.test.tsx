import { cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TileInspectDialogMap } from '@/components/dialogs/tile-inspect-map';
import { martinClient } from '@/lib/martin-client';
import type { Catalog } from '@/lib/types.gen';

// A single mock MapLibre instance whose methods we assert against. Hoisted so
// the vi.mock factory below (also hoisted) can reference it.
const { mapSpies } = vi.hoisted(() => ({
  mapSpies: {
    addControl: vi.fn(),
    addLayer: vi.fn(),
    addSource: vi.fn(),
    fitBounds: vi.fn(),
    getLayer: vi.fn(),
    getSource: vi.fn(),
    getStyle: vi.fn(),
    isStyleLoaded: vi.fn(),
    jumpTo: vi.fn(),
    on: vi.fn(),
    removeControl: vi.fn(),
    removeLayer: vi.fn(),
    removeSource: vi.fn(),
    setCenter: vi.fn(),
    setMaxBounds: vi.fn(),
    setMaxZoom: vi.fn(),
    setMinZoom: vi.fn(),
  },
}));

// Override the global @vis.gl/react-maplibre mock with one that exposes a ref
// (getMap) and fires onLoad on mount, so configureMap() actually runs.
vi.mock('@vis.gl/react-maplibre', () => {
  const React = require('react');
  return {
    Layer: ({ children }: { children?: React.ReactNode }) =>
      React.createElement('div', { 'data-testid': 'maplibre-layer' }, children),
    Map: React.forwardRef(
      (
        { children, onLoad }: { children?: React.ReactNode; onLoad?: () => void },
        ref: React.Ref<unknown>,
      ) => {
        React.useImperativeHandle(ref, () => ({ getMap: () => mapSpies }));
        React.useEffect(() => {
          onLoad?.();
        }, []);
        return React.createElement('div', { 'data-testid': 'maplibre-map' }, children);
      },
    ),
    Source: ({ children }: { children?: React.ReactNode }) =>
      React.createElement('div', { 'data-testid': 'maplibre-source' }, children),
  };
});

// The component does `new Popup(...)` / `new MaplibreInspect(...)` inside the
// onLoad path. The global setup mocks these as arrow functions, which are not
// constructable; provide constructable stubs for this file.
vi.mock('maplibre-gl', () => ({
  Popup: class {
    addTo() {
      return this;
    }
    remove() {
      return this;
    }
    setHTML() {
      return this;
    }
    setLngLat() {
      return this;
    }
  },
}));

vi.mock('@maplibre/maplibre-gl-inspect', () => ({
  default: class {
    onAdd() {
      return document.createElement('div');
    }
    onRemove() {}
  },
}));

// Feed a controllable TileJSON through the martin client.
vi.mock('@/lib/martin-client', () => ({
  martinClient: { GET: vi.fn() },
}));

// Keep the underlay out of the way; findProvider(undefined) short-circuits.
vi.mock('@/hooks/use-underlay-preference', () => ({
  useUnderlayPreference: () => [undefined, vi.fn()],
}));

const mockedGet = vi.mocked(martinClient.GET);

const setTileJson = (tileJson: unknown) => {
  mockedGet.mockResolvedValue({
    data: tileJson,
    response: { statusText: 'OK' } as Response,
    // openapi-fetch returns extra fields we don't use in this component.
  } as never);
};

const source: Catalog['tiles'][string] = {
  attribution: '',
  content_encoding: 'gzip',
  content_type: 'application/x-protobuf',
  description: '',
  layer_count: 1,
  name: 'workshop',
} as Catalog['tiles'][string];

describe('TileInspectDialogMap initial view', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mapSpies.getStyle.mockReturnValue({ layers: [] });
    mapSpies.getSource.mockReturnValue(undefined);
    mapSpies.getLayer.mockReturnValue(undefined);
    mapSpies.isStyleLoaded.mockReturnValue(true);
    window.location.hash = '';
  });

  afterEach(() => {
    cleanup();
  });

  it('fits the map to the tileset bounds and honors a zero minzoom', async () => {
    setTileJson({
      bounds: [-89.85918, 42.71594, -88.69675, 43.37933],
      center: [-89.27797, 43.04763, 9],
      maxzoom: 14,
      minzoom: 0,
    });

    render(<TileInspectDialogMap name="workshop" source={source} />);

    await waitFor(() => expect(mapSpies.fitBounds).toHaveBeenCalled());

    // Regression: a falsy-zero minzoom used to be skipped entirely.
    expect(mapSpies.setMinZoom).toHaveBeenCalledWith(0);
    expect(mapSpies.setMaxZoom).toHaveBeenCalledWith(14);
    expect(mapSpies.setMaxBounds).toHaveBeenCalledWith([
      [-89.85918, 42.71594],
      [-88.69675, 43.37933],
    ]);
    expect(mapSpies.fitBounds).toHaveBeenCalledWith(
      [
        [-89.85918, 42.71594],
        [-88.69675, 43.37933],
      ],
      expect.objectContaining({ animate: false }),
    );
  });

  it('jumps to center (with its zoom) for a world-extent tileset', async () => {
    setTileJson({
      bounds: [-180, -85, 180, 85],
      center: [10, 20, 4],
      maxzoom: 6,
      minzoom: 0,
    });

    render(<TileInspectDialogMap name="workshop" source={source} />);

    await waitFor(() => expect(mapSpies.jumpTo).toHaveBeenCalled());

    // World extent: no maxBounds clamp, and center[2] drives the zoom.
    expect(mapSpies.setMaxBounds).not.toHaveBeenCalled();
    expect(mapSpies.fitBounds).not.toHaveBeenCalled();
    expect(mapSpies.jumpTo).toHaveBeenCalledWith({ center: [10, 20], zoom: 4 });
  });

  it('respects an explicit #map deep link and does not reposition', async () => {
    window.location.hash = '#map=12/43.07/-89.4';
    setTileJson({
      bounds: [-89.85918, 42.71594, -88.69675, 43.37933],
      center: [-89.27797, 43.04763, 9],
      maxzoom: 14,
      minzoom: 0,
    });

    render(<TileInspectDialogMap name="workshop" source={source} />);

    // Constraints still apply, but the view is left to the URL hash.
    await waitFor(() => expect(mapSpies.setMinZoom).toHaveBeenCalledWith(0));
    expect(mapSpies.fitBounds).not.toHaveBeenCalled();
    expect(mapSpies.jumpTo).not.toHaveBeenCalled();
  });
});
