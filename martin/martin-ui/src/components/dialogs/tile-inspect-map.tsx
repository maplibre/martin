'use client';

import MaplibreInspect from '@maplibre/maplibre-gl-inspect';
import type { MapRef } from '@vis.gl/react-maplibre';
import { Layer, Map as MapLibreMap, Source } from '@vis.gl/react-maplibre';
import type { Map as MapLibre, VectorSourceSpecification } from 'maplibre-gl';
import { Popup } from 'maplibre-gl';
import { type ErrorInfo, useEffect, useId, useRef } from 'react';
import { useAsyncOperation } from '@/hooks/use-async-operation';
import { useToast } from '@/hooks/use-toast';
import { useUnderlayPreference } from '@/hooks/use-underlay-preference';
import { buildMartinUrl } from '@/lib/api';
import { martinClient } from '@/lib/martin-client';
import type { Catalog } from '@/lib/types.gen';
import { ErrorBoundary } from '../error/error-boundary';
import { UnderlayPicker } from './underlay-picker';
import { findProvider, UNDERLAY_PROVIDER_IDS, type UnderlayProviderId } from './underlay-providers';

interface TileInspectDialogMapProps {
  name: string;
  source: Catalog['tiles'][string];
}

interface TileJson {
  bounds: [number, number, number, number];
  maxzoom?: number;
  minzoom?: number;
  center?: [number, number, number];
}

const UNDERLAY_SOURCE_ID = 'martin-underlay';
const UNDERLAY_LAYER_ID = 'martin-underlay-layer';

const fetchTileJson = async (endpoint: string): Promise<TileJson> => {
  const { data, response } = await martinClient.GET('/{source_ids}', {
    params: { path: { source_ids: endpoint } },
  });
  if (data === undefined) {
    throw new Error(`Failed to fetch tileJson: ${response.statusText}`);
  }
  // The OpenAPI spec types this response as `unknown` (TileJSON 3.0.0); the
  // local `TileJson` interface narrows to the fields this component reads.
  return data as TileJson;
};

function applyUnderlay(map: MapLibre, underlayId: UnderlayProviderId | undefined) {
  const provider = findProvider(underlayId);
  // Short-circuit when the existing source already serves the requested
  // provider: avoids removing & re-adding (and re-fetching the tiles) when
  // the user picks the same option twice.
  const existingSource = map.getSource(UNDERLAY_SOURCE_ID) as
    | { tiles?: readonly string[] }
    | undefined;
  if (
    provider &&
    existingSource?.tiles?.[0] === provider.tiles &&
    map.getLayer(UNDERLAY_LAYER_ID)
  ) {
    return;
  }

  if (map.getLayer(UNDERLAY_LAYER_ID)) {
    map.removeLayer(UNDERLAY_LAYER_ID);
  }
  if (existingSource) {
    map.removeSource(UNDERLAY_SOURCE_ID);
  }

  if (!provider) return;

  map.addSource(UNDERLAY_SOURCE_ID, {
    attribution: provider.attribution,
    tileSize: 256,
    tiles: [provider.tiles],
    type: 'raster',
  });

  const existingLayers = map.getStyle().layers ?? [];
  const beforeId = existingLayers.find(
    (l) => l.id !== UNDERLAY_LAYER_ID && l.type !== 'background',
  )?.id;
  map.addLayer(
    {
      id: UNDERLAY_LAYER_ID,
      source: UNDERLAY_SOURCE_ID,
      type: 'raster',
    },
    beforeId,
  );
}

export function TileInspectDialogMap({ name, source }: TileInspectDialogMapProps) {
  const { toast } = useToast();
  const id = useId();
  const mapRef = useRef<MapRef>(null);
  const inspectControlRef = useRef<MaplibreInspect>(null);
  const [selectedUnderlay, setSelectedUnderlay] = useUnderlayPreference(UNDERLAY_PROVIDER_IDS);
  const selectedUnderlayRef = useRef(selectedUnderlay);
  selectedUnderlayRef.current = selectedUnderlay;

  const tileJsonOperation = useAsyncOperation<TileJson>(() => fetchTileJson(name), {
    onError: (error: Error) => console.error('TileJson Fetch Failed:', error),
    showErrorToast: false,
  });

  const isImageSource = ['image/gif', 'image/jpeg', 'image/png', 'image/webp'].includes(
    source.content_type,
  );

  useEffect(() => {
    tileJsonOperation.execute();
  }, []);

  // Re-apply when the user clicks a different underlay button. The
  // styledata listener wired in onLoad covers the maplibre-gl-inspect
  // setStyle case; this effect covers user-driven changes only.
  useEffect(() => {
    const map = mapRef.current?.getMap();
    if (map?.isStyleLoaded()) {
      applyUnderlay(map, selectedUnderlay);
    }
  }, [selectedUnderlay]);

  const configureMap = () => {
    if (!mapRef.current) {
      console.error('Map not found despite being initialized, this cannot happen');
      return;
    }
    const map = mapRef.current.getMap();
    // Apply the underlay before the TileJSON early-return: on initial map
    // load the TileJSON fetch is still in flight, but the underlay should
    // be visible immediately rather than only after maplibre-gl-inspect's
    // ~1s setStyle re-fires styledata.
    applyUnderlay(map, selectedUnderlay);
    if (!tileJsonOperation.data) {
      return;
    }
    const tileJson: TileJson = tileJsonOperation.data;
    if (tileJson.bounds) {
      const [west, south, east, north] = tileJson.bounds;
      const isWorld = west <= -179 && east >= 179;
      if (!isWorld) {
        map.setMaxBounds([
          [west, south],
          [east, north],
        ]);
      }
    }
    if (tileJson.minzoom) {
      map.setMinZoom(tileJson.minzoom);
      map.setZoom(tileJson.minzoom);
    }
    if (tileJson.maxzoom) {
      map.setMaxZoom(tileJson.maxzoom);
    }
    if (tileJson.center) {
      map.setCenter([tileJson.center[0], tileJson.center[1]]);
    }
  };

  const addInspectorToMap = () => {
    if (!mapRef.current) {
      console.error('Map not found despite being initialized, this cannot happen');
      return;
    }
    const map = mapRef.current.getMap();

    map.addSource(name, {
      type: 'vector',
      url: buildMartinUrl(`/${name}`),
      ...((source.content_type === 'application/vnd.maplibre-tile' ||
        source.content_type === 'application/vnd.maplibre-vector-tile') && { encoding: 'mlt' }),
    } as VectorSourceSpecification);
    // Import and add the inspect control
    if (inspectControlRef.current) {
      map.removeControl(inspectControlRef.current);
    }

    inspectControlRef.current = new MaplibreInspect({
      popup: new Popup({
        closeButton: false,
        closeOnClick: false,
      }),
      showInspectButton: false,
      showInspectMap: true,
      showInspectMapPopup: true,
      showInspectMapPopupOnHover: true,
      showMapPopup: true,
    });

    map.addControl(inspectControlRef.current);

    // maplibre-gl-inspect renders by calling map.setStyle(...) ~1s after the
    // vector source loads, which wipes our raster underlay. Listen for
    // styledata and re-apply only when the underlay has actually been wiped
    // - checking the source's presence avoids re-entering this handler on
    // every styledata fired by our own addSource/addLayer calls.
    map.on('styledata', () => {
      const wanted = selectedUnderlayRef.current;
      if (wanted && !map.getSource(UNDERLAY_SOURCE_ID)) {
        applyUnderlay(map, wanted);
      }
    });

    configureMap();
  };

  return (
    <ErrorBoundary
      onError={(error: Error, errorInfo: ErrorInfo) => {
        console.error('Application error:', error, errorInfo);
        toast({
          description: 'An unexpected error occurred. The page will reload automatically.',
          title: 'Application Error',
          variant: 'destructive',
        });

        // Auto-reload after 3 seconds
        setTimeout(() => {
          window.location.reload();
        }, 3000);
      }}
    >
      <div className="relative">
        <UnderlayPicker onChange={setSelectedUnderlay} value={selectedUnderlay} />
        {isImageSource ? (
          <MapLibreMap
            hash="map"
            onLoad={configureMap}
            ref={mapRef}
            reuseMaps={false}
            style={{
              height: '500px',
              width: '100%',
            }}
          >
            <Source id={`${id}tile-source`} type="raster" url={buildMartinUrl(`/${name}`)} />
            <Layer id={`${id}tile-layer`} source={`${id}tile-source`} type="raster" />
          </MapLibreMap>
        ) : (
          <MapLibreMap
            hash="map"
            onLoad={addInspectorToMap}
            ref={mapRef}
            reuseMaps={false}
            style={{
              height: '500px',
              width: '100%',
            }}
          ></MapLibreMap>
        )}
      </div>
    </ErrorBoundary>
  );
}
