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

/**
 * Represents a data source in the data catalog.
 */
export interface DataSource {
	id: string;
	name: string;
	type: string;
	description: string;
	layers: number;
	lastUpdated: string;
	size: string;
}

/**
 * Represents a sprite, which can be selected or downloaded.
 * This is a placeholder type and might need to be adjusted based on the actual sprite data.
 */
export interface Sprite {
	id: string;
	name: string;
	url: string;
	width: number;
	height: number;
}
