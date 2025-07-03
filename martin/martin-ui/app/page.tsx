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
  // below API is a prometheus metrics endpoint and does not return json
  await new Promise<void>((resolve) => setTimeout(resolve, 60 * 60 * 1000));
	// the metrics api does not currently support gzip compression
  const res = await fetch("/metrics", {
		headers: {
			"Accept-Encoding": "identity",
		},
	});
	if (!res.ok) {
		throw new Error(`Failed to fetch analytics: ${res.statusText}`);
	}

	return {
		requestsPerSecond: 1247,
		memoryUsage: 68,
		cacheHitRate: 94.2,
		activeSources: 23,
	};
};

const fetchCatalog = async (): Promise<CatalogSchema> => {
	const res = await fetch("/catalog");
	if (!res.ok) {
		throw new Error(`Failed to fetch catalog: ${res.statusText}`);
	}
	return res.json();
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

					<Tabs defaultValue="tiles" className="space-y-6">
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
