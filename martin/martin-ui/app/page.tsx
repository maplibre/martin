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
import type { AnalyticsData, DataSource, Sprite } from "@/lib/types";

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

	if (Math.random() < 0.15) {
		throw new Error("Network error: Unable to connect to data source API");
	}

	return [
		{
			id: "osm-bright",
			name: "OSM Bright",
			type: "Vector Tiles",
			description: "OpenStreetMap data with bright styling",
			layers: 12,
			lastUpdated: "2 hours ago",
			size: "2.3 GB",
		},
		{
			id: "satellite",
			name: "Satellite Imagery",
			type: "Raster Tiles",
			description: "High-resolution satellite imagery",
			layers: 1,
			lastUpdated: "1 day ago",
			size: "15.7 GB",
		},
		{
			id: "terrain",
			name: "Terrain Contours",
			type: "Vector Tiles",
			description: "Elevation contours and terrain features",
			layers: 8,
			lastUpdated: "6 hours ago",
			size: "1.8 GB",
		},
		{
			id: "pois",
			name: "POIs",
			type: "Vector Tile",
			description: "Point of interest icons and markers",
			layers: 1,
			lastUpdated: "3 days ago",
			size: "45 MB",
		},
	];
};

const fetchStyles = async (): Promise<boolean> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 800));

	if (Math.random() < 0.1) {
		throw new Error("Server timeout: Style service is temporarily unavailable");
	}

	return true;
};

const fetchFonts = async (): Promise<boolean> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 900));

	if (Math.random() < 0.1) {
		throw new Error("Font service error: Unable to load font catalog");
	}

	return true;
};

const fetchSprites = async (): Promise<boolean> => {
	await new Promise<void>((resolve) => setTimeout(resolve, 1100));

	if (Math.random() < 0.1) {
		throw new Error("Sprite service error: Failed to fetch sprite collections");
	}

	return true;
};

export default function MartinTileserverDashboard() {
	const [searchQuery, setSearchQuery] = useState("");
	const [selectedSprite, setSelectedSprite] = useState<Sprite | null>(null);
	const [downloadSprite, setDownloadSprite] = useState<Sprite | null>(null);
	const [isSearching, setIsSearching] = useState(false);
	const [searchError, setSearchError] = useState<Error | null>(null);

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
	const stylesOperation = useAsyncOperation<boolean>(fetchStyles, {
		showErrorToast: false,
		onError: (error) => {
			console.error("Styles fetch failed:", error);
		},
	});

	// Fonts operation
	const fontsOperation = useAsyncOperation<boolean>(fetchFonts, {
		showErrorToast: false,
		onError: (error) => {
			console.error("Fonts fetch failed:", error);
		},
	});

	// Sprites operation
	const spritesOperation = useAsyncOperation<boolean>(fetchSprites, {
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

	// Handle search with error simulation
	useEffect(() => {
		if (searchQuery) {
			setIsSearching(true);
			setSearchError(null);

			const searchTimer = setTimeout(() => setIsSearching(false), 500);

			return () => clearTimeout(searchTimer);
		} else {
			setIsSearching(false);
			setSearchError(null);
		}
	}, [searchQuery]);

	// Handle sprite selection
	const handleSpriteSelect = (sprite: Sprite) => {
		setSelectedSprite(sprite);
	};

	const handleSpriteClose = () => {
		setSelectedSprite(null);
	};

	const handleRetrySearch = () => {
		setSearchError(null);
		setIsSearching(true);
		setTimeout(() => {
			setIsSearching(false);
		}, 500);
	};

	// Removed unused handleDownloadOpen

	const handleDownloadClose = () => {
		setDownloadSprite(null);
	};

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
			<div className="min-h-screen bg-gradient-to-br from-purple-50 to-white">
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
								isSearching={isSearching}
								error={dataSourcesOperation.error}
								searchError={searchError}
								onRetry={dataSourcesOperation.retry}
								onRetrySearch={handleRetrySearch}
								isRetrying={dataSourcesOperation.isRetrying}
							/>
						</TabsContent>

						<TabsContent value="styles">
							<StylesCatalog
								isLoading={stylesOperation.isLoading}
								error={stylesOperation.error}
								onRetry={stylesOperation.retry}
								isRetrying={stylesOperation.isRetrying}
							/>
						</TabsContent>

						<TabsContent value="fonts">
							<FontCatalog
								isLoading={fontsOperation.isLoading}
								error={fontsOperation.error}
								onRetry={fontsOperation.retry}
								isRetrying={fontsOperation.isRetrying}
							/>
						</TabsContent>

						<TabsContent value="sprites">
							<SpriteCatalog
								selectedSprite={selectedSprite}
								onSpriteSelectAction={handleSpriteSelect}
								onSpriteCloseAction={handleSpriteClose}
								downloadSprite={downloadSprite}
								onDownloadCloseAction={handleDownloadClose}
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
