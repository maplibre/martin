import { Image, Info, Layers, Palette, Type } from 'lucide-react';
import { MiniHistogram } from '@/components/charts/mini-histogram';
import { ErrorState } from '@/components/error/error-state';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { Skeleton } from '@/components/ui/skeleton';
import { type CacheMetrics, type HitCount, hitRate, type ZoomHitCount } from '@/lib/prometheus';
import type { AnalyticsData } from '@/lib/types';

interface AnalyticsSectionProps {
  analytics?: AnalyticsData;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

interface CardConfig {
  readonly icon: typeof Layers;
  readonly key: keyof Omit<AnalyticsData, 'caches'>;
  readonly title: string;
  /** Cache types backing this endpoint, with display labels. */
  readonly caches: readonly { readonly key: string; readonly label: string }[];
}

const CARD_CONFIG: readonly CardConfig[] = [
  {
    caches: [
      { key: 'tile', label: 'Tile cache' },
      { key: 'pmtiles_directory', label: 'PMTiles dirs' },
    ],
    icon: Layers,
    key: 'tiles',
    title: 'Tiles',
  },
  {
    caches: [],
    icon: Palette,
    key: 'styles',
    title: 'Styles',
  },
  {
    caches: [{ key: 'font', label: 'Font cache' }],
    icon: Type,
    key: 'fonts',
    title: 'Fonts',
  },
  {
    caches: [{ key: 'sprite', label: 'Sprite cache' }],
    icon: Image,
    key: 'sprites',
    title: 'Sprites',
  },
];

function formatHitRate(counts: HitCount): string {
  const rate = hitRate(counts);
  if (rate === null) return 'no requests yet';
  return `${(rate * 100).toFixed(1)}% hit`;
}

function ZoomBreakdownPopover({ label, metrics }: { label: string; metrics: CacheMetrics }) {
  return (
    <HoverCard closeDelay={100} openDelay={150}>
      <HoverCardTrigger asChild>
        <button
          aria-label={`${label} hit rate by zoom`}
          className="text-muted-foreground hover:text-foreground focus-visible:text-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring/50 rounded-sm"
          type="button"
        >
          <Info className="h-3 w-3" />
        </button>
      </HoverCardTrigger>
      <HoverCardContent align="end" className="w-56 p-3">
        <div className="text-xs font-medium mb-2">{label} by zoom</div>
        <table className="w-full text-xs tabular-nums">
          <thead>
            <tr className="text-muted-foreground">
              <th className="text-left font-normal">Zoom</th>
              <th className="text-right font-normal">Hit rate</th>
              <th className="text-right font-normal">Hits / Total</th>
            </tr>
          </thead>
          <tbody>
            {metrics.byZoom.map((row: ZoomHitCount) => (
              <tr key={row.zoom}>
                <td className="text-left">{row.zoom}</td>
                <td className="text-right font-medium">{formatHitRate(row)}</td>
                <td className="text-right text-muted-foreground">
                  {row.hits.toLocaleString()} / {(row.hits + row.misses).toLocaleString()}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </HoverCardContent>
    </HoverCard>
  );
}

export function AnalyticsSection({
  analytics,
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
}: AnalyticsSectionProps) {
  if (error) {
    return (
      <div className="mb-8">
        <ErrorState
          description="Unable to fetch server metrics and usage data"
          error={error}
          isRetrying={isRetrying}
          onRetry={onRetry}
          showDetails={true}
          title="Failed to Load Analytics"
          variant="server"
        />
      </div>
    );
  }
  return (
    <div className="space-y-6 mb-8">
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
        {CARD_CONFIG.map(({ key, title, icon: Icon, caches }) => {
          const data = analytics?.[key];
          const cacheEntries = caches
            .map((c) => ({ ...c, metrics: analytics?.caches?.[c.key] }))
            .filter((c) => c.metrics);
          return (
            <Card key={key}>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">{title}</CardTitle>
                <Icon className="h-4 w-4 text-primary" />
              </CardHeader>
              <CardContent>
                <div className="flex justify-between items-start">
                  <div className="flex-1">
                    <div className="text-2xl font-bold flex flex-row gap-1">
                      {isLoading || !data ? (
                        <Skeleton className="h-6 w-12 flex flex-row" />
                      ) : (
                        <>
                          {Math.round(data.averageRequestDurationMs)}
                          {' ms'}
                        </>
                      )}
                    </div>
                    <span className="text-xs text-muted-foreground flex flex-row gap-1">
                      {isLoading || !data ? (
                        <Skeleton className="h-3 w-20" />
                      ) : (
                        <>
                          {data.requestCount.toLocaleString()}
                          {' requests'}
                        </>
                      )}
                    </span>
                  </div>
                  <div className="flex items-center">
                    {data?.histogram ? (
                      <MiniHistogram histogram={data.histogram} />
                    ) : (
                      <div className="w-20 h-12 bg-muted/10 rounded-md opacity-40 animate-pulse bg-linear-to-r from-transparent to-muted"></div>
                    )}
                  </div>
                </div>
                {cacheEntries.length > 0 && (
                  <dl className="mt-3 pt-3 border-t border-border space-y-1">
                    {cacheEntries.map(({ key: cacheKey, label, metrics }) => (
                      <div className="flex justify-between items-center text-xs" key={cacheKey}>
                        <dt className="text-muted-foreground">{label}</dt>
                        <dd
                          className="flex items-center gap-1.5 font-medium tabular-nums"
                          title={
                            metrics
                              ? `${metrics.hits.toLocaleString()} hits, ${metrics.misses.toLocaleString()} misses`
                              : undefined
                          }
                        >
                          {metrics ? formatHitRate(metrics) : '—'}
                          {metrics && metrics.byZoom.length > 0 && (
                            <ZoomBreakdownPopover label={label} metrics={metrics} />
                          )}
                        </dd>
                      </div>
                    ))}
                  </dl>
                )}
              </CardContent>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
