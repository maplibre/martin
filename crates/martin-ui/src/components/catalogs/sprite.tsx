import { Download, Eye, ImageIcon } from 'lucide-react';

import { CatalogEmpty } from '@/components/catalogs/catalog-empty';
import { CatalogHeader } from '@/components/catalogs/catalog-header';
import { SpriteDownloadDialog } from '@/components/dialogs/sprite-download';
import { SpritePreviewDialog } from '@/components/dialogs/sprite-preview';
import { ErrorState } from '@/components/error/error-state';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { TooltipCopyText } from '@/components/ui/tooltip-copy-text';
import type { Catalog } from '@/lib/types.gen';
import { formatFileSize } from '@/lib/utils';
import SpritePreview from '../sprite/SpritePreview';

interface SpriteCatalogProps {
  spriteCollections?: Catalog['sprites'];
  searchQuery?: string;
  isLoading?: boolean;
  error?: string | Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
  selectedSprite?: string | null;
  downloadSprite?: string | null;
  onPreviewSprite?: (spriteName: string | undefined) => void;
  onDownloadSprite?: (spriteName: string | undefined) => void;
}

export function SpriteCatalog({
  spriteCollections,
  searchQuery = '',
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
  selectedSprite = null,
  downloadSprite = null,
  onPreviewSprite = () => {},
  onDownloadSprite = () => {},
}: SpriteCatalogProps) {
  if (isLoading) {
    return (
      <CatalogSkeleton
        description="Preview all available sprite sheets and icons"
        title="Sprite Catalog"
      />
    );
  }

  if (error) {
    return (
      <ErrorState
        description="Unable to fetch sprite catalog from the server"
        error={error}
        isRetrying={isRetrying}
        onRetry={onRetry}
        showDetails={true}
        title="Failed to Load Sprites"
        variant="server"
      />
    );
  }

  // Prepare preview filters outside the render loop
  const filteredSpriteCollections = Object.entries(spriteCollections || {}).filter(([name]) =>
    name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <>
      <div className="space-y-6">
        <CatalogHeader
          count={filteredSpriteCollections.length}
          noun={['sprite sheet', 'sprite sheets']}
          searchQuery={searchQuery}
          total={Object.keys(spriteCollections ?? {}).length}
        />

        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 lg:gap-5">
          {filteredSpriteCollections.map(([name, sprite]) => (
            <Card
              className="flex min-w-0 flex-col transition-shadow hover:shadow-card-hover"
              key={name}
            >
              <CardHeader className="pb-3">
                <CardTitle>
                  <TooltipCopyText text={name} />
                </CardTitle>
                <CardDescription className="tabular-nums">
                  {sprite.images.length} {sprite.images.length === 1 ? 'icon' : 'icons'}
                  {sprite.size_in_bytes ? ` · ${formatFileSize(sprite.size_in_bytes)}` : null}
                </CardDescription>
              </CardHeader>
              <CardContent className="flex grow flex-col justify-between gap-4">
                <div className="checker rounded-lg border p-3 text-paper-foreground">
                  <SpritePreview
                    className="flex min-h-14 w-full flex-wrap items-center justify-center gap-2"
                    previewMode
                    spriteIds={sprite.images}
                    spriteUrl={`/sprite/${name}`}
                  />
                </div>
                <div className="flex gap-2">
                  <Button
                    className="min-w-0 flex-1"
                    onClick={() => onPreviewSprite(name)}
                    size="sm"
                    variant="default"
                  >
                    <Eye />
                    Preview
                  </Button>
                  <Button
                    className="min-w-0 flex-1"
                    onClick={() => onDownloadSprite(name)}
                    size="sm"
                    variant="outline"
                  >
                    <Download />
                    Download
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        {filteredSpriteCollections.length === 0 && (
          <CatalogEmpty
            icon={ImageIcon}
            linkHref="https://maplibre.org/martin/sources-sprites.html"
            linkLabel="Learn how to configure Sprites"
            message={
              searchQuery
                ? `No sprite collections found matching "${searchQuery}"`
                : 'No sprite collections found.'
            }
          />
        )}
      </div>

      {selectedSprite && spriteCollections ? (
        <SpritePreviewDialog
          name={selectedSprite}
          onCloseAction={() => onPreviewSprite(undefined)}
          onDownloadAction={() => {
            onDownloadSprite(selectedSprite);
            onPreviewSprite(undefined);
          }}
          sprite={spriteCollections[selectedSprite]}
        />
      ) : null}
      {downloadSprite && spriteCollections ? (
        <SpriteDownloadDialog
          name={downloadSprite}
          onCloseAction={() => onDownloadSprite(undefined)}
          sprite={spriteCollections[downloadSprite]}
        />
      ) : null}
    </>
  );
}
