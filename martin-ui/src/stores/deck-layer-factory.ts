import { MVTLayerProps } from "@deck.gl/geo-layers/mvt-layer/mvt-layer";
import { TileSet } from "./tile-set-store";

export const deckGlLayerFactory = {
  create: (id: string, tileSet: TileSet): MVTLayerProps<any> => {
    const { tilejson } = tileSet;
    return {
      visible: true,
      id,
      data: tilejson.tiles,
      highlightColor: [0, 0, 128, 128],
      lineWidthMinPixels: 2,
      lineWidthUnits: "pixels",
      minZoom: tilejson.minzoom,
      maxZoom: tilejson.maxzoom,
      pickable: true,
      autoHighlight: true,
      // @ts-ignore
      getLineColor: (a, b, c) => {
        return [255, 0, 0, 180];
      },
      getFillColor: [0, 0, 255, 40],
      binary: false,
    };
  },
};
