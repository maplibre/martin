// lib/types.ts

/**
 * Represents the server metrics for the analytics section.
 */
export interface ServerMetrics {
	requestsPerSecond: number;
	memoryUsage: number;
	cacheHitRate: number;
	activeSources: number;
}

/**
 * A single data point for usage statistics over time.
 */
export interface UsageDataPoint {
	time: string;
	requests: number;
	memory: number;
}

export interface Font {
	name: string;
	family: string;
	weight: number;
	format: "otf" | "ttf" | "ttc";
	sizeInBytes: number;
	usagePerDay: number;
}
/**
 * Represents a tile source with its usage data.
 */
export interface TileSourceData {
	name: string;
	requests: number;
	type: "vector" | "raster" | "sprite" | "font";
	status: "active" | "inactive";
}

/**
 * The complete analytics data structure.
 */
export interface AnalyticsData {
	serverMetrics: ServerMetrics;
	usageData: UsageDataPoint[];
	tileSourcesData: TileSourceData[];
}
export interface Style {
	name: string;
	description: string;
	type: "vector" | "raster" | "hybrid";
	version: string;
	usage: string;
	layers: number;
	colors: string[];
	lastModified: string;
}
/**
 * Represents a data source in the data catalog.
 */
export interface DataSource {
	id: string;
	name: string;
	type: "vector" | "raster";
	description: string;
	layers: number;
	lastUpdatedAt: Date;
	sizeBytes: number;
}

/**
 * Represents a sprite, which can be selected or downloaded.
 * This is a placeholder type and might need to be adjusted based on the actual sprite data.
 */
 export interface SpriteCollection {
	name: string;
	description: string;
	sizeInBytes: number;
	requestsPerDay: number;
	sprites: string[];
 }
