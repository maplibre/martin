import { type ErrorInfo, useCallback, useEffect, useState } from "react";
import { FontCatalog } from "@/components/catalogs/font";
import { SpriteCatalog } from "@/components/catalogs/sprite";
import { StylesCatalog } from "@/components/catalogs/styles";
import { TilesCatalog } from "@/components/catalogs/tiles";
import { ErrorBoundary } from "@/components/error/error-boundary";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toaster } from "@/components/ui/toaster";
import { useAsyncOperation } from "@/hooks/use-async-operation";
import { useToast } from "@/hooks/use-toast";
import { buildMartinUrl } from "@/lib/api";
import type { CatalogSchema } from "@/lib/types";

const fetchCatalog = async (): Promise<CatalogSchema> => {
  const res = await fetch(buildMartinUrl("/catalog"));
  if (!res.ok) {
    throw new Error(`Failed to fetch catalog: ${res.statusText}`);
  }
  return res.json();
};

export function DashboardContent() {
  const { toast } = useToast();
  const [activeTab, setActiveTab] = useState("tiles");
  const [searchQuery, setSearchQuery] = useState("");

  const handleTabChange = (value: string) => {
    setActiveTab(value);
  };

  const handleCatalogError = useCallback((error: Error) => {
    console.error("Catalog fetch failed:", error);
  }, []);

  // Catalog operation - unified data fetching
  const catalogOperation = useAsyncOperation<CatalogSchema>(fetchCatalog, {
    onError: handleCatalogError,
    showErrorToast: false,
  });

  // Load catalog data
  useEffect(() => {
    catalogOperation.execute();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [catalogOperation.execute]);

  return (
    <ErrorBoundary
      onError={(error: Error, errorInfo: ErrorInfo) => {
        console.error("Application error:", error, errorInfo);
        toast({
          description: "An unexpected error occurred. The page will reload automatically.",
          title: "Application Error",
          variant: "destructive",
        });

        // Auto-reload after 3 seconds
        setTimeout(() => {
          window.location.reload();
        }, 3000);
      }}
    >
      <Tabs className="space-y-6" onValueChange={handleTabChange} value={activeTab}>
        <TabsList className="grid w-full grid-cols-4">
          <TabsTrigger value="tiles">Data Catalog</TabsTrigger>
          <TabsTrigger value="styles">Styles Catalog</TabsTrigger>
          <TabsTrigger value="fonts">Font Catalog</TabsTrigger>
          <TabsTrigger value="sprites">Sprite Catalog</TabsTrigger>
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
