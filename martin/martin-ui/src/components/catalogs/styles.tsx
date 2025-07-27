import { Brush, Code, Search, SquarePen } from 'lucide-react';
import { StyleIntegrationGuideDialog } from '@/components/dialogs/style-integration-guide';
import { ErrorState } from '@/components/error/error-state';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { buildMartinUrl } from '@/lib/api';
import type { Style } from '@/lib/types';
import 'maplibre-gl/dist/maplibre-gl.css';
import { FullscreenControl, Map as MapLibreMap } from '@vis.gl/react-maplibre';
import { useState } from 'react';

interface StylesCatalogProps {
  styles?: { [name: string]: Style };
  searchQuery?: string;
  onSearchChangeAction?: (query: string) => void;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
  onEditStyle?: (styleName: string) => void;
}

export function StylesCatalog({
  styles,
  searchQuery = '',
  onSearchChangeAction = () => {},
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
  onEditStyle,
}: StylesCatalogProps) {
  const [viewState, setViewState] = useState({
    latitude: 53,
    longitude: 9,
    zoom: 2,
  });
  const [selectedStyleForGuide, setSelectedStyleForGuide] = useState<{
    name: string;
    style: Style;
  } | null>(null);
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
      <div className="flex flex-col md:flex-row md:items-center items-start justify-between gap-5">
        <div>
          <h2 className="text-2xl font-bold text-foreground">Styles Catalog</h2>
          <p className="text-muted-foreground">
            Browse and preview all available map styles and themes
          </p>
        </div>
        <div className="relative w-full md:w-64">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
          <Input
            className="pl-10 md:w-64 w-full bg-card"
            onChange={(e) => onSearchChangeAction(e.target.value)}
            placeholder="Search styles..."
            value={searchQuery}
          />
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        {filteredStyles.map(([name, style]) => (
          <Card className="hover:shadow-lg transition-shadow" key={name}>
            <CardHeader>
              <div className="flex items-center justify-between">
                <div className="flex items-center space-x-2">
                  <Brush className="w-5 h-5 text-primary" />
                  <CardTitle className="text-lg font-mono">{name}</CardTitle>
                </div>
                {style.type && <Badge variant="secondary">{style.type}</Badge>}
              </div>
              <CardDescription>{style.path}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                <MapLibreMap
                  reuseMaps
                  {...viewState}
                  mapStyle={buildMartinUrl(`/style/${name}`)}
                  onMove={(evt) => setViewState(evt.viewState)}
                  style={{
                    aspectRatio: 16 / 9,
                    backgroundColor: '#E5E7EB',
                    backgroundImage: 'linear-gradient(to bottom right, var(--tw-gradient-stops))',
                    borderRadius: 'var(--radius)',
                    width: '100%',
                  }}
                >
                  <FullscreenControl />
                </MapLibreMap>
                <div className="space-y-2 text-sm text-muted-foreground">
                  {style.versionHash && (
                    <div className="flex justify-between">
                      <span>Version:</span>
                      <span>{style.versionHash}</span>
                    </div>
                  )}
                  {style.layerCount && (
                    <div className="flex justify-between">
                      <span>Layers:</span>
                      <span>{style.layerCount}</span>
                    </div>
                  )}
                  {style.lastModifiedAt && (
                    <div className="flex justify-between">
                      <span>Modified:</span>
                      <span>{style.lastModifiedAt?.toLocaleString()}</span>
                    </div>
                  )}
                </div>
                {style.colors && (
                  <div>
                    <p className="text-sm font-medium mb-2">Color Palette:</p>
                    <div className="flex space-x-1">
                      {style.colors.map((color) => (
                        <div
                          className="w-6 h-6 rounded border border-gray-200"
                          key={color}
                          style={{ backgroundColor: color }}
                          title={color}
                        ></div>
                      ))}
                    </div>
                  </div>
                )}
                <div className="flex flex-col md:flex-row items-center gap-2 mt-4">
                  <Button
                    className="flex-1 w-full"
                    onClick={() => setSelectedStyleForGuide({ name, style })}
                    size="sm"
                    variant="outline"
                  >
                    <Code className="w-4 h-4 mr-2" />
                    Integration Guide
                  </Button>

                  <Button
                    className="flex-1 w-full"
                    onClick={() => onEditStyle?.(name)}
                    size="sm"
                    variant="default"
                  >
                    <SquarePen className="w-4 h-4 mr-2" />
                    Edit
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {filteredStyles.length === 0 && (
        <div className="text-center py-12">
          <p className="text-muted-foreground mb-2">
            {searchQuery ? `No styles found matching "${searchQuery}"` : 'No styles found.'}
          </p>
          <Button asChild size="lg" variant="link">
            <a
              href="https://maplibre.org/martin/sources-styles.html"
              rel="noopener noreferrer"
              target="_blank"
            >
              Learn how to configure Styles
            </a>
          </Button>
        </div>
      )}
    </div>
  );
}
