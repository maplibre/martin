import { create } from "zustand";
import { MAPBOX_STYLES } from "../config/mapbox-styles";
import { MapboxOverlay } from "@deck.gl/mapbox/typed";
import {devtools} from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

interface MapConfigStore {
  mapStyle: string;
  setMapStyle: (mapStyle: string) => void;
  showPosition: boolean;
  setShowPosition: (showPosition: boolean) => void;
  showTilesBorders: boolean;
  setShowTilesBorders: (show: boolean) => void;
  deckOverlayInstance?: MapboxOverlay;
  setDeckOverlayInstance: (overlayInstance: MapboxOverlay) => void;
}
export const useMapConfigStore = create<MapConfigStore>(
  immer(
    devtools((set, get) => ({
      mapStyle: MAPBOX_STYLES[0].url,
      showTilesBorders: false,
      showPosition: false,
      deckOverlayInstance: undefined,
      setMapStyle: (mapStyle) => set((state) => {
        state.mapStyle = mapStyle
      }),
      setShowPosition: (showPosition) => set((state) => {
        state.showPosition = showPosition
      }),
      setShowTilesBorders: (showTilesBorders) =>
        set((state) => {
          state.showTilesBorders = showTilesBorders
        }),
      setDeckOverlayInstance: (deckInstance) => set((state) => {
        state.deckOverlayInstance = deckInstance;
      }),
    }),
      {
        name: "MapConfigStore",
        enabled: false,
      }
    )
  )
);

