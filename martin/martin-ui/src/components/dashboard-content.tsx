import { type ErrorInfo, useCallback, useEffect } from 'react';
import { FontCatalog } from '@/components/catalogs/font';
import { SpriteCatalog } from '@/components/catalogs/sprite';
import { StylesCatalog } from '@/components/catalogs/styles';
import { ErrorBoundary } from '@/components/error/error-boundary';
import { StyleEditor } from '@/components/style-editor';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Toaster } from '@/components/ui/toaster';
import { useAsyncOperation } from '@/hooks/use-async-operation';
import { useToast } from '@/hooks/use-toast';
import { useURLParams } from '@/hooks/use-url-params';
import { buildMartinUrl } from '@/lib/api';
import type { CatalogSchema } from '@/lib/types';
import { TilesCatalog } from './catalogs/tiles';

const fetchCatalog = async (): Promise<CatalogSchema> => {
  const res = await fetch(buildMartinUrl('/catalog'));
  if (!res.ok) {
    throw new Error(`Failed to fetch catalog: ${res.statusText}`);
  }
  return res.json();
};

export function DashboardContent() {
  const { toast } = useToast();
  const { params, updateParam } = useURLParams({
    download: undefined,
    guide: undefined,
    inspect: undefined,
    preview: undefined,
    search: '',
    style: undefined,
    tab: 'tiles',
  });

  // Catalog operation
  const catalogOperation = useAsyncOperation<CatalogSchema>(fetchCatalog, {
    onError: (error: Error) => console.error('Catalog fetch failed:', error),
    showErrorToast: false,
  });

  const handleEditStyle = useCallback(
    (styleName: string) => {
      updateParam('style', styleName);
    },
    [updateParam],
  );

  const handleCloseEditor = useCallback(() => {
    updateParam('style', undefined);
  }, [updateParam]);

  const handleSearchChange = useCallback(
    (query: string) => updateParam('search', query),
    [updateParam],
  );

  const handleInspectTile = useCallback(
    (tileName: string | undefined) => updateParam('inspect', tileName),
    [updateParam],
  );

  const handlePreviewSprite = useCallback(
    (spriteName: string | undefined) => updateParam('preview', spriteName),
    [updateParam],
  );

  const handleDownloadSprite = useCallback(
    (spriteName: string | undefined) => updateParam('download', spriteName),
    [updateParam],
  );

  const handleStyleGuide = useCallback(
    (styleName: string | undefined) => updateParam('guide', styleName),
    [updateParam],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: if we list analyticsOperation.execute below, this is an infinte loop
  useEffect(() => {
    catalogOperation.execute();
  }, []);

  // If editing a style, show the editor
  if (params.style && catalogOperation.data?.styles?.[params.style]) {
    return (
      <StyleEditor
        onClose={handleCloseEditor}
        style={catalogOperation.data.styles[params.style]}
        styleName={params.style}
      />
    );
  }

  return (
    <ErrorBoundary
      onError={(error: Error, errorInfo: ErrorInfo) => {
        console.error('Application error:', error, errorInfo);
        toast({
          description: 'An unexpected error occurred. The page will reload automatically.',
          title: 'Application Error',
          variant: 'destructive',
        });

        // Auto-reload after 3 seconds
        setTimeout(() => {
          window.location.reload();
        }, 3000);
      }}
    >
      <Tabs
        className="space-y-6"
        onValueChange={(value) => updateParam('tab', value)}
        value={params.tab}
      >
        <TabsList className="grid w-full grid-cols-4">
          <TabsTrigger value="tiles">
            Tiles<span className="md:block hidden ms-1">Catalog</span>
          </TabsTrigger>
          <TabsTrigger value="styles">
            Styles<span className="md:block hidden ms-1">Catalog</span>
          </TabsTrigger>
          <TabsTrigger value="fonts">
            Fonts<span className="md:block hidden ms-1">Catalog</span>
          </TabsTrigger>
          <TabsTrigger value="sprites">
            Sprites<span className="md:block hidden ms-1">Catalog</span>
          </TabsTrigger>
        </TabsList>

        <TabsContent value="tiles">
          <TilesCatalog
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onInspectTile={handleInspectTile}
            onSearchChangeAction={handleSearchChange}
            searchQuery={params.search ?? ''}
            selectedTileForInspection={params.inspect}
            tileSources={catalogOperation.data?.tiles}
          />
        </TabsContent>

        <TabsContent value="styles">
          <StylesCatalog
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onEditStyle={handleEditStyle}
            onSearchChangeAction={handleSearchChange}
            onStyleGuide={handleStyleGuide}
            searchQuery={params.search ?? ''}
            selectedStyleForGuide={params.guide}
            styles={catalogOperation.data?.styles}
          />
        </TabsContent>

        <TabsContent value="fonts">
          <FontCatalog
            error={catalogOperation.error}
            fonts={catalogOperation.data?.fonts}
            isLoading={catalogOperation.isLoading}
            onSearchChangeAction={handleSearchChange}
            searchQuery={params.search ?? ''}
          />
        </TabsContent>

        <TabsContent value="sprites">
          <SpriteCatalog
            downloadSprite={params.download}
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onDownloadSprite={handleDownloadSprite}
            onPreviewSprite={handlePreviewSprite}
            onSearchChangeAction={handleSearchChange}
            searchQuery={params.search ?? ''}
            selectedSprite={params.preview}
            spriteCollections={catalogOperation.data?.sprites}
          />
        </TabsContent>
      </Tabs>

      <Toaster />
    </ErrorBoundary>
  );
}
