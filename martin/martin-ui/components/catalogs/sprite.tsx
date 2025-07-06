"use client";

import Link from "next/link";
import { Download, Eye, ImageIcon, Search } from "lucide-react";
import { useState, useMemo } from "react";
import { SpriteDownloadDialog } from "@/components/dialogs/sprite-download";
import { SpritePreviewDialog } from "@/components/dialogs/sprite-preview";
import { ErrorState } from "@/components/error/error-state";
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { SpriteCollection } from "@/lib/types";
import { formatFileSize } from "@/lib/utils";
import SpritePreview from "../sprite/SpritePreview";

interface SpriteCatalogProps {
  spriteCollections?: {
    [sprite_collection_id: string]: SpriteCollection;
  };
  searchQuery?: string;
  onSearchChangeAction?: (query: string) => void;
  isLoading?: boolean;
  error?: string | Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

export function SpriteCatalog({
  spriteCollections,
  searchQuery = "",
  onSearchChangeAction = () => {},
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
}: SpriteCatalogProps) {
  const [selectedSprite, setSelectedSprite] = useState<string | null>(null);
  const [downloadSprite, setDownloadSprite] = useState<string | null>(null);

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
  const filteredSpriteCollections = Object.entries(spriteCollections || {}).map(([name, sprite]) => {
    const allowed = new Set(sprite.images.slice(0, 15));
    // Attach a filter function to each sprite object for preview
    return [
      name,
      {
        ...sprite,
        __previewFilter: (id: string) => allowed.has(id),
      },
    ] as [string, SpriteCollection & { __previewFilter: (id: string) => boolean }];
  }).filter(([name]) =>
    name.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <>
      <div className="space-y-6">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold text-foreground">Sprite Catalog</h2>
            <p className="text-muted-foreground">Preview all available sprite sheets and icons</p>
          </div>
          <div className="relative">
            <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
            <Input
              className="pl-10 w-64 bg-card"
              onChange={(e) => onSearchChangeAction(e.target.value)}
              placeholder="Search sprites..."
              value={searchQuery}
            />
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {filteredSpriteCollections.map(([name, sprite]) => (
            <Card className="hover:shadow-lg transition-shadow flex flex-col" key={name}>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <ImageIcon className="w-5 h-5 text-primary" />
                    <CardTitle className="text-lg">{name}</CardTitle>
                  </div>
                  <Badge variant="secondary">1x, 2x</Badge>
                </div>
                <CardDescription>{sprite.images.length} total icons</CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col p-6 justify-between flex-grow grow-1">
                <div>
                  <div className="p-3 bg-gray-50 rounded-lg text-gray-900">
                    <p className="text-sm font-medium mb-2">Icon Preview:</p>
                    <div className="w-full">
                        <SpritePreview
                          spriteUrl="https://nav.tum.de/tiles/sprite/maki,navigatum"
                          spriteIds={sprite.images}
                          previewMode
                          className="w-full grid grid-cols-6 min-h-[48px]"
                        />
                    </div>
                  </div>
                  {sprite.sizeInBytes && (
                    <div className="space-y-2 text-sm text-muted-foreground mt-4">
                      <div className="flex justify-between">
                        <span>File Size:</span>
                        <span>{formatFileSize(sprite.sizeInBytes)}</span>
                      </div>
                    </div>
                  )}
                </div>
                <div className="flex space-x-2 mt-4">
                  <Button
                    className="flex-1 bg-transparent"
                    onClick={() => setDownloadSprite(name)}
                    size="sm"
                    variant="outline"
                  >
                    <Download className="w-4 h-4 mr-2" />
                    Download
                  </Button>
                  <Button
                    className="flex-1 bg-primary hover:bg-purple-700 text-primary-foreground"
                    onClick={() => setSelectedSprite(name)}
                    size="sm"
                    variant="default"
                  >
                    <Eye className="w-4 h-4 mr-2" />
                    Preview
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        {filteredSpriteCollections.length === 0 && (
          <div className="text-center py-12">
            {searchQuery ? (
              <p className="text-muted-foreground mb-2">
                No sprite collections found matching "{searchQuery}"
              </p>
            ) : (
              <p className="text-muted-foreground mb-2">
                No sprite collections found.
              </p>
            )}
            <Button
              asChild
              variant="link"
              size="lg"
            >
              <Link
                href="https://maplibre.org/martin/sources-sprites.html"
                target="_blank"
                rel="noopener noreferrer"
              >
                Learn how to configure Sprites
              </Link>
            </Button>
          </div>
        )}
      </div>

      {selectedSprite && spriteCollections && (
        <SpritePreviewDialog
          name={selectedSprite}
          onCloseAction={() => setSelectedSprite(null)}
          onDownloadAction={() => {
            setDownloadSprite(selectedSprite);
            setSelectedSprite(null);
          }}
          sprite={spriteCollections[selectedSprite]}
        />
      )}
      {downloadSprite && spriteCollections && (
        <SpriteDownloadDialog
          name={downloadSprite}
          onCloseAction={() => setDownloadSprite(null)}
          sprite={spriteCollections[downloadSprite]}
        />
      )}
    </>
  );
}
