'use client';

import { useCallback, useEffect, useId, useRef, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { TileSource } from '@/lib/types';
import '@maplibre/maplibre-gl-inspect/dist/maplibre-gl-inspect.css';
import MaplibreInspect from '@maplibre/maplibre-gl-inspect';
import type { MapRef } from '@vis.gl/react-maplibre';
import { Layer, Map as MapLibreMap, Source } from '@vis.gl/react-maplibre';
import { Database } from 'lucide-react';
import { Popup } from 'maplibre-gl';
import { buildMartinUrl } from '@/lib/api';

interface TileJSON {
  tilejson?: string;
  name?: string;
  description?: string;
  version?: string;
  attribution?: string;
  scheme?: string;
  tiles: string[];
  grids?: string[];
  data?: string[];
  minzoom?: number;
  maxzoom?: number;
  bounds?: [number, number, number, number]; // [west, south, east, north]
  center?: [number, number, number]; // [longitude, latitude, zoom]
  vector_layers?: unknown[];
}

interface TileInspectDialogProps {
  name: string;
  source: TileSource;
  onCloseAction: () => void;
}

export function TileInspectDialog({ name, source, onCloseAction }: TileInspectDialogProps) {
  const id = useId();
  const mapRef = useRef<MapRef>(null);
  const inspectControlRef = useRef<MaplibreInspect>(null);
  const [tileJSON, setTileJSON] = useState<TileJSON | null>(null);

  const configureMapBounds = useCallback(() => {
    if (!mapRef.current || !tileJSON) {
      return;
    }

    const map = mapRef.current.getMap();

    // Set minzoom and maxzoom restrictions
    if (tileJSON.minzoom !== undefined) {
      map.setMinZoom(tileJSON.minzoom);
    }
    if (tileJSON.maxzoom !== undefined) {
      map.setMaxZoom(tileJSON.maxzoom);
    }

    // Set bounds restrictions if available
    if (tileJSON.bounds) {
      const [west, south, east, north] = tileJSON.bounds;
      map.setMaxBounds([
        [west, south],
        [east, north],
      ]);
    }

    // Fit to bounds or center if available
    if (tileJSON.bounds) {
      const [west, south, east, north] = tileJSON.bounds;
      map.fitBounds(
        [
          [west, south],
          [east, north],
        ],
        {
          padding: 50,
          maxZoom: tileJSON.maxzoom,
        },
      );
    } else if (tileJSON.center) {
      const [lng, lat, zoom] = tileJSON.center;
      map.setCenter([lng, lat]);
      if (zoom !== undefined) {
        map.setZoom(zoom);
      }
    }
  }, [tileJSON]);

  // Fetch TileJSON when dialog opens
  useEffect(() => {
    let cancelled = false;

    fetch(buildMartinUrl(`/${name}`))
      .then((response) => {
        if (!response.ok) {
          throw new Error(`Failed to fetch TileJSON: ${response.statusText}`);
        }
        return response.json();
      })
      .then((data: TileJSON) => {
        if (!cancelled) {
          setTileJSON(data);
        }
      })
      .catch((error) => {
        console.error('Error fetching TileJSON:', error);
        // Continue without TileJSON restrictions if fetch fails
      });

    return () => {
      cancelled = true;
    };
  }, [name]);

  // Reconfigure bounds when TileJSON loads after map is already initialized
  useEffect(() => {
    if (tileJSON && mapRef.current) {
      configureMapBounds();
    }
  }, [tileJSON, configureMapBounds]);

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

    // Configure bounds after adding inspector
    configureMapBounds();
  }, [name, configureMapBounds]);
  const isImageSource = ['image/gif', 'image/jpeg', 'image/png', 'image/webp'].includes(
    source.content_type,
  );
  return (
    <Dialog onOpenChange={(v: boolean) => !v && onCloseAction()} open={true}>
      <DialogContent className="max-w-6xl w-full p-6 max-h-[90vh] overflow-auto">
        <DialogHeader className="mb-6">
          <DialogTitle className="text-2xl flex items-center justify-between">
            <span>
              Inspect Tile Source: <code>{name}</code>
            </span>
          </DialogTitle>
          <DialogDescription>
            Inspect the tile source to explore tile boundaries and properties.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <section className="border rounded-lg overflow-hidden">
            {isImageSource ? (
              <MapLibreMap
                ref={mapRef}
                reuseMaps={false}
                onLoad={() => {
                  // Configure bounds for raster sources after map loads
                  if (tileJSON) {
                    configureMapBounds();
                  }
                }}
                initialViewState={
                  tileJSON?.center
                    ? {
                        longitude: tileJSON.center[0],
                        latitude: tileJSON.center[1],
                        zoom: tileJSON.center[2] ?? 0,
                      }
                    : undefined
                }
                minZoom={tileJSON?.minzoom}
                maxZoom={tileJSON?.maxzoom}
                maxBounds={
                  tileJSON?.bounds
                    ? [
                        [tileJSON.bounds[0], tileJSON.bounds[1]],
                        [tileJSON.bounds[2], tileJSON.bounds[3]],
                      ]
                    : undefined
                }
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
                initialViewState={
                  tileJSON?.center
                    ? {
                        longitude: tileJSON.center[0],
                        latitude: tileJSON.center[1],
                        zoom: tileJSON.center[2] ?? 0,
                      }
                    : undefined
                }
                minZoom={tileJSON?.minzoom}
                maxZoom={tileJSON?.maxzoom}
                maxBounds={
                  tileJSON?.bounds
                    ? [
                        [tileJSON.bounds[0], tileJSON.bounds[1]],
                        [tileJSON.bounds[2], tileJSON.bounds[3]],
                      ]
                    : undefined
                }
                style={{
                  height: '500px',
                  width: '100%',
                }}
              ></MapLibreMap>
            )}
          </section>
          {/* Source Information */}
          <section className="bg-muted/30 p-4 rounded-lg">
            <div className="flex items-center gap-2 mb-2">
              <Database className="w-5 h-5 text-muted-foreground" />
              <h3 className="font-semibold">Source Information</h3>
            </div>
            <div className="flex flex-col md:grid md:grid-cols-2 gap-y-4 text-sm">
              <p>
                <span className="font-medium">Content Type:</span>
                <br />
                <code>{source.content_type}</code>
              </p>
              {source.content_encoding && (
                <p>
                  <span className="font-medium">Encoding:</span>
                  <br />
                  <code>{source.content_encoding}</code>
                </p>
              )}
              {source.name && (
                <p>
                  <span className="font-medium">Name:</span>
                  <br />
                  <span>{source.name}</span>
                </p>
              )}
              {source.description && (
                <p className="col-span-2">
                  <span className="font-medium">Description:</span>
                  <br />
                  <span>{source.description}</span>
                </p>
              )}
              {source.layerCount && (
                <p>
                  <span className="font-medium">Layer Count:</span>
                  <br />
                  <span>{source.layerCount}</span>
                </p>
              )}
              {source.attribution && (
                <p className="col-span-2">
                  <span className="font-medium">Attribution:</span>
                  <br />
                  <span>{source.attribution}</span>
                </p>
              )}
            </div>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
