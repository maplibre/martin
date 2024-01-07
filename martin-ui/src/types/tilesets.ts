export interface TileSetCatalogItem {
  id: string;
  name: string;
  content_encoding: "gzip",
  content_type: "application/x-protobuf";
  description: string;
}

export type TileSourceCatalog = {
  tiles: TileSetCatalogItem[];
};
