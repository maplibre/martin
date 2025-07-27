import { type ErrorInfo, useEffect, useState } from 'react';
import { FontCatalog } from '@/components/catalogs/font';
import { ErrorBoundary } from '@/components/error/error-boundary';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Toaster } from '@/components/ui/toaster';
import { useAsyncOperation } from '@/hooks/use-async-operation';
import { useToast } from '@/hooks/use-toast';
import { buildMartinUrl } from '@/lib/api';
import type { CatalogSchema } from '@/lib/types';
import { CatalogSkeleton } from './loading/catalog-skeleton';

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

  // Catalog operation
  const catalogOperation = useAsyncOperation<CatalogSchema>(fetchCatalog, {
    onError: (error: Error) => console.error('Catalog fetch failed:', error),
    showErrorToast: false,
  });

  // Load catalog data
  // biome-ignore lint/correctness/useExhaustiveDependencies: if we list analyticsOperation.execute below, this is an infinte loop
  useEffect(() => {
    catalogOperation.execute();
  }, []);

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
          <CatalogSkeleton
            description="Explore all available tile sources"
            title="Tiles Sources Catalog"
          />
        </TabsContent>

        <TabsContent value="styles">
          <CatalogSkeleton
            description="Preview all available map styles and themes"
            title="Styles Catalog"
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
          <CatalogSkeleton
            description="Preview all available sprite sheets and icons"
            title="Sprite Catalog"
          />
        </TabsContent>
      </Tabs>

      <Toaster />
    </ErrorBoundary>
  );
}
