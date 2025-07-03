"use client";

import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Database, Eye, ImageIcon, Layers, Palette, Search } from "lucide-react";
import { ErrorState } from "@/components/error/error-state";
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { TileSource } from "@/lib/types";
import { DisabledNonInteractiveButton } from "../ui/disabledNonInteractiveButton";
import { Tooltip, TooltipContent, TooltipTrigger } from "../ui/tooltip";

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
  if (isLoading) {
    return (
      <CatalogSkeleton
        description="Explore all available tile sources, sprites, and fonts"
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

  const filteredTileSources = Object.entries(tileSources || {}).filter(
    ([name, source]) =>
      name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      source.name?.toLowerCase().includes(searchQuery.toLowerCase()) ||
      source.description?.toLowerCase().includes(searchQuery.toLowerCase()) ||
      source.attribution?.toLowerCase().includes(searchQuery.toLowerCase()) ||
      source.content_encoding?.toLowerCase().includes(searchQuery.toLowerCase()) ||
      source.content_type?.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  const getIcon = (content_type: string) => {
    if (content_type.startsWith("image/")) return <ImageIcon className="w-5 h-5 text-primary" />;
    if (content_type === "application/x-protobuf")
      return <Layers className="w-5 h-5 text-primary" />;
    return <Database className="w-5 h-5 text-primary" />;
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-foreground">Tiles Sources Catalog</h2>
          <p className="text-muted-foreground">
            Explore all available tile sources, sprites, and fonts
          </p>
        </div>
        <div className="relative">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
          <Input
            className="pl-10 w-64 bg-card"
            onChange={(e) => onSearchChangeAction(e.target.value)}
            placeholder="Search tiles sources..."
            value={searchQuery}
          />
        </div>
      </div>

      <div>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {filteredTileSources.map(([name, source]) => (
            <Card className="hover:shadow-lg transition-shadow" key={name}>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    {getIcon(source.content_type)}
                    <CardTitle className="text-lg">{name}</CardTitle>
                  </div>
                  <Badge variant="secondary">{source.content_type}</Badge>
                </div>
                {(source.description || source.name) && (
                  <CardDescription>
                    {source.name}
                    {source.description && source.name && <br />}
                    {source.description}
                  </CardDescription>
                )}
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
                <div className="flex space-x-2 mt-4">
                  <Tooltip>
                    <TooltipTrigger className="flex flex-1">
                      <DisabledNonInteractiveButton
                        className="flex-1 bg-transparent"
                        size="sm"
                        variant="outline"
                      >
                        <Eye className="w-4 h-4 mr-2" />
                        Inspect
                      </DisabledNonInteractiveButton>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Not currently implemented in the frontend</p>
                    </TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger className="flex flex-1">
                      <DisabledNonInteractiveButton className="flex-1" size="sm">
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
            {searchQuery
              ? <>No tile sources found matching "{searchQuery}".</>
              : <>No tile sources found.</>
            }
          </p>
          <Button
            asChild
            variant="link"
            size="lg"
          >
            <Link
              href="https://maplibre.org/martin/sources-tiles.html"
              target="_blank"
              rel="noopener noreferrer"
            >
              Learn how to configure Tile Sources
            </Link>
          </Button>
        </div>
      )}
    </div>
  );
}
