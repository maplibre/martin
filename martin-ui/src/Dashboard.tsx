import { Suspense, useEffect, useState } from 'react';
import type { CatalogCounts } from '@/components/analytics-section';
import { AnalyticsSection } from '@/components/analytics-section';
import { DashboardContent } from '@/components/dashboard-content';
import { useAsyncOperation } from '@/hooks/use-async-operation';
import { buildMartinUrl } from '@/lib/api';
import type { AnalyticsData } from '@/lib/prometheus';
import {
  aggregateEndpointGroups,
  aggregateHistogramGroups,
  ENDPOINT_GROUPS,
  parseCacheMetrics,
  parseCompletePrometheusMetrics,
} from '@/lib/prometheus';

const fetchAnalytics = async (): Promise<AnalyticsData> => {
  const res = await fetch(buildMartinUrl('/_/metrics'));
  if (!res.ok) {
    throw new Error(`Failed to fetch analytics: ${res.statusText}`);
  }
  const text = await res.text();
  const { sum, count, histograms } = parseCompletePrometheusMetrics(text);

  // We want to merge some URLs, such as the many sprite URLs
  const groupResults = aggregateEndpointGroups(sum, count, ENDPOINT_GROUPS);
  const groupHistograms = aggregateHistogramGroups(histograms, ENDPOINT_GROUPS);

  return {
    caches: parseCacheMetrics(text),
    fonts: { ...groupResults.fonts, histogram: groupHistograms.fonts },
    sprites: { ...groupResults.sprites, histogram: groupHistograms.sprites },
    styles: { ...groupResults.styles, histogram: groupHistograms.styles },
    tiles: { ...groupResults.tiles, histogram: groupHistograms.tiles },
  };
};

function DashboardLoading() {
  return (
    <div className="animate-pulse space-y-6">
      {/* Tab navigation skeleton */}
      <div className="grid w-full grid-cols-4 h-10 bg-muted rounded-sm"></div>

      {/* Content skeleton */}
      <div className="space-y-4">
        <div className="h-8 bg-muted rounded-sm w-1/3"></div>
        <div className="h-4 bg-muted rounded-sm w-2/3"></div>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          <div className="h-64 bg-muted rounded-sm"></div>
          <div className="h-64 bg-muted rounded-sm"></div>
          <div className="h-64 bg-muted rounded-sm"></div>
        </div>
      </div>
    </div>
  );
}

async function fetchCounts(): Promise<CatalogCounts> {
  const res = await fetch(buildMartinUrl('/catalog'));
  if (!res.ok) throw new Error(`Failed to fetch catalog: ${res.statusText}`);
  const catalog = await res.json();
  const size = (group: unknown) =>
    group && typeof group === 'object' ? Object.keys(group).length : 0;
  return {
    fonts: size(catalog.fonts),
    sprites: size(catalog.sprites),
    styles: size(catalog.styles),
    tiles: size(catalog.tiles),
  };
}

export default function MartinTileserverDashboard() {
  const [counts, setCounts] = useState<CatalogCounts>();
  const [countsFailed, setCountsFailed] = useState(false);

  // Analytics operation
  const analyticsOperation = useAsyncOperation<AnalyticsData>(fetchAnalytics, {
    onError: (error: Error) => console.error('Analytics fetch failed:', error),
    showErrorToast: false,
  });

  // Load analytics data and set up auto-refresh
  useEffect(() => {
    // Initial load
    const refresh = () => {
      analyticsOperation.execute();
      // Retried with every refresh, so one failed request does not leave the summary loading forever
      fetchCounts().then(
        (next) => {
          setCounts(next);
          setCountsFailed(false);
        },
        (error) => {
          console.error('Catalog count failed:', error);
          setCountsFailed(true);
        },
      );
    };
    refresh();

    // 15-second refresh interval
    const interval = setInterval(refresh, 15 * 1000);

    return () => clearInterval(interval);
  }, []);

  return (
    <div className="container mx-auto px-6 py-8">
      <AnalyticsSection
        analytics={analyticsOperation.data}
        counts={counts}
        countsUnavailable={countsFailed && !counts}
        error={analyticsOperation.error}
        isLoading={analyticsOperation.isLoading}
      />

      <Suspense fallback={<DashboardLoading />}>
        <DashboardContent />
      </Suspense>
    </div>
  );
}
