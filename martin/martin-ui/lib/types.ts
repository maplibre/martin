export interface CatalogSchema {
  tiles: { [tile_id: string]: TileSource };
  sprites: {[sprite_collection_id: string]: SpriteCollection};
  fonts: { [name: string]: Font };
  styles: {[name: string]: Style};
}

export interface Font {
	// the group of fonts that are used in the application
   // Example
   // - "Roboto Medium" has the family of Roboto
   // - "Roboto Condensed Medium Italic" has family "Roboto Condensed"
	family: string;
	// if the style is Medium, Bold, Italic, Bold Italic, ..
  style: string;
  format?: "otf" | "ttf" | "ttc"; // todo: make this provided as required upstream
  start: number; // todo: what is this?
  end: number; // todo: what is this?
	glyphs: number;
	lastModifiedAt?: Date; // todo: make this provided as required upstream
}

export interface Style {
  path: string;
	type?: "vector" | "raster" | "hybrid"; // todo: make this provided as required upstream
	versionHash?: string; // todo: make this provided as required upstream
	layerCount?: number; // todo: make this provided as required upstream
	colors?: string[]; // todo: make this provided as required upstream
	lastModifiedAt?: Date; // todo: make this provided as required upstream
}
/**
 * Represents a data source in the data catalog.
 */
export interface TileSource {
  // application/x-protobuf, image/... 
	content_type: string;
	// for example gzip
	content_encoding?: string;
	name?: string;
	description?: string;
	attribution?: string;
	layerCount?: number; // todo: make this provided as required upstream
	lastModifiedAt?: Date; // todo: make this provided as required upstream
}

/**
 * Represents a sprite, which can be selected or downloaded.
 * This is a placeholder type and might need to be adjusted based on the actual sprite data.
 */
 export interface SpriteCollection {
	images: string[];
	sizeInBytes?: number; // todo: make this provided as required upstream
	lastModifiedAt?: Date; // todo: make this provided as required upstream
 }

 /**
  * Usage and performance metrics
  */
 export interface AnalyticsData {
	requestsPerSecond: number;
	memoryUsage: number;
	cacheHitRate: number;
	activeSources: number;
 }
