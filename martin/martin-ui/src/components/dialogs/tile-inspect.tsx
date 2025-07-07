"use client";

import { useEffect, useRef, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { TileSource } from "@/lib/types";
import "@maplibre/maplibre-gl-inspect/dist/maplibre-gl-inspect.css";
import MaplibreInspect from "@maplibre/maplibre-gl-inspect";
import type { MapRef } from "@vis.gl/react-maplibre";
import { Map as MapLibreMap, Source } from "@vis.gl/react-maplibre";
import { Popup } from "maplibre-gl";
import { buildMartinUrl } from "@/lib/api";

interface TileInspectDialogProps {
  name: string;
  source: TileSource;
  onCloseAction: () => void;
}

export function TileInspectDialog({ name, source, onCloseAction }: TileInspectDialogProps) {
  const mapRef = useRef<MapRef>(null);
  const [isMapLoaded, setIsMapLoaded] = useState(false);
  const inspectControlRef = useRef<MaplibreInspect | null>(null);

  useEffect(() => {
    if (!isMapLoaded || !mapRef.current) return;

    const map = mapRef.current.getMap();

    console.log({ isMapLoaded, map });
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

    // Cleanup function
    return () => {
      if (inspectControlRef.current && map) {
        try {
          map.removeControl(inspectControlRef.current);
        } catch (_e) {
          // Control might already be removed
        }
      }
    };
  }, [isMapLoaded]);

  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
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
          {/* Map Container */}
          <div className="border rounded-lg overflow-hidden">
            <MapLibreMap
              onLoad={() => setIsMapLoaded(true)}
              ref={mapRef}
              reuseMaps={false}
              style={{
                height: "500px",
                width: "100%",
              }}
            >
              <Source type="vector" url={buildMartinUrl(`/${name}`)} />
            </MapLibreMap>
          </div>
          {/* Source Information */}
          <div className="bg-muted/30 p-4 rounded-lg">
            <h3 className="font-semibold mb-2">Source Information</h3>
            <div className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <span className="font-medium">Content Type:</span>
                <span className="ml-2">{source.content_type}</span>
              </div>
              {source.content_encoding && (
                <div>
                  <span className="font-medium">Encoding:</span>
                  <span className="ml-2">{source.content_encoding}</span>
                </div>
              )}
              {source.name && (
                <div>
                  <span className="font-medium">Name:</span>
                  <span className="ml-2">{source.name}</span>
                </div>
              )}
              {source.description && (
                <div>
                  <span className="font-medium">Description:</span>
                  <span className="ml-2">{source.description}</span>
                </div>
              )}
              {source.layerCount && (
                <div>
                  <span className="font-medium">Layer Count:</span>
                  <span className="ml-2">{source.layerCount}</span>
                </div>
              )}
              {source.attribution && (
                <div className="col-span-2">
                  <span className="font-medium">Attribution:</span>
                  <span className="ml-2">{source.attribution}</span>
                </div>
              )}
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
