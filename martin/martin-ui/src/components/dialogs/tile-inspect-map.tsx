'use client';

import MaplibreInspect from '@maplibre/maplibre-gl-inspect';
import type { MapRef } from '@vis.gl/react-maplibre';
import { Layer, Map as MapLibreMap, Source } from '@vis.gl/react-maplibre';
import { Popup } from 'maplibre-gl';
import { type ErrorInfo, useCallback, useEffect, useId, useRef } from 'react';
import { Toaster } from '@/components/ui/toaster';
import { useAsyncOperation } from '@/hooks/use-async-operation';
import { useToast } from '@/hooks/use-toast';
import { buildMartinUrl } from '@/lib/api';
import type { TileSource } from '@/lib/types';
import { ErrorBoundary } from '../error/error-boundary';

interface TileInspectDialogMapProps {
  name: string;
  source: TileSource;
}

interface TileJson {
  bounds: [number, number, number, number];
  maxzoom?: number;
  minzoom?: number;
  center?: [number, number, number];
}

const fetchTileJson = async (endpoint: string): Promise<TileJson> => {
  const response = await fetch(buildMartinUrl(`/${endpoint}`));
  if (!response.ok) {
    throw new Error(`Failed to fetch tileJson: ${response.statusText}`);
  }
  return response.json();
};

export function TileInspectDialogMap({ name, source }: TileInspectDialogMapProps) {
  const { toast } = useToast();
  const id = useId();
  const mapRef = useRef<MapRef>(null);
  const inspectControlRef = useRef<MaplibreInspect>(null);

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

  const configureMap = () => {
    if (!tileJsonOperation.data) {
      return;
    }
    if (!mapRef.current) {
      console.error('Map not found despite being initialized, this cannot happen');
      return;
    }
    const map = mapRef.current.getMap();
    const tileJson: TileJson = tileJsonOperation.data;
    //Error when first value set to -180. it gets resolved on setting to -179 or lower. Other values work fine.
    map.setMaxBounds([
      [tileJson.bounds[0] == -180 ? -179 : tileJson.bounds[0], tileJson.bounds[1]],
      [tileJson.bounds[2], tileJson.bounds[3]],
    ]);
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

  const addInspectorToMap = useCallback(() => {
    if (!mapRef.current) {
      console.error('Map not found despite being initialized, this cannot happen');
      return;
    }
    const map = mapRef.current.getMap();

    map.addSource(name, { type: 'vector', url: buildMartinUrl(`/${name}`) });
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

    configureMap();
  }, [name]);

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
      {isImageSource ? (
        <MapLibreMap
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
          onLoad={addInspectorToMap}
          ref={mapRef}
          reuseMaps={false}
          style={{
            height: '500px',
            width: '100%',
          }}
        ></MapLibreMap>
      )}
      <Toaster />
    </ErrorBoundary>
  );
}
