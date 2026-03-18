import { useCallback, useEffect, useRef, useState } from 'react';
import { aggregateTileMetrics, parsePrometheusMetrics } from '@/lib/prometheus';

interface MetricsPanelProps {
  martinBaseUrl: string;
  refreshIntervalMs?: number;
  /** When true, do not render the section title (e.g. when used inside an "Observability" card). */
  hideTitle?: boolean;
}

interface MetricsData {
  requestCount: number;
  averageDurationMs: number;
}

export default function MetricsPanel({
  martinBaseUrl,
  refreshIntervalMs = 5000,
  hideTitle = false,
}: MetricsPanelProps) {
  const [metrics, setMetrics] = useState<MetricsData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fetchMetricsRef = useRef<(() => Promise<void>) | null>(null);

  const fetchMetrics = useCallback(async () => {
    const url = `${martinBaseUrl.replace(/\/$/, '')}/_/metrics`;
    try {
      const res = await fetch(url);
      if (!res.ok) {
        setError(`HTTP ${res.status}`);
        return;
      }
      const text = await res.text();
      const { sum, count } = parsePrometheusMetrics(text);
      const tile = aggregateTileMetrics(sum, count);
      setMetrics({
        averageDurationMs: tile.averageDurationMs,
        requestCount: tile.requestCount,
      });
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to fetch');
      // Do not clear metrics so we can show "Connection lost" with stale data
    }
  }, [martinBaseUrl]);

  useEffect(() => {
    fetchMetricsRef.current = fetchMetrics;
  }, [fetchMetrics]);

  useEffect(() => {
    fetchMetrics();
    const id = setInterval(() => fetchMetricsRef.current?.(), refreshIntervalMs);
    return () => clearInterval(id);
  }, [fetchMetrics, refreshIntervalMs]);

  const handleRetry = useCallback(() => {
    setError(null);
    void fetchMetrics();
  }, [fetchMetrics]);

  if (error && !metrics) {
    return (
      <div className="rounded-lg border border-border bg-background/90 p-3">
        {!hideTitle && (
          <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider block mb-1">
            Metrics
          </span>
        )}
        <p className="text-[11px] text-muted-foreground">Unable to fetch: {error}</p>
        <button
          className="mt-2 text-[11px] font-mono text-accent hover:text-foreground underline"
          onClick={handleRetry}
          type="button"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-border bg-background/90 p-3">
      {!hideTitle && (
        <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider block mb-2">
          Live metrics
        </span>
      )}
      {error && metrics && (
        <p className="text-[11px] text-amber-600 dark:text-amber-500 mb-1">
          Connection lost. Showing last known data.
        </p>
      )}
      <div className={hideTitle && !error ? 'flex gap-4' : 'mt-2 flex gap-4'}>
        <div>
          <span className="block text-lg font-mono font-bold text-accent">
            {metrics?.requestCount ?? '–'}
          </span>
          <span className="text-[10px] text-muted-foreground">Tile requests</span>
        </div>
        <div>
          <span className="block text-lg font-mono font-bold text-accent">
            {metrics ? `${metrics.averageDurationMs.toFixed(1)} ms` : '–'}
          </span>
          <span className="text-[10px] text-muted-foreground">Avg duration</span>
        </div>
      </div>
    </div>
  );
}
