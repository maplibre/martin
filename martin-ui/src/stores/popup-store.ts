import { create } from "zustand";
import { devtools } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";
import { MapboxGeoJSONFeature } from "react-map-gl";

export type PopupStore = {
  visible: boolean;
  position?: [number, number];
  layerId?: string;
  features?: MapboxGeoJSONFeature[];
  actions: {
    setPosition: (position: [number, number]) => void;
    setFeatures: (layerId: string, features: MapboxGeoJSONFeature[]) => void;
  };
};

export const usePopupStore = create<PopupStore>()(
  immer(
    devtools(
      (set) => ({
        visible: false,
        position: undefined,
        content: undefined,
        actions: {
          setPosition: (position) => {
            set((state) => {
              state.position = position;
            });
          },
          setFeatures: (id, features) => {
            set((state) => {
              // @ts-ignore
              state.features = features;
              state.layerId = id;
            });
          },
        },
      }),
      {
        name: "PopupStore",
        enabled: false,
      },
    ),
  ),
);
