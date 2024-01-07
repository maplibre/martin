import { TileSourceCatalog } from "../types/tilesets";
import { TileJSON } from "../types/tile-json";

const TILES_SOURCE_PATH = "http://0.0.0.0:3000/";
export const tileSetsSource = {
  getCatalog: async (): Promise<TileSourceCatalog> => {
    const catalogResponse =  await fetch(`${TILES_SOURCE_PATH}/catalog`);
    const catalog = await catalogResponse.json();
    const tileSourceCatalog = catalog.tiles;
    const catalogList = Object.keys(tileSourceCatalog).map((key) => {
      return {
        id: key,
        name: key,
        description: tileSourceCatalog[key].description,
        content_type: tileSourceCatalog[key].content_type,
        content_encoding: tileSourceCatalog[key].content_encoding,
      };
    });
    return { tiles: catalogList};
  },
  getTileJson: async (tileSetId: string): Promise<TileJSON> => {
    return fetch(`${TILES_SOURCE_PATH}/${tileSetId}`).then((response) => {
      return response.json();
    });
  },
};
