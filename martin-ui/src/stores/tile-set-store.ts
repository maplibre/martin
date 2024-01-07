import { create } from "zustand";
import { devtools } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";
import { TileJSON } from "../types/tile-json";
import { MVTLayerProps } from "@deck.gl/geo-layers/mvt-layer/mvt-layer";
import { tileSetsSource } from "../services/tiles-source";
import { TileSourceCatalog } from "../types/tilesets";
import { enableMapSet } from "immer";
import KeplerTable from "@kepler.gl/table";
import { VisConfig } from "./types";
import { colorMaker } from "@kepler.gl/layers";
import { processGeojson } from "@kepler.gl/processors";
import { ProtoDataset } from "@kepler.gl/types";
import { MVTLayer } from "@deck.gl/geo-layers";

enableMapSet();

export interface TileSet {
  id: string;
  tilejson: TileJSON;
  keplerTable?: KeplerTable;
  visConfig?: VisConfig;
}

export type TileSetStore = {
  catalog: TileSourceCatalog;
  ids: Set<string>;
  selectedIds: Set<string>;
  tileSets: Map<string, TileSet>;
  actions: {
    // addTileSet: (path: string) => string;
    // removeTileSet: (tileSetId: string) => void;
    initCatalog: () => Promise<void>;
    selectTileSet: (tileSetId: string) => void;
    unselectTileSet: (tileSetId: string) => void;
    getSelectedTileSets: () => TileSet[];
    setSelectedTileSetsIds: (tileSetIds: string[]) => void;
    setKeplerTable: (id: string, geojson: any) => void;
    getTileSetsLayers: () => MVTLayer<any>[];
  };
};

export const getDeckLayer = ({ id, tilejson, visConfig }: TileSet): MVTLayer<any> => {
  return new MVTLayer({
    id: id,
    data: tilejson.tiles,
    pickable: true,
    getFillColor: visConfig.color,
    getLineColor: [0, 0, 0],
    getLineWidth: 1,
    getPointRadius: 1,
    lineWidthMinPixels: 1,
    pointRadiusMinPixels: 1,
    autoHighlight: true,
    lineWidthUnits: "pixels",
    minZoom: tilejson.minzoom,
    maxZoom: tilejson.maxzoom,
    highlightColor: [255, 255, 255, 100],
    opacity: 0.3,
  } as MVTLayerProps<any>);
}
export const useTileSetsStore = create<TileSetStore>()(
  immer(
    devtools(
      (set, get) => ({
        catalog: {
          tiles: [],
        },
        ids: new Set(),
        selectedIds: new Set(),
        tileSets: new Map(),
        keplerTableMap: new Map(),
        actions: {
          initCatalog: async () => {
            const catalog = await (tileSetsSource.getCatalog() as Promise<TileSourceCatalog>);
            const getTileJsonPromises = catalog.tiles.map(async ({ id }) =>
              tileSetsSource.getTileJson(id),
            );
            const tileJsons = await Promise.all(getTileJsonPromises);

            set((state) => {
              state.catalog = catalog;
              tileJsons.forEach((tileJson) => {
                const id = window.crypto.randomUUID();
                const visConfig = initVisConfig(tileJson);
                state.ids.add(id);
                state.tileSets.set(id, {
                  id,
                  tilejson: tileJson,
                  visConfig,
                  keplerTable: undefined,
                });
              });
            });
          },
          selectTileSet: (id) => {
            set((state) => {
              if (!state.selectedIds.has(id)) {
                state.selectedIds.add(id);
              }
            });
          },
          unselectTileSet: (tileSetId: string) => {
            set((state) => {
              if (state.selectedIds.has(tileSetId)) {
                state.selectedIds.delete(tileSetId);
              }
            });
          },
          getSelectedTileSets: () => {
            const selectedIds = get().selectedIds;
            const tileSets = get().tileSets;
            return [...selectedIds.values()].map((id) => tileSets.get(id)!);
          },
          setSelectedTileSetsIds: (tileSetIds: string[]) => {
            set((state) => {
              state.selectedIds = new Set(tileSetIds);
            });
          },
          setKeplerTable: (id, geojson) => {
            set((state) => {
              const keplerData = processGeojson(geojson);
              const tileSet = state.tileSets.get(id);
              if (!tileSet) {
                console.warn(
                  "trying to set kepler table for non existing tileset",
                  id,
                );
                return;
              }
              const { visConfig, tilejson } = tileSet;
              const table = new KeplerTable({
                data: keplerData as ProtoDataset["data"],
                color: visConfig?.color ?? colorMaker.next().value,
                info: {
                  id: id,
                  label: tilejson.name,
                },
              });

              state.tileSets.get(id)!.keplerTable = table;
            });
          },
          getTileSetsLayers: () => {
            const selectedTileSets = get().actions.getSelectedTileSets();
            return selectedTileSets.map(getDeckLayer);
          },
        },
      }),
      {
        name: "TileSetStore",
        enabled: false,
      },
    ),
  ),
);

export const initVisConfig = (tileJson: TileJSON): VisConfig => {
  const visConfig: VisConfig = {
    color: colorMaker.next().value,
  };
  return visConfig;
};
