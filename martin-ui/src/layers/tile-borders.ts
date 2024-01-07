import { useMapConfigStore } from "../stores/map-config-store";
import { PathLayer, Position, TileLayer, TextLayer } from "deck.gl";

export const getTilesBordersLayer = () =>
  new TileLayer({
    id: "tile-borders-layer",
    renderSubLayers: (props) => {
      const {
        bbox: { west, south, east, north },
        id: tileId,
        index,
      } = props.tile;
      return [
        new PathLayer({
          id: "tile-borders-" + tileId,
          data: [
            [
              [west, north],
              [west, south],
              [east, south],
              [east, north],
              [west, north],
            ],
          ],
          getPath: (d) => d as Position[],
          getColor: [255, 0, 0],
          widthMinPixels: 2,
        }),
        new TextLayer({
          id: "tile-borders-text-" + tileId,
          data: [
            {
              text: `${index.z}-${index.x}-${index.y}`,
              position: [west, north],
              color: [255, 255, 255],
            },
          ],
          getTextAnchor: "start",
          getPosition: (d) => d.position as Position,
          getText: (d) => d.text as string,
          getColor: (d) => d.color as number[],
          getSize: 16,
        })
      ];
    },
  });
