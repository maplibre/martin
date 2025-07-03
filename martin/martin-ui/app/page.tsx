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
import type { AnalyticsData, CatalogSchema } from "@/lib/types";

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
    activeSources: 23,
    cacheHitRate: 94.2,
    memoryUsage: 68,
    requestsPerSecond: 1247,
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
    onError: (error) => {
      console.error("Analytics fetch failed:", error);
    }, // We handle errors in the component
    showErrorToast: false,
  });

  // Catalog operation - unified data fetching
  const catalogOperation = useAsyncOperation(fetchCatalog, {
    onError: (error) => {
      console.error("Catalog fetch failed:", error);
    },
    showErrorToast: false,
  });

  // Load initial data
  useEffect(() => {
    analyticsOperation.execute();
    catalogOperation.execute();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [analyticsOperation.execute, catalogOperation.execute]);

  return (
    <ErrorBoundary
      onError={(error: Error, errorInfo: ErrorInfo) => {
        console.error("Application error:", error, errorInfo);
        toast({
          description: "An unexpected error occurred. The page will reload automatically.",
          title: "Application Error",
          variant: "destructive",
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
            error={analyticsOperation.error}
            isLoading={analyticsOperation.isLoading}
            isRetrying={analyticsOperation.isRetrying}
            onRetry={analyticsOperation.retry}
          />

          <Tabs className="space-y-6" defaultValue="tiles">
            <TabsList className="grid w-full grid-cols-4">
              <TabsTrigger value="tiles">Data Catalog</TabsTrigger>
              <TabsTrigger value="styles">Styles Catalog</TabsTrigger>
              <TabsTrigger value="fonts">Font Catalog</TabsTrigger>
              <TabsTrigger value="sprites">Sprite Catalog</TabsTrigger>
            </TabsList>

            <TabsContent value="tiles">
              <TilesCatalog
                error={catalogOperation.error}
                isLoading={catalogOperation.isLoading}
                isRetrying={catalogOperation.isRetrying}
                onRetry={catalogOperation.retry}
                onSearchChangeAction={setSearchQuery}
                searchQuery={searchQuery}
                tileSources={catalogOperation.data?.tiles}
              />
            </TabsContent>

            <TabsContent value="styles">
              <StylesCatalog
                error={catalogOperation.error}
                isLoading={catalogOperation.isLoading}
                isRetrying={catalogOperation.isRetrying}
                onRetry={catalogOperation.retry}
                onSearchChangeAction={setSearchQuery}
                searchQuery={searchQuery}
                styles={catalogOperation.data?.styles}
              />
            </TabsContent>

            <TabsContent value="fonts">
              <FontCatalog
                error={catalogOperation.error}
                fonts={catalogOperation.data?.fonts}
                isLoading={catalogOperation.isLoading}
                isRetrying={catalogOperation.isRetrying}
                onRetry={catalogOperation.retry}
                onSearchChangeAction={setSearchQuery}
                searchQuery={searchQuery}
              />
            </TabsContent>

            <TabsContent value="sprites">
              <SpriteCatalog
                error={catalogOperation.error}
                isLoading={catalogOperation.isLoading}
                isRetrying={catalogOperation.isRetrying}
                onRetry={catalogOperation.retry}
                onSearchChangeAction={setSearchQuery}
                searchQuery={searchQuery}
                spriteCollections={catalogOperation.data?.sprites}
              />
            </TabsContent>
          </Tabs>
        </div>

        <Toaster />
      </div>
    </ErrorBoundary>
  );
}
