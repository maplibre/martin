"use client";

import { type ErrorInfo, useEffect, useState } from "react";
import { AnalyticsSection } from "@/components/analytics-section";
import { DataCatalog } from "@/components/catalogs/data";
import { FontCatalog } from "@/components/catalogs/font";
import { SpriteCatalog } from "@/components/catalogs/sprite";
import { StylesCatalog } from "@/components/catalogs/styles";
import { ErrorBoundary } from "@/components/error/error-boundary";
import { Header } from "@/components/header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toaster } from "@/components/ui/toaster";
import { useAsyncOperation } from "@/hooks/use-async-operation";
import { useToast } from "@/hooks/use-toast";
import type { AnalyticsData, DataSource, Font, SpriteCollection, Style } from "@/lib/types";

// Simulate API functions that can fail
const fetchAnalytics = async (): Promise<AnalyticsData> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 1000));

	// Simulate random failures
	if (Math.random() < 0.2) {
		throw new Error(`Failed to fetch analytics data`);
	}

	return {
		serverMetrics: {
			requestsPerSecond: 1247,
			memoryUsage: 68,
			cacheHitRate: 94.2,
			activeSources: 23,
		},
		usageData: [
			{ time: "00:00", requests: 400, memory: 45 },
			{ time: "04:00", requests: 300, memory: 42 },
			{ time: "08:00", requests: 800, memory: 55 },
			{ time: "12:00", requests: 1200, memory: 68 },
			{ time: "16:00", requests: 1400, memory: 72 },
			{ time: "20:00", requests: 900, memory: 58 },
		],
		tileSourcesData: [
			{ name: "osm-bright", requests: 45000, type: "vector", status: "active" },
			{
				name: "satellite-imagery",
				requests: 32000,
				type: "raster",
				status: "active",
			},
			{
				name: "terrain-contours",
				requests: 18000,
				type: "vector",
				status: "active",
			},
			{
				name: "poi-markers",
				requests: 12000,
				type: "sprite",
				status: "active",
			},
			{ name: "custom-fonts", requests: 8000, type: "font", status: "active" },
		],
	};
};

const fetchDataSources = async (): Promise<DataSource[]> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 1200));

	return [
		{
			id: "osm-bright",
			name: "OSM Bright",
			type: "vector",
			description: "OpenStreetMap data with bright styling",
			layers: 12,
			lastUpdatedAt: new Date(Date.now() - 2 * 60 * 60 * 1000),
			sizeBytes: 2 * 1024 * 1024 * 1024,
		},
		{
			id: "satellite",
			name: "Satellite Imagery",
			type: "raster",
			description: "High-resolution satellite imagery",
			layers: 1,
			lastUpdatedAt: new Date(Date.now() - 24 * 60 * 60 * 1000),
			sizeBytes: 14 * 1024 * 1024 * 1024,
		},
		{
			id: "terrain",
			name: "Terrain Contours",
			type: "vector",
			description: "Elevation contours and terrain features",
			layers: 8,
			lastUpdatedAt: new Date(Date.now() - 6 * 60 * 60 * 1000),
			sizeBytes: 1 * 1024 * 1024 * 1024,
		},
		{
			id: "pois",
			name: "POIs",
			type: "vector",
			description: "Point of interest icons and markers",
			layers: 1,
			lastUpdatedAt: new Date(Date.now() - 3 * 24 * 60 * 60 * 1000),
			sizeBytes: 33 * 1024 * 1024,
		},
	];
};

const fetchStyles = async (): Promise<Style[]> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 800));

	return [
		{
			name: "OSM Bright",
			description: "Clean and bright OpenStreetMap styling",
			type: "vector",
			version: "1.2.0",
			usage: "45,230 requests/day",
			layers: 12,
			colors: ["#ffffff", "#f8f8f8", "#e8e8e8", "#4a90e2"],
			lastModified: "2 days ago",
		},
		{
			name: "Dark Theme",
			description: "Modern dark theme for night viewing",
			type: "vector",
			version: "2.1.0",
			usage: "32,180 requests/day",
			layers: 15,
			colors: ["#1a1a1a", "#2d2d2d", "#404040", "#8b5cf6"],
			lastModified: "1 week ago",
		},
		{
			name: "Satellite Hybrid",
			description: "Satellite imagery with vector overlays",
			type: "hybrid",
			version: "1.0.3",
			usage: "28,450 requests/day",
			layers: 8,
			colors: ["#2c5234", "#4a7c59", "#8fbc8f", "#ffffff"],
			lastModified: "3 days ago",
		},
		{
			name: "Terrain",
			description: "Topographic style with elevation contours",
			type: "vector",
			version: "1.5.2",
			usage: "18,920 requests/day",
			layers: 18,
			colors: ["#f4f1de", "#e07a5f", "#3d405b", "#81b29a"],
			lastModified: "5 days ago",
		},
		{
			name: "Minimal",
			description: "Clean minimal style for data visualization",
			type: "vector",
			version: "1.0.0",
			usage: "22,340 requests/day",
			layers: 6,
			colors: ["#ffffff", "#f5f5f5", "#cccccc", "#666666"],
			lastModified: "1 day ago",
		},
		{
			name: "Retro",
			description: "Vintage-inspired map styling",
			type: "vector",
			version: "1.3.1",
			usage: "12,670 requests/day",
			layers: 14,
			colors: ["#f7e7ce", "#d4a574", "#8b4513", "#2f4f4f"],
			lastModified: "1 week ago",
		},
	];
};

const fetchFonts = async (): Promise<Font[]> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 900));

	return [
		{
			name: "Roboto Regular",
			family: "Roboto",
			weight: 400,
			format: "ttf",
			sizeInBytes: 156 * 1024,
			usagePerDay: 12450,
		},
		{
			name: "Roboto Bold",
			family: "Roboto",
			weight: 700,
			format: "ttf",
			sizeInBytes: 164 * 1024,
			usagePerDay: 8230,
		},
		{
			name: "Open Sans Regular",
			family: "Open Sans",
			weight: 400,
			format: "ttc",
			sizeInBytes: 142 * 1024,
			usagePerDay: 15680,
		},
		{
			name: "Noto Sans CJK",
			family: "Noto Sans",
			weight: 400,
			format: "otf",
			sizeInBytes: 2.1 * 1024 * 1024,
			usagePerDay: 3420,
		},
		{
			name: "Source Code Pro",
			family: "Source Code Pro",
			weight: 400,
			format: "ttf",
			sizeInBytes: 198 * 1024,
			usagePerDay: 1890,
		},
		{
			name: "Inter Medium",
			family: "Inter",
			weight: 500,
			format: "ttc",
			sizeInBytes: 178 * 1024,
			usagePerDay: 9340,
		},
	];
};

const fetchSprites = async (): Promise<SpriteCollection[]> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 1100));

	return [
		{
			name: "POI Icons",
			description: "Point of interest markers and symbols",
			sizeInBytes: 23 * 1024 * 1024,
			requestsPerDay: 45230,
			sprites: [
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
		{
			name: "Transportation",
			description: "Transit and transportation related icons",
			sizeInBytes: 18 * 1024 * 1024,
			requestsPerDay: 32180,
			sprites: [
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
		{
			name: "Amenities",
			description: "Public amenities and services",
			sizeInBytes: 21 * 1024 * 1024,
			requestsPerDay: 28450,
			sprites: [
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
		{
			name: "Recreation",
			description: "Parks, sports, and recreational facilities",
			sizeInBytes: 14 * 1024 * 1024,
			requestsPerDay: 18920,
			sprites: [
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
		{
			name: "Shopping",
			description: "Retail and commercial establishments",
			sizeInBytes: 16 * 1024 * 1024,
			requestsPerDay: 22340,
			sprites: [
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
		{
			name: "Custom Markers",
			description: "Custom branded location markers",
			sizeInBytes: 890 * 1024,
			requestsPerDay: 12670,
			sprites: [
				"brand-a-marker-icon",
				"brand-b-marker-icon",
				"special-event-icon",
				"promotion-icon",
				"new-location-icon",
				"featured-icon",
			],
		},
	];
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

	// Data sources operation
	const dataSourcesOperation = useAsyncOperation<DataSource[]>(
		fetchDataSources,
		{
			showErrorToast: false,
			onError: (error) => {
				console.error("Data sources fetch failed:", error);
			},
		},
	);

	// Styles operation
	const stylesOperation = useAsyncOperation<Style[]>(fetchStyles, {
		showErrorToast: false,
		onError: (error) => {
			console.error("Styles fetch failed:", error);
		},
	});

	// Fonts operation
	const fontsOperation = useAsyncOperation<Font[]>(fetchFonts, {
		showErrorToast: false,
		onError: (error) => {
			console.error("Fonts fetch failed:", error);
		},
	});

	// Sprites operation
	const spritesOperation = useAsyncOperation<SpriteCollection[]>(fetchSprites, {
		showErrorToast: false,
		onError: (error) => {
			console.error("Sprites fetch failed:", error);
		},
	});

	// Load initial data
	useEffect(() => {
		analyticsOperation.execute();
		dataSourcesOperation.execute();
		stylesOperation.execute();
		fontsOperation.execute();
		spritesOperation.execute();
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
						serverMetrics={
							analyticsOperation.data?.serverMetrics || {
								requestsPerSecond: 0,
								memoryUsage: 0,
								cacheHitRate: 0,
								activeSources: 0,
							}
						}
						usageData={analyticsOperation.data?.usageData || []}
						tileSourcesData={analyticsOperation.data?.tileSourcesData || []}
						isLoading={analyticsOperation.isLoading}
						error={analyticsOperation.error}
						onRetry={analyticsOperation.retry}
						isRetrying={analyticsOperation.isRetrying}
					/>

					<Tabs defaultValue="catalog" className="space-y-6">
						<TabsList className="grid w-full grid-cols-4">
							<TabsTrigger value="catalog">Data Catalog</TabsTrigger>
							<TabsTrigger value="styles">Styles Catalog</TabsTrigger>
							<TabsTrigger value="fonts">Font Catalog</TabsTrigger>
							<TabsTrigger value="sprites">Sprite Catalog</TabsTrigger>
						</TabsList>

						<TabsContent value="catalog">
							<DataCatalog
								dataSources={dataSourcesOperation.data || []}
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
								isLoading={dataSourcesOperation.isLoading}
								error={dataSourcesOperation.error}
								onRetry={dataSourcesOperation.retry}
								isRetrying={dataSourcesOperation.isRetrying}
							/>
						</TabsContent>

						<TabsContent value="styles">
							<StylesCatalog
								spriteCollections={spritesOperation.data}
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
								isLoading={stylesOperation.isLoading}
								error={stylesOperation.error}
								onRetry={stylesOperation.retry}
								isRetrying={stylesOperation.isRetrying}
								styles={stylesOperation.data}
							/>
						</TabsContent>

						<TabsContent value="fonts">
							<FontCatalog
								fonts={fontsOperation.data}
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
								isLoading={fontsOperation.isLoading}
								error={fontsOperation.error}
								onRetry={fontsOperation.retry}
								isRetrying={fontsOperation.isRetrying}
							/>
						</TabsContent>

						<TabsContent value="sprites">
							<SpriteCatalog
								spriteCollections={spritesOperation.data}
								searchQuery={searchQuery}
								onSearchChangeAction={setSearchQuery}
								isLoading={spritesOperation.isLoading}
								error={spritesOperation.error}
								onRetry={spritesOperation.retry}
								isRetrying={spritesOperation.isRetrying}
							/>
						</TabsContent>
					</Tabs>
				</div>

				<Toaster />
			</div>
		</ErrorBoundary>
	);
}
