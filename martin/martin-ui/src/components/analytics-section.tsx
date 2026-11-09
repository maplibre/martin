import { ChevronDown, Image, Info, Layers, Palette, Type } from 'lucide-react';
import { useState } from 'react';
import { MiniHistogram } from '@/components/charts/mini-histogram';
import { ErrorState } from '@/components/error/error-state';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { Skeleton } from '@/components/ui/skeleton';
import type { AnalyticsData } from '@/lib/prometheus';
import { type CacheMetrics, type HitCount, hitRate, type ZoomHitCount } from '@/lib/prometheus';

export interface CatalogCounts {
  tiles: number;
  styles: number;
  fonts: number;
  sprites: number;
}

interface AnalyticsSectionProps {
  analytics?: AnalyticsData;
  counts?: CatalogCounts;
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
          className="inline-flex size-6 items-center justify-center rounded-sm text-muted-foreground hover:text-foreground"
          type="button"
        >
          <Info className="size-3.5" />
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
  counts,
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
}: AnalyticsSectionProps) {
  const [expanded, setExpanded] = useState(false);
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
    <section aria-label="Server activity" className="mb-8">
      <div className="mb-3 flex items-end justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            Server activity
            <span className="ml-2 hidden font-normal normal-case tracking-normal sm:inline">
              avg. response time since start
            </span>
          </h2>
          {counts ? (
            <p className="mt-1 text-base font-medium sm:text-lg">
              Serving <strong className="text-brand">{counts.tiles}</strong> tile{' '}
              {counts.tiles === 1 ? 'source' : 'sources'},{' '}
              <strong className="text-brand">{counts.styles}</strong>{' '}
              {counts.styles === 1 ? 'style' : 'styles'},{' '}
              <strong className="text-brand">{counts.fonts}</strong>{' '}
              {counts.fonts === 1 ? 'font' : 'fonts'} and{' '}
              <strong className="text-brand">{counts.sprites}</strong>{' '}
              {counts.sprites === 1 ? 'sprite sheet' : 'sprite sheets'}
            </p>
          ) : (
            <Skeleton className="mt-1 h-6 w-72 max-w-full" />
          )}
        </div>
        <Button
          aria-expanded={expanded}
          className="md:hidden"
          onClick={() => setExpanded((v) => !v)}
          size="sm"
          variant="ghost"
        >
          {expanded ? 'Less' : 'Details'}
          <ChevronDown className={expanded ? 'rotate-180' : ''} />
        </Button>
      </div>
      <Card className="grid grid-cols-2 gap-px overflow-hidden bg-border lg:grid-cols-4">
        {CARD_CONFIG.map(({ key, title, icon: Icon, caches }) => {
          const data = analytics?.[key];
          const cacheEntries = caches
            .map((c) => ({ ...c, metrics: analytics?.caches?.[c.key] }))
            .filter((c) => c.metrics);
          const pending = isLoading || !data;
          const idle = !pending && data.requestCount === 0;
          return (
            <div className="min-w-0 bg-card p-4" key={key}>
              <div className="flex items-center justify-between gap-2">
                <h3 className="flex items-center gap-1.5 text-sm font-medium text-muted-foreground">
                  <Icon aria-hidden="true" className="size-4 text-brand" />
                  {title}
                </h3>
              </div>
              <div className="mt-2 flex items-end justify-between gap-3">
                <div className="min-w-0">
                  <div className="mt-2 whitespace-nowrap text-xl font-semibold leading-none tabular-nums">
                    {pending ? (
                      <Skeleton className="h-6 w-14" />
                    ) : idle ? (
                      <span aria-label="No data" className="text-muted-foreground" role="img">
                        -
                      </span>
                    ) : (
                      <>
                        {Math.round(data.averageRequestDurationMs)}
                        {' ms'}
                        <span className="ml-1.5 text-xs font-normal text-muted-foreground">
                          avg
                        </span>
                      </>
                    )}
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground tabular-nums">
                    {pending ? (
                      <Skeleton className="h-3 w-20" />
                    ) : idle ? (
                      'No requests yet'
                    ) : (
                      <>
                        {data.requestCount.toLocaleString()}
                        {' requests'}
                      </>
                    )}
                  </div>
                </div>
                {pending || !data?.histogram ? (
                  <div
                    aria-hidden="true"
                    className="h-10 w-10 shrink-0 animate-pulse sm:w-16 rounded-md bg-linear-to-r from-transparent to-muted opacity-40"
                  />
                ) : (
                  <MiniHistogram
                    className="h-10 w-10 shrink-0 sm:w-16"
                    histogram={data.histogram}
                  />
                )}
              </div>
              <dl
                className={`mt-3 min-h-9 space-y-1 border-t border-border pt-2 ${expanded ? '' : 'max-md:hidden'}`}
              >
                {cacheEntries.length === 0 ? (
                  <div className="flex items-center justify-between gap-2 text-[0.8125rem]">
                    <dt className="text-muted-foreground">
                      {caches.length === 0 ? 'Style cache' : 'Cache'}
                    </dt>
                    <dd className="text-muted-foreground">
                      {caches.length === 0 ? 'not cached' : '-'}
                    </dd>
                  </div>
                ) : null}
                {cacheEntries.map(({ key: cacheKey, label, metrics }) => (
                  <div
                    className="flex items-center justify-between gap-2 text-[0.8125rem]"
                    key={cacheKey}
                  >
                    <dt className="truncate text-muted-foreground">{label}</dt>
                    <dd
                      className="flex shrink-0 items-center gap-1 font-medium tabular-nums"
                      title={
                        metrics
                          ? `${metrics.hits.toLocaleString()} hits, ${metrics.misses.toLocaleString()} misses`
                          : undefined
                      }
                    >
                      {metrics ? formatHitRate(metrics) : '-'}
                      {metrics && metrics.byZoom.length > 0 && (
                        <ZoomBreakdownPopover label={label} metrics={metrics} />
                      )}
                    </dd>
                  </div>
                ))}
              </dl>
            </div>
          );
        })}
      </Card>
    </section>
  );
}
