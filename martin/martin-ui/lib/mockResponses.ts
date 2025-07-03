import { aggregateEndpointGroups, ENDPOINT_GROUPS, parsePrometheusMetrics } from "./prometheus";
import type { AnalyticsData, CatalogSchema } from "./types";

/**
 * Returns mock analytics data for Martin dashboard.
 * Simulates a 10% failure rate and logs when the mock is used.
 */
export function getMartinMockAnalytics(): AnalyticsData {
  // 10% chance to fail
  if (Math.random() < 0.1) {
    console.log("[MOCK] getMartinMockAnalytics: Simulated failure");
    throw new Error("Simulated mock analytics failure");
  }
  console.log("[MOCK] getMartinMockAnalytics: Returning mock analytics data");
  return {
    fonts: { averageRequestDurationMs: 2, requestCount: 17 },
    sprites: { averageRequestDurationMs: 42, requestCount: 5 },
    styles: { averageRequestDurationMs: 1, requestCount: 13 },
    tiles: { averageRequestDurationMs: 32, requestCount: 99 },
  };
}

/**
 * Returns mock catalog data for Martin dashboard.
 * Simulates a 10% failure rate and logs when the mock is used.
 */
export function getMartinMockCatalog(): CatalogSchema {
  // 10% chance to fail
  if (Math.random() < 0.1) {
    console.log("[MOCK] getMartinMockCatalog: Simulated failure");
    throw new Error("Simulated mock catalog failure");
  }
  console.log("[MOCK] getMartinMockCatalog: Returning mock catalog data");
  return {
    fonts: {
      "Inter Medium": {
        end: 65535,
        family: "Inter",
        format: "ttc",
        glyphs: 178 * 1024,
        start: 0,
        style: "Medium",
      },
      "Noto Sans CJK": {
        end: 65535,
        family: "Noto Sans",
        format: "otf",
        glyphs: 2.1 * 1024 * 1024,
        start: 0,
        style: "Regular",
      },
      "Open Sans Regular": {
        end: 65535,
        family: "Open Sans",
        format: "ttc",
        glyphs: 142 * 1024,
        start: 0,
        style: "Regular",
      },
      "Roboto Bold": {
        end: 65535,
        family: "Roboto",
        format: "ttf",
        glyphs: 164 * 1024,
        start: 0,
        style: "Bold",
      },
      "Roboto Regular": {
        end: 65535,
        family: "Roboto",
        format: "ttf",
        glyphs: 156 * 1024,
        start: 0,
        style: "Regular",
      },
      "Source Code Pro": {
        end: 65535,
        family: "Source Code Pro",
        format: "ttf",
        glyphs: 198 * 1024,
        start: 0,
        style: "Monospace",
      },
    },
    sprites: {
      amenities: {
        images: [
          "wifi-icon",
          "restroom-icon",
          "information-icon",
          "wheelchair-icon",
          "elevator-icon",
          "stairs-icon",
          "drinking-water-icon",
          "phone-icon",
        ],
        sizeInBytes: 210 * 1024,
      },
      customMarkers: {
        images: [
          "brand-a-marker-icon",
          "brand-b-marker-icon",
          "special-event-icon",
          "promotion-icon",
          "new-location-icon",
          "featured-icon",
        ],
        sizeInBytes: 89 * 1024,
      },
      pois: {
        images: [
          "restaurant-icon",
          "hotel-icon",
          "gas-station-icon",
          "hospital-icon",
          "bank-icon",
          "atm-icon",
          "pharmacy-icon",
          "school-icon",
          "library-icon",
          "post-office-icon",
          "police-icon",
          "fire-station-icon",
        ],
        sizeInBytes: 230 * 1024,
      },
      recreation: {
        images: [
          "park-icon",
          "playground-icon",
          "stadium-icon",
          "beach-icon",
          "swimming-icon",
          "tennis-icon",
          "golf-icon",
          "hiking-icon",
        ],
        sizeInBytes: 140 * 1024,
      },
      shopping: {
        images: [
          "shopping-mall-icon",
          "grocery-store-icon",
          "clothing-store-icon",
          "electronics-icon",
          "bookstore-icon",
          "flower-shop-icon",
          "jewelry-icon",
          "bakery-icon",
        ],
        sizeInBytes: 160 * 1024,
      },
      transportation: {
        images: [
          "bus-stop-icon",
          "train-station-icon",
          "airport-icon",
          "parking-icon",
          "subway-icon",
          "taxi-icon",
          "bicycle-icon",
          "car-rental-icon",
        ],
        sizeInBytes: 180 * 1024,
      },
    },
    styles: {
      dark: {
        colors: ["#1a1a1a", "#2d2d2d", "#404040", "#8b5cf6"],
        lastModifiedAt: new Date(Date.now() - 7 * 24 * 60 * 60 * 1000),
        layerCount: 15,
        path: "/styles/dark/style.json",
        type: "vector",
      },
      minimal: {
        colors: ["#ffffff", "#f5f5f5", "#cccccc", "#666666"],
        lastModifiedAt: new Date(Date.now() - 1 * 24 * 60 * 60 * 1000),
        layerCount: 6,
        path: "/styles/minimal/style.json",
        type: "vector",
      },
      "osm-bright": {
        colors: ["#ffffff", "#f8f8f8", "#e8e8e8", "#4a90e2"],
        lastModifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000),
        layerCount: 12,
        path: "/styles/osm-bright/style.json",
        type: "vector",
      },
      retro: {
        colors: ["#f7e7ce", "#d4a574", "#8b4513", "#2f4f4f"],
        lastModifiedAt: new Date(Date.now() - 7 * 24 * 60 * 60 * 1000),
        layerCount: 14,
        path: "/retro.json",
        type: "vector",
      },
      "satelite-hybrid": {
        colors: ["#2c5234", "#4a7c59", "#8fbc8f", "#ffffff"],
        lastModifiedAt: new Date(Date.now() - 3 * 24 * 60 * 60 * 1000),
        layerCount: 8,
        path: "/styles/satelite-hybrid/style.json",
        type: "hybrid",
      },
      terrain: {
        colors: ["#f4f1de", "#e07a5f", "#3d405b", "#81b29a"],
        lastModifiedAt: new Date(Date.now() - 5 * 24 * 60 * 60 * 1000),
        layerCount: 18,
        path: "/styles/terrain.json",
        type: "vector",
      },
    },
    tiles: {
      "osm-bright": {
        content_encoding: "gzip",
        content_type: "application/x-protobuf",
        description: "OpenStreetMap data with bright styling",
        lastModifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000),
        layerCount: 12,
        name: "OSM Bright",
      },
      pois: {
        content_type: "application/x-protobuf",
        description: "Point of interest icons and markers",
        layerCount: 1,
        name: "POIs",
      },
      sattelite: {
        content_type: "image/png",
        description: "High-resolution satellite imagery",
        lastModifiedAt: new Date(Date.now() - 24 * 60 * 60 * 1000),
        layerCount: 1,
        name: "Satellite Imagery",
      },
      terrain: {
        content_encoding: "zlib",
        content_type: "application/x-protobuf",
        description: "Elevation contours and terrain features",
        lastModifiedAt: new Date(Date.now() - 6 * 60 * 60 * 1000),
        layerCount: 8,
        name: "Terrain Contours",
      },
    },
  };
}
