export interface TileSource {
  content_type: string;
  description?: string;
  content_encoding?: string;
  name?: string;
  attribution?: string;
}

export interface SpriteCollection {
  images: string[];
}

export interface Style {
  path: string;
}
