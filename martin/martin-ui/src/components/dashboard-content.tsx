import { type ErrorInfo, useCallback, useEffect, useState } from 'react';
import { FontCatalog } from '@/components/catalogs/font';
import { SpriteCatalog } from '@/components/catalogs/sprite';
import { StylesCatalog } from '@/components/catalogs/styles';
import { ErrorBoundary } from '@/components/error/error-boundary';
import { StyleEditor } from '@/components/style-editor';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Toaster } from '@/components/ui/toaster';
import { useAsyncOperation } from '@/hooks/use-async-operation';
import { useToast } from '@/hooks/use-toast';
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
  const [activeTab, setActiveTab] = useState('tiles');
  const [searchQuery, setSearchQuery] = useState('');
  const [editingStyle, setEditingStyle] = useState<string | null>(null);

  // Catalog operation
  const catalogOperation = useAsyncOperation<CatalogSchema>(fetchCatalog, {
    onError: (error: Error) => console.error('Catalog fetch failed:', error),
    showErrorToast: false,
  });

  const handleEditStyle = useCallback((styleName: string) => {
    setEditingStyle(styleName);
  }, []);

  const handleCloseEditor = useCallback(() => {
    setEditingStyle(null);
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: if we list analyticsOperation.execute below, this is an infinte loop
  useEffect(() => {
    catalogOperation.execute();
  }, []);

  // If editing a style, show the editor
  if (editingStyle && catalogOperation.data?.styles?.[editingStyle]) {
    return (
      <StyleEditor
        onClose={handleCloseEditor}
        style={catalogOperation.data.styles[editingStyle]}
        styleName={editingStyle}
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
      <Tabs className="space-y-6" onValueChange={(value) => setActiveTab(value)} value={activeTab}>
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
            onSearchChangeAction={setSearchQuery}
            searchQuery={searchQuery}
            tileSources={catalogOperation.data?.tiles}
          />
        </TabsContent>

        <TabsContent value="styles">
          <StylesCatalog
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onEditStyle={handleEditStyle}
            onSearchChangeAction={setSearchQuery}
            searchQuery={searchQuery}
            styles={catalogOperation.data?.styles}
          />
        </TabsContent>

        <TabsContent value="fonts">
          <FontCatalog
            error={catalogOperation.error}
            fonts={catalogOperation.data?.fonts}
            isLoading={catalogOperation.isLoading}
            onSearchChangeAction={setSearchQuery}
            searchQuery={searchQuery}
          />
        </TabsContent>

        <TabsContent value="sprites">
          <SpriteCatalog
            error={catalogOperation.error}
            isLoading={catalogOperation.isLoading}
            onSearchChangeAction={setSearchQuery}
            searchQuery={searchQuery}
            spriteCollections={catalogOperation.data?.sprites}
          />
        </TabsContent>
      </Tabs>

      <Toaster />
    </ErrorBoundary>
  );
}
