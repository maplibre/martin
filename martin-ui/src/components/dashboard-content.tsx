import { type ErrorInfo, useEffect } from 'react';
import { CatalogSearch } from '@/components/catalogs/catalog-search';
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
import { martinClient } from '@/lib/martin-client';
import type { Catalog } from '@/lib/types.gen';
import { TilesCatalog } from './catalogs/tiles';

/** Search box hint for each catalog tab. */
const SEARCH_PLACEHOLDERS: Record<string, string> = {
  fonts: 'Search fonts...',
  sprites: 'Search sprites...',
  styles: 'Search styles...',
  tiles: 'Search tile sources...',
};

const fetchCatalog = async (): Promise<Catalog> => {
  const { data, response } = await martinClient.GET('/catalog');
  if (data === undefined) {
    throw new Error(`Failed to fetch catalog: ${response.statusText}`);
  }
  return data;
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
  const catalogOperation = useAsyncOperation<Catalog>(fetchCatalog, {
    onError: (error: Error) => console.error('Catalog fetch failed:', error),
    showErrorToast: false,
  });

  const handleSearchChange = (query: string) => updateParam('search', query);

  useEffect(() => {
    catalogOperation.execute();
  }, []);

  // If editing a style, show the editor
  if (params.style && catalogOperation.data?.styles?.[params.style]) {
    return (
      <StyleEditor
        onClose={() => updateParam('style', undefined)}
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
        className="space-y-5"
        onValueChange={(value) => updateParam('tab', value)}
        value={params.tab}
      >
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <TabsList className="grid w-full grid-cols-4 sm:inline-flex sm:w-auto">
            <TabsTrigger value="tiles">Tiles</TabsTrigger>
            <TabsTrigger value="styles">Styles</TabsTrigger>
            <TabsTrigger value="fonts">Fonts</TabsTrigger>
            <TabsTrigger value="sprites">Sprites</TabsTrigger>
          </TabsList>
          <CatalogSearch
            onChange={handleSearchChange}
            placeholder={SEARCH_PLACEHOLDERS[params.tab ?? 'tiles'] ?? 'Search...'}
            value={params.search ?? ''}
          />
        </div>

        <TabsContent className="mt-0" value="tiles">
          <TilesCatalog
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onInspectTile={(tileName) => updateParam('inspect', tileName)}
            searchQuery={params.search ?? ''}
            selectedTileForInspection={params.inspect}
            tileSources={catalogOperation.data?.tiles}
          />
        </TabsContent>

        <TabsContent className="mt-0" value="styles">
          <StylesCatalog
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onEditStyle={(styleName) => updateParam('style', styleName)}
            onStyleGuide={(styleName) => updateParam('guide', styleName)}
            searchQuery={params.search ?? ''}
            selectedStyleForGuide={params.guide}
            styles={catalogOperation.data?.styles}
          />
        </TabsContent>

        <TabsContent className="mt-0" value="fonts">
          <FontCatalog
            error={catalogOperation.error}
            fonts={catalogOperation.data?.fonts}
            isLoading={catalogOperation.isLoading}
            searchQuery={params.search ?? ''}
          />
        </TabsContent>

        <TabsContent className="mt-0" value="sprites">
          <SpriteCatalog
            downloadSprite={params.download}
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onDownloadSprite={(spriteName) => updateParam('download', spriteName)}
            onPreviewSprite={(spriteName) => updateParam('preview', spriteName)}
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
