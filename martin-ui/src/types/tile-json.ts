export interface TileJSON {
  tilejson: string; // REQUIRED
  tiles: string[]; // REQUIRED
  vector_layers: VectorLayer[];
  attribution?: string;
  bounds?: [number, number, number, number];
  center?: [number, number, number];
  data?: string[];
  description?: string;
  fillzoom?: number;
  grids?: string[];
  legend?: any;
  maxzoom?: number;
  minzoom?: number;
  name?: string;
  scheme?: "xyz" | "tms";
  template?: string;
  [key: string]: any;
}

export interface VectorLayer {
  id: string; // REQUIRED
  fields: Record<string, string>; // REQUIRED
  description?: string;
  minzoom?: number;
  maxzoom?: number;
  [key: string]: any;
}
