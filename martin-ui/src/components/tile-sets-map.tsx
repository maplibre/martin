import React, { useRef } from "react";
import Map, { MapRef } from "react-map-gl/maplibre";
import "mapbox-gl/dist/mapbox-gl.css";
import { useTileSetsStore } from "../stores/tile-set-store";
import { FeaturesPopup } from "./features-popuop/features-popup";
import { MapControls } from "../map-controls/map-controls";
import { useMapConfigStore } from "../stores/map-config-store";
import { getTilesBordersLayer } from "../layers/tile-borders";
import { MBDeckGLOverlay } from "./mb-deckgl-overlay/mb-deckgl-overlay";

export const TileSetsMap = () => {
  const map = useRef<MapRef | null>(null);
  const { showTilesBorders } = useMapConfigStore((state) => ({
    showTilesBorders: state.showTilesBorders,
  }));
  const { deckLayers } = useTileSetsStore((state) => ({
    deckLayers: state.actions.getTileSetsLayers(),
  }));

  const layers = showTilesBorders
    ? [...deckLayers, getTilesBordersLayer()]
    : deckLayers;
  const { mapStyle } = useMapConfigStore((state) => ({
    mapStyle: state.mapStyle,
  }));

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        position: "absolute",
        top: 0,
        left: 0,
      }}
    >
      <Map
        ref={map}
        mapStyle={mapStyle}
      >
        <FeaturesPopup/>
        <MBDeckGLOverlay layers={layers} />
        <MapControls />
      </Map>
    </div>
  );
};
