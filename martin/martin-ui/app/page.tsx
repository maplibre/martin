"use client";

import { Suspense, useCallback, useEffect } from "react";
import { AnalyticsSection } from "@/components/analytics-section";
import { DashboardContent } from "@/components/dashboard-content";
import { useAsyncOperation } from "@/hooks/use-async-operation";
import { buildMartinUrl } from "@/lib/api";
import { getMartinMockAnalytics } from "@/lib/mockResponses";
import { aggregateEndpointGroups, ENDPOINT_GROUPS, parsePrometheusMetrics } from "@/lib/prometheus";
import type { AnalyticsData } from "@/lib/types";

const fetchAnalytics = async (): Promise<AnalyticsData> => {
  if (process.env.NEXT_PUBLIC_MARTIN_ENABLE_MOCK_API === "true") {
    return getMartinMockAnalytics();
  }

  const res = await fetch(buildMartinUrl("/_/metrics"), {
    headers: {
      "Accept-Encoding": "identity",
    },
  });
  if (!res.ok) {
    throw new Error(`Failed to fetch analytics: ${res.statusText}`);
  }
  const text = await res.text();
  const { sum, count } = parsePrometheusMetrics(text);
  const groupResults = aggregateEndpointGroups(sum, count, ENDPOINT_GROUPS);
  return {
    fonts: groupResults.fonts,
    sprites: groupResults.sprites,
    styles: groupResults.styles,
    tiles: groupResults.tiles,
  };
};

function DashboardLoading() {
  return (
    <div className="animate-pulse space-y-6">
      {/* Tab navigation skeleton */}
      <div className="grid w-full grid-cols-4 h-10 bg-gray-200 rounded"></div>

      {/* Content skeleton */}
      <div className="space-y-4">
        <div className="h-8 bg-gray-200 rounded w-1/3"></div>
        <div className="h-4 bg-gray-200 rounded w-2/3"></div>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          <div className="h-64 bg-gray-200 rounded"></div>
          <div className="h-64 bg-gray-200 rounded"></div>
          <div className="h-64 bg-gray-200 rounded"></div>
        </div>
      </div>
    </div>
  );
}

export default function MartinTileserverDashboard() {
  const handleAnalyticsError = useCallback((error: Error) => {
    console.error("Analytics fetch failed:", error);
  }, []);

  // Analytics operation
  const analyticsOperation = useAsyncOperation<AnalyticsData>(fetchAnalytics, {
    onError: handleAnalyticsError,
    showErrorToast: false,
  });

  // Load analytics data
  useEffect(() => {
    analyticsOperation.execute();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [analyticsOperation.execute]);

  return (
    <div className="container mx-auto px-6 py-8">
      <AnalyticsSection
        analytics={analyticsOperation.data}
        error={analyticsOperation.error}
        isLoading={analyticsOperation.isLoading}
      />

      <Suspense fallback={<DashboardLoading />}>
        <DashboardContent />
      </Suspense>
    </div>
  );
}
