import { Eye, Layers } from 'lucide-react';
import { useTheme } from 'next-themes';
import { useEffect, useRef, useState } from 'react';
import { CatalogEmpty } from '@/components/catalogs/catalog-empty';
import { CatalogHeader } from '@/components/catalogs/catalog-header';
import { TileInspectDialog } from '@/components/dialogs/tile-inspect';
import { ErrorState } from '@/components/error/error-state';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { CopyLinkButton } from '@/components/ui/copy-link-button';
import { TooltipCopyText } from '@/components/ui/tooltip-copy-text';
import { buildMartinUrl } from '@/lib/api';
import { decodeVectorTile, drawVectorTile } from '@/lib/tile-preview';
import type { Catalog } from '@/lib/types.gen';

const THUMB_PX = 128;

const THUMB_PALETTES = {
  dark: { fill: 'rgba(167,139,250,0.22)', halo: '#14152c', line: '#a78bfa', point: '#ddd6fe' },
  light: { fill: 'rgba(96,52,214,0.16)', halo: '#ffffff', line: '#6034d6', point: '#6034d6' },
};

interface ZoomRange {
  min: number;
  max: number;
  worldwide: boolean;
}

interface ThumbTileJson {
  bounds?: [number, number, number, number];
  center?: [number, number, number];
  minzoom?: number;
  maxzoom?: number;
}

function tileAt(lng: number, lat: number, z: number) {
  const n = 2 ** z;
  const latRad = (Math.max(-85, Math.min(85, lat)) * Math.PI) / 180;
  const x = Math.floor(((lng + 180) / 360) * n);
  const y = Math.floor(((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2) * n);
  return `${z}/${Math.min(Math.max(x, 0), n - 1)}/${Math.min(Math.max(y, 0), n - 1)}`;
}

/** World tile first, then the tile under the source's own center: sparse sources often only have data there. */
async function thumbCandidates(
  name: string,
  signal: AbortSignal,
): Promise<{ candidates: string[]; zooms?: ZoomRange }> {
  const candidates = ['0/0/0'];
  let zooms: ZoomRange | undefined;
  try {
    const response = await fetch(buildMartinUrl(`/${name}`), { signal });
    const tileJson = (await response.json()) as ThumbTileJson;
    const [west, south, east, north] = tileJson.bounds ?? [-180, -85, 180, 85];
    const lng = tileJson.center?.[0] ?? (west + east) / 2;
    const lat = tileJson.center?.[1] ?? (south + north) / 2;
    const minzoom = tileJson.minzoom ?? 0;
    if (tileJson.maxzoom !== undefined) {
      zooms = {
        max: tileJson.maxzoom,
        min: minzoom,
        worldwide: east - west >= 359 && north - south >= 160,
      };
    }
    for (const z of [
      Math.max(minzoom, 2),
      Math.min(tileJson.center?.[2] ?? 4, tileJson.maxzoom ?? 4, 6),
    ]) {
      const tile = tileAt(lng, lat, z);
      if (!candidates.includes(tile)) candidates.push(tile);
    }
  } catch {
    // The world tile alone is a fine fallback
  }
  return { candidates, zooms };
}

/** Some fixtures serve placeholder bytes that are not a valid image; those must not render. */
function canDecode(url: string): Promise<boolean> {
  return new Promise((resolve) => {
    const image = new Image();
    image.onload = () => resolve(image.naturalWidth > 0);
    image.onerror = () => resolve(false);
    image.src = url;
  });
}

export const MAX_CONCURRENT_THUMBS = 4;
const THUMB_DEADLINE_MS = 10_000;
let activeThumbs = 0;
const thumbQueue: (() => void)[] = [];

/** Limits how many thumbnails fetch at once; returns a release function. */
export async function acquireThumbSlot(signal: AbortSignal): Promise<(() => void) | null> {
  if (activeThumbs >= MAX_CONCURRENT_THUMBS) {
    await new Promise<void>((resolve) => {
      const wake = () => {
        signal.removeEventListener('abort', onAbort);
        resolve();
      };
      const onAbort = () => {
        const i = thumbQueue.indexOf(wake);
        if (i >= 0) thumbQueue.splice(i, 1);
        resolve();
      };
      signal.addEventListener('abort', onAbort, { once: true });
      thumbQueue.push(wake);
    });
    if (signal.aborted) return null;
  }
  activeThumbs++;
  return () => {
    activeThumbs--;
    thumbQueue.shift()?.();
  };
}

function hashSeed(text: string) {
  let hash = 2166136261;
  for (const char of text) hash = Math.imul(hash ^ char.charCodeAt(0), 16777619);
  return hash >>> 0;
}

/** Deterministic contour-map tile, so sources without a decodable tile still get a distinct picture. */
function GeneratedTile({ name, label }: { name: string; label: string }) {
  const seed = hashSeed(name);
  const tilt = 10 + (seed % 24);
  const lift = 18 + ((seed >>> 5) % 28);
  return (
    <svg aria-hidden="true" className="absolute inset-0 size-full" viewBox="0 0 64 64">
      <g className="stroke-border" fill="none" strokeWidth="0.75">
        <path d="M16 0V64M32 0V64M48 0V64M0 16H64M0 32H64M0 48H64" />
      </g>
      <path
        className="stroke-primary/35"
        d={`M-2 ${lift}C20 ${lift - tilt} 44 ${lift + tilt} 66 ${lift - 6}`}
        fill="none"
        strokeWidth="1.25"
      />
      <rect className="fill-card stroke-border" height="16" rx="5" width="40" x="12" y="24" />
      <text
        className="fill-foreground"
        fontFamily="var(--font-sans, sans-serif)"
        fontSize="9"
        fontWeight="600"
        textAnchor="middle"
        x="32"
        y="35"
      >
        {label}
      </text>
    </svg>
  );
}

const FORMAT_NAMES: Record<string, string> = {
  'application/json': 'JSON',
  'application/vnd.mapbox-vector-tile': 'MVT',
  'application/vnd.maplibre-tile': 'MLT',
  'application/x-protobuf': 'MVT',
};

/** Short tile format name, such as MVT or PNG, for a content type. */
export function formatName(contentType: string) {
  return FORMAT_NAMES[contentType] ?? (contentType.split('/')[1] ?? 'TILE').toUpperCase();
}

function formatLabel(contentType: string) {
  return formatName(contentType).slice(0, 4);
}

/** Every source shows a real tile (raster image or drawn geometry); a glyph is the fallback. */
function TileThumb({
  name,
  contentType,
  onZooms,
}: {
  name: string;
  contentType: string;
  onZooms?: (zooms: ZoomRange) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const { resolvedTheme } = useTheme();
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [drawn, setDrawn] = useState(false);
  const [visible, setVisible] = useState(false);
  const isRaster = contentType.startsWith('image/');
  const isVector = contentType === 'application/x-protobuf';

  useEffect(() => {
    const el = containerRef.current;
    if (!el || visible) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: '200px' },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [visible]);

  useEffect(() => {
    if (!(visible && (isRaster || isVector))) return;
    const palette = resolvedTheme === 'dark' ? THUMB_PALETTES.dark : THUMB_PALETTES.light;
    const controller = new AbortController();
    let objectUrl: string | null = null;
    let release: (() => void) | null = null;
    let deadline: ReturnType<typeof setTimeout> | undefined;
    (async () => {
      release = await acquireThumbSlot(controller.signal);
      if (!release) return;
      // Started once a slot is held, so queue time does not count; a stalled source must not hold its slot forever
      deadline = setTimeout(() => controller.abort(), THUMB_DEADLINE_MS);
      const { candidates, zooms } = await thumbCandidates(name, controller.signal);
      if (zooms) onZooms?.(zooms);
      for (const tile of candidates) {
        try {
          const response = await fetch(buildMartinUrl(`/${name}/${tile}`), {
            signal: controller.signal,
          });
          if (response.status !== 200) continue;
          if (isRaster) {
            const candidate = URL.createObjectURL(await response.blob());
            if (await canDecode(candidate)) {
              objectUrl = candidate;
              setImageUrl(candidate);
              return;
            }
            URL.revokeObjectURL(candidate);
            continue;
          }
          const ctx = canvasRef.current?.getContext('2d');
          if (!ctx) return;
          ctx.clearRect(0, 0, THUMB_PX, THUMB_PX);
          const layers = decodeVectorTile(await response.arrayBuffer());
          if (drawVectorTile(ctx, layers, THUMB_PX, palette) > 0) {
            setDrawn(true);
            return;
          }
        } catch {
          if (controller.signal.aborted) return;
        }
      }
    })().finally(() => {
      clearTimeout(deadline);
      release?.();
    });
    return () => {
      clearTimeout(deadline);
      controller.abort();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [name, isRaster, isVector, resolvedTheme, visible]);

  return (
    <div
      className="tile-grid relative flex size-16 shrink-0 items-center justify-center overflow-hidden rounded-lg border"
      ref={containerRef}
    >
      {imageUrl ? (
        <img
          alt=""
          className="absolute inset-0 size-full object-cover dark:brightness-[0.85] dark:contrast-110"
          src={imageUrl}
        />
      ) : null}
      {isVector ? (
        <div aria-hidden="true" className="absolute inset-0">
          <canvas
            className={`size-full ${drawn ? '' : 'invisible'}`}
            height={THUMB_PX}
            ref={canvasRef}
            width={THUMB_PX}
          />
        </div>
      ) : null}
      {imageUrl || drawn ? null : <GeneratedTile label={formatLabel(contentType)} name={name} />}
    </div>
  );
}

type TileSourceInfo = NonNullable<Catalog['tiles']>[string];

/** Zoom range, coverage and layer count, each phrased so it needs no label. */
function tileFacts(zooms: ZoomRange | undefined, layerCount: number | null | undefined) {
  const facts: string[] = [];
  if (zooms) {
    facts.push(zooms.min === zooms.max ? `zoom ${zooms.min}` : `zoom ${zooms.min}-${zooms.max}`);
    facts.push(zooms.worldwide ? 'worldwide' : 'regional');
  }
  if (layerCount) facts.push(`${layerCount} ${layerCount === 1 ? 'layer' : 'layers'}`);
  return facts;
}

function TileCard({
  name,
  source,
  repeatOf,
  showFormat,
  onInspect,
}: {
  name: string;
  source: TileSourceInfo;
  repeatOf?: string;
  /** Only worth a badge when the catalog mixes formats. */
  showFormat: boolean;
  onInspect: () => void;
}) {
  const [zooms, setZooms] = useState<ZoomRange>();
  const facts = tileFacts(zooms, source.layer_count);
  // Sources that share a title and description with an earlier card add nothing but noise
  const isRepeat = repeatOf !== undefined;
  const title =
    !isRepeat && source.name && source.name !== name && /\s/.test(source.name.trim())
      ? source.name
      : undefined;
  const description =
    !isRepeat &&
    source.description &&
    source.description !== title &&
    source.description !== name &&
    /\s/.test(source.description.trim())
      ? source.description
      : undefined;
  return (
    <Card className="group flex min-w-0 flex-col transition-shadow hover:shadow-card-hover">
      <CardHeader className="flex-row items-start gap-3 pb-3">
        <TileThumb contentType={source.content_type} name={name} onZooms={setZooms} />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex items-start justify-between gap-3">
            <CardTitle className="min-w-0">
              <TooltipCopyText text={name} />
            </CardTitle>
            {showFormat ? (
              <Badge title={source.content_type} variant="secondary">
                {formatName(source.content_type)}
              </Badge>
            ) : null}
          </div>
          {facts.length > 0 ? (
            <p className="text-xs tabular-nums text-muted-foreground">{facts.join(' · ')}</p>
          ) : null}
        </div>
      </CardHeader>
      {title || description ? (
        <CardContent className="pb-3">
          {title ? (
            <p className="text-sm font-medium leading-snug [overflow-wrap:anywhere]">{title}</p>
          ) : null}
          {description ? (
            <CardDescription
              className="line-clamp-2 break-words"
              title={description.length > 100 ? description : undefined}
            >
              {description}
            </CardDescription>
          ) : null}
        </CardContent>
      ) : null}
      <CardContent className="mt-auto flex gap-2">
        <Button className="min-w-0 flex-1" onClick={onInspect} size="sm" variant="default">
          <Eye />
          Inspect
        </Button>
        <CopyLinkButton
          className="min-w-0 flex-1"
          link={buildMartinUrl(`/${name}`)}
          size="sm"
          toastMessage="TileJSON link copied!"
          variant="outline"
        />
      </CardContent>
    </Card>
  );
}

interface TilesCatalogProps {
  tileSources?: Catalog['tiles'];
  searchQuery: string;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
  selectedTileForInspection?: string | null;
  onInspectTile?: (tileName: string | undefined) => void;
}

export function TilesCatalog({
  tileSources,
  searchQuery,
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
  selectedTileForInspection = null,
  onInspectTile = () => {},
}: TilesCatalogProps) {
  if (isLoading) {
    return (
      <CatalogSkeleton
        description="Explore all available tile sources"
        title="Tiles Sources Catalog"
      />
    );
  }

  if (error) {
    return (
      <ErrorState
        description="Unable to fetch tiles sources from the server"
        error={error}
        isRetrying={isRetrying}
        onRetry={onRetry}
        showDetails={true}
        title="Failed to Load Tiles Catalog"
        variant="server"
      />
    );
  }

  const lowercaseSearchQuery = searchQuery.toLowerCase();
  const filteredTileSources = Object.entries(tileSources || {}).filter(
    ([name, source]) =>
      name.toLowerCase().includes(lowercaseSearchQuery) ||
      source.name?.toLowerCase().includes(lowercaseSearchQuery) ||
      source.description?.toLowerCase().includes(lowercaseSearchQuery) ||
      source.attribution?.toLowerCase().includes(lowercaseSearchQuery) ||
      source.content_encoding?.toLowerCase().includes(lowercaseSearchQuery) ||
      source.content_type?.toLowerCase().includes(lowercaseSearchQuery),
  );

  const mixedFormats =
    new Set(Object.values(tileSources ?? {}).map((source) => source.content_type)).size > 1;

  return (
    <div className="space-y-6">
      <CatalogHeader
        count={filteredTileSources.length}
        noun={['tile source', 'tile sources']}
        searchQuery={searchQuery}
        total={Object.keys(tileSources ?? {}).length}
      />

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 lg:gap-5">
        {filteredTileSources.map(([name, source], index) => (
          <TileCard
            key={name}
            name={name}
            onInspect={() => onInspectTile(name)}
            repeatOf={
              source.name || source.description
                ? filteredTileSources
                    .slice(0, index)
                    .find(
                      ([, other]) =>
                        other.name === source.name && other.description === source.description,
                    )?.[0]
                : undefined
            }
            showFormat={mixedFormats}
            source={source}
          />
        ))}
      </div>

      {filteredTileSources.length === 0 && (
        <CatalogEmpty
          icon={Layers}
          linkHref="https://maplibre.org/martin/sources-tiles.html"
          linkLabel="Learn how to configure Tile Sources"
          message={
            searchQuery
              ? `No tile sources found matching "${searchQuery}".`
              : 'No tile sources found.'
          }
        />
      )}

      {selectedTileForInspection && tileSources ? (
        <TileInspectDialog
          name={selectedTileForInspection}
          onCloseAction={() => onInspectTile(undefined)}
          source={tileSources[selectedTileForInspection]}
        />
      ) : null}
    </div>
  );
}
