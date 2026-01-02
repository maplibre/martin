'use client';

import { useCallback, useId, useRef } from 'react';
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

interface TileInspectDialogProps {
  name: string;
  source: TileSource;
  onCloseAction: () => void;
}

export function TileInspectDialog({ name, source, onCloseAction }: TileInspectDialogProps) {
  const id = useId();
  const mapRef = useRef<MapRef>(null);
  const inspectControlRef = useRef<MaplibreInspect>(null);

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
  }, [name]);
  const isImageSource = ['image/gif', 'image/jpeg', 'image/png', 'image/webp'].includes(
    source.content_type,
  );
  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
      <DialogContent className="max-w-6xl w-full p-6 max-h-[90vh] overflow-auto">
        <DialogHeader className="mb-6 truncate">
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
