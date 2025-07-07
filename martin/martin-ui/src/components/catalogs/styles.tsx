import { Brush, Eye, Map, Search } from "lucide-react";

import { ErrorState } from "@/components/error/error-state";
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { Style } from "@/lib/types";
import { DisabledNonInteractiveButton } from "../ui/disabledNonInteractiveButton";
import { Tooltip, TooltipContent, TooltipTrigger } from "../ui/tooltip";
import { CopyLinkButton } from "../ui/copy-link-button";
import { buildMartinUrl } from "@/lib/api";
import 'maplibre-gl/dist/maplibre-gl.css';
import {Map as MapLibreMap} from '@vis.gl/react-maplibre';
import {useState} from "react"

interface StylesCatalogProps {
  styles?: { [name: string]: Style };
  searchQuery?: string;
  onSearchChangeAction?: (query: string) => void;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

export function StylesCatalog({
  styles,
  searchQuery = "",
  onSearchChangeAction = () => {},
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
}: StylesCatalogProps) {
  const [viewState, setViewState] = useState({
    longitude: -100,
    latitude: 40,
    zoom: 3.5
  });
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
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-foreground">Styles Catalog</h2>
          <p className="text-muted-foreground">
            Browse and preview all available map styles and themes
          </p>
        </div>
        <div className="relative">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
          <Input
            className="pl-10 w-64 bg-card"
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
                  <CardTitle className="text-lg">{name}</CardTitle>
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
                    onMove={evt => setViewState(evt.viewState)}
                    style={{
                      width: "100%",
                      aspectRatio: 16/9,
                      borderRadius: "var(--radius)",
                      backgroundImage: "linear-gradient(to bottom right, var(--tw-gradient-stops))",
                      backgroundColor: "#E5E7EB",
                    }}
                    mapStyle={buildMartinUrl(`/style/${name}`)}
                  />
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
                      {style.colors.map((color, i) => (
                        <div
                          className="w-6 h-6 rounded border border-gray-200"
                          key={i}
                          style={{ backgroundColor: color }}
                          title={color}
                        ></div>
                      ))}
                    </div>
                  </div>
                )}
                <div className="flex space-x-2">
                  <CopyLinkButton
                    className="flex-1 bg-transparent"
                    size="sm"
                    variant="outline"
                    link={buildMartinUrl(`/style/${name}`)}
                    toastMessage="Style link copied!"
                  />

                  <Tooltip>
                    <TooltipTrigger className="flex flex-1">
                      <DisabledNonInteractiveButton className="flex-1" size="sm">
                        <Eye className="w-4 h-4 mr-2" />
                        Preview
                      </DisabledNonInteractiveButton>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Not currently implemented in the frontend</p>
                    </TooltipContent>
                  </Tooltip>
                </div>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {filteredStyles.length === 0 && (
        <div className="text-center py-12">
          <p className="text-muted-foreground mb-2">
            {searchQuery
              ? `No styles found matching "${searchQuery}"`
              : "No styles found."
            }
          </p>
          <Button
            asChild
            variant="link"
            size="lg"
          >
            <a
              href="https://maplibre.org/martin/sources-styles.html"
              target="_blank"
              rel="noopener noreferrer"
            >
              Learn how to configure Styles
            </a>
          </Button>
        </div>
      )}
    </div>
  );
}
