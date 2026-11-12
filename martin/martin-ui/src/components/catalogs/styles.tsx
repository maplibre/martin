import { Brush, Code, Layers, SquarePen, TriangleAlert } from 'lucide-react';
import { CatalogEmpty } from '@/components/catalogs/catalog-empty';
import { CatalogHeader } from '@/components/catalogs/catalog-header';
import { StyleIntegrationGuideDialog } from '@/components/dialogs/style-integration-guide';
import { ErrorState } from '@/components/error/error-state';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { TooltipCopyText } from '@/components/ui/tooltip-copy-text';
import { buildMartinUrl } from '@/lib/api';
import { mltAcceptTransformRequest } from '@/lib/mlt';
import type { Catalog } from '@/lib/types.gen';
import 'maplibre-gl/dist/maplibre-gl.css';
import { FullscreenControl, Map as MapLibreMap, type MapRef } from '@vis.gl/react-maplibre';
import { useRef, useState } from 'react';

type PreviewState = 'loading' | 'ready' | 'empty' | 'failed';

function StylePreviewMap({ styleName }: { styleName: string }) {
  const mapRef = useRef<MapRef>(null);
  const [state, setState] = useState<PreviewState>('loading');
  const [reason, setReason] = useState<string>();
  return (
    <div className="relative overflow-hidden rounded-lg border bg-sunken">
      <MapLibreMap
        attributionControl={{ compact: true }}
        mapStyle={buildMartinUrl(`/style/${styleName}`)}
        onError={(e) => {
          // Missing sprites or glyphs still leave a drawable style; only an unreadable style is fatal
          const url = (e.error as { url?: string } | undefined)?.url;
          if (!url?.includes(`/style/${styleName}`)) return;
          setReason(e.error?.message);
          setState((prev) => (prev === 'loading' ? 'failed' : prev));
        }}
        onLoad={(e) => {
          // Keep attribution collapsed to the (i) button in thumbnails
          e.target
            .getContainer()
            .querySelector('.maplibregl-ctrl-attrib')
            ?.classList.remove('maplibregl-compact-show');
          e.target
            .getContainer()
            .querySelector('details.maplibregl-ctrl-attrib')
            ?.removeAttribute('open');
          const layers = e.target.getStyle().layers ?? [];
          setState(layers.every((l) => l.type === 'background') ? 'empty' : 'ready');
        }}
        ref={mapRef}
        reuseMaps
        style={{ aspectRatio: 16 / 9, width: '100%' }}
        transformRequest={mltAcceptTransformRequest(() => mapRef.current?.getMap())}
      >
        <FullscreenControl />
      </MapLibreMap>
      {state === 'loading' ? (
        <div className="tile-grid pointer-events-none absolute inset-0 flex animate-pulse items-center justify-center text-sm text-muted-foreground">
          Loading preview…
        </div>
      ) : null}
      {state === 'empty' ? (
        <div className="tile-grid absolute inset-0 flex flex-col items-center justify-center gap-1 p-4 text-center">
          <Layers aria-hidden="true" className="size-5 text-muted-foreground" />
          <p className="text-sm font-medium">Nothing to draw</p>
          <p className="text-xs text-muted-foreground">
            This style only defines a background, so there are no map layers to preview.
          </p>
        </div>
      ) : null}
      {state === 'failed' ? (
        <div className="tile-grid absolute inset-0 flex flex-col items-center justify-center gap-1 overflow-y-auto p-4 text-center">
          <TriangleAlert aria-hidden="true" className="size-5 text-destructive" />
          <p className="text-sm font-medium">Preview unavailable</p>
          <p className="text-xs text-muted-foreground">This style could not be loaded.</p>
          {reason ? (
            <details className="max-w-full text-xs text-muted-foreground">
              <summary className="cursor-pointer underline underline-offset-2">
                Technical details
              </summary>
              <p className="mt-1 font-mono [overflow-wrap:anywhere]">{reason}</p>
            </details>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

interface StylesCatalogProps {
  styles?: Catalog['styles'];
  searchQuery?: string;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
  onEditStyle?: (styleName: string) => void;
  selectedStyleForGuide?: string | undefined;
  onStyleGuide?: (styleName: string | undefined) => void;
}

export function StylesCatalog({
  styles,
  searchQuery = '',
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
  onEditStyle,
  selectedStyleForGuide = undefined,
  onStyleGuide = () => {},
}: StylesCatalogProps) {
  if (isLoading) {
    return (
      <CatalogSkeleton
        description="Preview all available map styles and themes"
        title="Styles Catalog"
      />
    );
  }

  if (error) {
    return (
      <ErrorState
        description="Unable to fetch style catalog from the server"
        error={error}
        isRetrying={isRetrying}
        onRetry={onRetry}
        showDetails={true}
        title="Failed to Load Styles"
        variant="server"
      />
    );
  }

  const filteredStyles = Object.entries(styles || {}).filter(
    ([name, style]) =>
      name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      style.path.toLowerCase().includes(searchQuery.toLowerCase()) ||
      style.type?.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="space-y-6">
      <CatalogHeader
        count={filteredStyles.length}
        noun={['style', 'styles']}
        searchQuery={searchQuery}
        total={Object.keys(styles ?? {}).length}
      />

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 lg:gap-5">
        {filteredStyles.map(([name, style]) => (
          <Card
            className="flex min-w-0 flex-col transition-shadow hover:shadow-card-hover"
            key={name}
          >
            <CardHeader className="pb-3">
              <div className="flex items-start justify-between gap-3">
                <CardTitle className="min-w-0">
                  <TooltipCopyText text={name} />
                </CardTitle>
                {style.type ? <Badge variant="secondary">{style.type}</Badge> : null}
              </div>
              <CardDescription
                className="line-clamp-2 font-mono text-xs [overflow-wrap:anywhere]"
                title={style.path}
              >
                {style.path}
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-1 flex-col gap-4">
              <StylePreviewMap styleName={name} />
              <dl className="space-y-1.5 text-sm">
                {style.version_hash ? (
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted-foreground">Version</dt>
                    <dd className="truncate font-mono text-xs">{style.version_hash}</dd>
                  </div>
                ) : null}
                {style.layer_count ? (
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted-foreground">Layers</dt>
                    <dd className="tabular-nums">{style.layer_count}</dd>
                  </div>
                ) : null}
                {style.last_modified_at ? (
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted-foreground">Modified</dt>
                    <dd>{style.last_modified_at?.toLocaleString()}</dd>
                  </div>
                ) : null}
              </dl>
              {style.colors ? (
                <ul aria-label="Color palette" className="flex flex-wrap gap-1">
                  {style.colors.map((color) => (
                    <li
                      className="size-6 rounded-sm border"
                      key={color}
                      style={{ backgroundColor: color }}
                      title={color}
                    />
                  ))}
                </ul>
              ) : null}
              <div className="mt-auto flex gap-2">
                <Button
                  className="min-w-0 flex-1"
                  onClick={() => onEditStyle?.(name)}
                  size="sm"
                  variant="default"
                >
                  <SquarePen />
                  Edit
                </Button>
                <Button
                  className="min-w-0 flex-1"
                  onClick={() => onStyleGuide(name)}
                  size="sm"
                  variant="outline"
                >
                  <Code />
                  Integration Guide
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {filteredStyles.length === 0 && (
        <CatalogEmpty
          icon={Brush}
          linkHref="https://maplibre.org/martin/sources-styles.html"
          linkLabel="Learn how to configure Styles"
          message={searchQuery ? `No styles found matching "${searchQuery}"` : 'No styles found.'}
        />
      )}

      {selectedStyleForGuide && styles ? (
        <StyleIntegrationGuideDialog
          name={selectedStyleForGuide}
          onCloseAction={() => onStyleGuide(undefined)}
          style={styles[selectedStyleForGuide]}
        />
      ) : null}
    </div>
  );
}
