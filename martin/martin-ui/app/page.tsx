"use client";

import { type ErrorInfo, useEffect, useState } from "react";
import { AnalyticsSection } from "@/components/analytics-section";
import { FontCatalog } from "@/components/catalogs/font";
import { SpriteCatalog } from "@/components/catalogs/sprite";
import { StylesCatalog } from "@/components/catalogs/styles";
import { TilesCatalog } from "@/components/catalogs/tiles";
import { ErrorBoundary } from "@/components/error/error-boundary";
import { Header } from "@/components/header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toaster } from "@/components/ui/toaster";
import { useAsyncOperation } from "@/hooks/use-async-operation";
import { useToast } from "@/hooks/use-toast";
import type {
	AnalyticsData,
	CatalogSchema,
} from "@/lib/types";

// Simulate API functions that can fail
const fetchAnalytics = async (): Promise<AnalyticsData> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 400));

	// Simulate random failures
	if (Math.random() < 0.2) {
		throw new Error(`Failed to fetch analytics`);
	}

	return {
		requestsPerSecond: 1247,
		memoryUsage: 68,
		cacheHitRate: 94.2,
		activeSources: 23,
	};
};

const fetchCatalog = async (): Promise<CatalogSchema> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 200));

	return {
		tiles: {
			"osm-bright": {
				name: "OSM Bright",
				content_type: "application/x-protobuf",
				content_encoding: "gzip",
				description: "OpenStreetMap data with bright styling",
				layers: 12,
				lastModifiedAt: new Date(Date.now() - 2 * 60 * 60 * 1000),
			},
			sattelite: {
				name: "Satellite Imagery",
				content_type: "image/png",
				description: "High-resolution satellite imagery",
				layers: 1,
				lastModifiedAt: new Date(Date.now() - 24 * 60 * 60 * 1000),
			},
			terrain: {
				name: "Terrain Contours",
				content_type: "application/x-protobuf",
				content_encoding: "zlib",
				description: "Elevation contours and terrain features",
				layers: 8,
				lastModifiedAt: new Date(Date.now() - 6 * 60 * 60 * 1000),
			},
			pois: {
				name: "POIs",
				content_type: "application/x-protobuf",
				description: "Point of interest icons and markers",
				layers: 1,
			},
		},
		styles: {
			"osm-bright": {
				description: "Clean and bright OpenStreetMap styling",
				type: "vector",
				version: "1.2.0",
				usage: "45,230 requests/day",
				layers: 12,
				colors: ["#ffffff", "#f8f8f8", "#e8e8e8", "#4a90e2"],
				lastModified: "2 days ago",
			},
			dark: {
				description: "Modern dark theme for night viewing",
				type: "vector",
				version: "2.1.0",
				usage: "32,180 requests/day",
				layers: 15,
				colors: ["#1a1a1a", "#2d2d2d", "#404040", "#8b5cf6"],
				lastModified: "1 week ago",
			},
			"satelite-hybrid": {
				description: "Satellite imagery with vector overlays",
				type: "hybrid",
				version: "1.0.3",
				usage: "28,450 requests/day",
				layers: 8,
				colors: ["#2c5234", "#4a7c59", "#8fbc8f", "#ffffff"],
				lastModified: "3 days ago",
			},
			terrain: {
				description: "Topographic style with elevation contours",
				type: "vector",
				version: "1.5.2",
				usage: "18,920 requests/day",
				layers: 18,
				colors: ["#f4f1de", "#e07a5f", "#3d405b", "#81b29a"],
				lastModified: "5 days ago",
			},
			minimal: {
				description: "Clean minimal style for data visualization",
				type: "vector",
				version: "1.0.0",
				usage: "22,340 requests/day",
				layers: 6,
				colors: ["#ffffff", "#f5f5f5", "#cccccc", "#666666"],
				lastModified: "1 day ago",
			},
			retro: {
				description: "Vintage-inspired map styling",
				type: "vector",
				version: "1.3.1",
				usage: "12,670 requests/day",
				layers: 14,
				colors: ["#f7e7ce", "#d4a574", "#8b4513", "#2f4f4f"],
				lastModified: "1 week ago",
			},
		},
		fonts: {
			"Roboto Regular": {
				family: "Roboto",
				style: "Regular",
				format: "ttf",
				glyphs: 156 * 1024,
				start: 0,
				end: 65535,
			},
			"Roboto Bold": {
				family: "Roboto",
				style: "Bold",
				format: "ttf",
				glyphs: 164 * 1024,
				start: 0,
				end: 65535,
			},
			"Open Sans Regular": {
				family: "Open Sans",
				style: "Regular",
				format: "ttc",
				glyphs: 142 * 1024,
				start: 0,
				end: 65535,
			},
			"Noto Sans CJK": {
				family: "Noto Sans",
				style: "Regular",
				format: "otf",
				glyphs: 2.1 * 1024 * 1024,
				start: 0,
				end: 65535,
			},
			"Source Code Pro": {
				family: "Source Code Pro",
				style: "Monospace",
				format: "ttf",
				glyphs: 198 * 1024,
				start: 0,
				end: 65535,
			},
			"Inter Medium": {
				family: "Inter",
				style: "Medium",
				format: "ttc",
				glyphs: 178 * 1024,
				start: 0,
				end: 65535,
			},
		},
		sprites: {
			pois: {
				sizeInBytes: 230 * 1024,
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
			},
			transportation: {
				sizeInBytes: 180 * 1024,
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
			},
			amenities: {
				sizeInBytes: 210 * 1024,
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
			},
			recreation: {
				sizeInBytes: 140 * 1024,
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
			},
			shopping: {
				sizeInBytes: 160 * 1024,
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
			},
			customMarkers: {
				sizeInBytes: 89 * 1024,
				images: [
					"brand-a-marker-icon",
					"brand-b-marker-icon",
					"special-event-icon",
					"promotion-icon",
					"new-location-icon",
					"featured-icon",
				],
			},
		},
	};
};

export default function MartinTileserverDashboard() {
	const [searchQuery, setSearchQuery] = useState("");
	const { toast } = useToast();

	// Analytics operation
	const analyticsOperation = useAsyncOperation<AnalyticsData>(fetchAnalytics, {
		showErrorToast: false, // We handle errors in the component
		onError: (error) => {
			console.error("Analytics fetch failed:", error);
		},
	});

	// Catalog operation - unified data fetching
	const catalogOperation = useAsyncOperation(fetchCatalog, {
		showErrorToast: false,
		onError: (error) => {
			console.error("Catalog fetch failed:", error);
		},
	});

	// Load initial data
	useEffect(() => {
		analyticsOperation.execute();
		catalogOperation.execute();
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);

	return (
		<ErrorBoundary
			onError={(error: Error, errorInfo: ErrorInfo) => {
				console.error("Application error:", error, errorInfo);
				toast({
					variant: "destructive",
					title: "Application Error",
					description:
						"An unexpected error occurred. The page will reload automatically.",
				});

				// Auto-reload after 3 seconds
				setTimeout(() => {
					window.location.reload();
				}, 3000);
			}}
		>
			<div className="min-h-screen bg-background">
				<Header />

				<div className="container mx-auto px-6 py-8">
					<AnalyticsSection
						analytics={analyticsOperation.data}
						isLoading={analyticsOperation.isLoading}
						error={analyticsOperation.error}
						onRetry={analyticsOperation.retry}
						isRetrying={analyticsOperation.isRetrying}
					/>

					<Tabs defaultValue="catalog" className="space-y-6">
						<TabsList className="grid w-full grid-cols-4">
							<TabsTrigger value="tiles">Data Catalog</TabsTrigger>
							<TabsTrigger value="styles">Styles Catalog</TabsTrigger>
							<TabsTrigger value="fonts">Font Catalog</TabsTrigger>
							<TabsTrigger value="sprites">Sprite Catalog</TabsTrigger>
						</TabsList>

						<TabsContent value="tiles">
							<TilesCatalog
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
								tileSources={catalogOperation.data?.tiles}
								isLoading={catalogOperation.isLoading}
								error={catalogOperation.error}
								onRetry={catalogOperation.retry}
								isRetrying={catalogOperation.isRetrying}
							/>
						</TabsContent>

						<TabsContent value="styles">
							<StylesCatalog
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
								styles={catalogOperation.data?.styles}
								isLoading={catalogOperation.isLoading}
								error={catalogOperation.error}
								onRetry={catalogOperation.retry}
								isRetrying={catalogOperation.isRetrying}
							/>
						</TabsContent>

						<TabsContent value="fonts">
							<FontCatalog
								fonts={catalogOperation.data?.fonts}
								isLoading={catalogOperation.isLoading}
								error={catalogOperation.error}
								onRetry={catalogOperation.retry}
								isRetrying={catalogOperation.isRetrying}
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
							/>
						</TabsContent>

						<TabsContent value="sprites">
							<SpriteCatalog
								spriteCollections={catalogOperation.data?.sprites}
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
								isLoading={catalogOperation.isLoading}
								error={catalogOperation.error}
								onRetry={catalogOperation.retry}
								isRetrying={catalogOperation.isRetrying}
							/>
						</TabsContent>
					</Tabs>
				</div>

				<Toaster />
			</div>
		</ErrorBoundary>
	);
}
