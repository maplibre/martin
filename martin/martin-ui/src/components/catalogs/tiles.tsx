import { Database, Eye, ImageIcon, Layers, Palette, Search } from 'lucide-react';
import { useState } from 'react';
import { TileInspectDialog } from '@/components/dialogs/tile-inspect';
import { ErrorState } from '@/components/error/error-state';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import type { TileSource } from '@/lib/types';
import { DisabledNonInteractiveButton } from '../ui/disabled-non-interactive-button';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/tooltip';

interface TilesCatalogProps {
  tileSources?: { [tile_id: string]: TileSource };
  searchQuery: string;
  onSearchChangeAction: (query: string) => void;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

export function TilesCatalog({
  tileSources,
  searchQuery,
  onSearchChangeAction,
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
}: TilesCatalogProps) {
  const [selectedTileForInspection, setSelectedTileForInspection] = useState<string | null>(null);
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

  const getIcon = (content_type: string) => {
    if (content_type.startsWith('image/')) return <ImageIcon className="w-5 h-5 text-primary" />;
    if (content_type === 'application/x-protobuf')
      return <Layers className="w-5 h-5 text-primary" />;
    return <Database className="w-5 h-5 text-primary" />;
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row md:items-center items-start justify-between gap-5">
        <div>
          <h2 className="text-2xl font-bold text-foreground">Tile Sources Catalog</h2>
          <p className="text-muted-foreground">Explore all available tile sources</p>
        </div>
        <div className="relative w-full md:w-64">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
          <Input
            className="pl-10 md:w-64 w-full bg-card"
            onChange={(e) => onSearchChangeAction(e.target.value)}
            placeholder="Search tile sources..."
            value={searchQuery}
          />
        </div>
      </div>

      <div>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {filteredTileSources.map(([name, source]) => (
            <Card className="hover:shadow-lg transition-shadow" key={name}>
              <CardHeader>
                <div className="flex flex-col md:flex-row items-center justify-between gap-2 mb-4">
                  <div className="flex items-center space-x-2">
                    {getIcon(source.content_type)}
                    <CardTitle className="text-lg font-mono">{name}</CardTitle>
                  </div>
                  <Badge variant="secondary">{source.content_type}</Badge>
                </div>
                <div className="text-center md:text-start break-all text-balance">
                  {(source.description || source.name) && (
                    <CardDescription>
                      {source.name}
                      {source.description && source.name && <br />}
                      {source.description}
                    </CardDescription>
                  )}
                </div>
              </CardHeader>
              <CardContent>
                <div className="space-y-2 text-sm text-muted-foreground">
                  {source.layerCount && (
                    <div className="flex justify-between">
                      <span>Layers:</span>
                      <span>{source.layerCount}</span>
                    </div>
                  )}
                </div>
                <div className="flex flex-col md:flex-row items-center gap-2 mt-4">
                  <Button
                    className="flex-1 bg-transparent w-full"
                    onClick={() => setSelectedTileForInspection(name)}
                    size="sm"
                    variant="outline"
                  >
                    <Eye className="w-4 h-4 mr-2" />
                    Inspect
                  </Button>
                  <Tooltip>
                    <TooltipTrigger className="flex flex-1 w-full cursor-help">
                      <DisabledNonInteractiveButton className="flex-1 w-full" size="sm">
                        <Palette className="w-4 h-4 mr-2" />
                        Style
                      </DisabledNonInteractiveButton>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Not currently implemented in the frontend</p>
                    </TooltipContent>
                  </Tooltip>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      </div>

      {filteredTileSources.length === 0 && (
        <div className="text-center py-12">
          <p className="text-muted-foreground mb-2">
            {searchQuery ? (
              <>No tile sources found matching "{searchQuery}".</>
            ) : (
              'No tile sources found.'
            )}
          </p>
          <Button asChild size="lg" variant="link">
            <a
              href="https://maplibre.org/martin/sources-tiles.html"
              rel="noopener noreferrer"
              target="_blank"
            >
              Learn how to configure Tile Sources
            </a>
          </Button>
        </div>
      )}

      {selectedTileForInspection && tileSources && (
        <TileInspectDialog
          name={selectedTileForInspection}
          onCloseAction={() => setSelectedTileForInspection(null)}
          source={tileSources[selectedTileForInspection]}
        />
      )}
    </div>
  );
}
