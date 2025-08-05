import { Eye, Search, Type } from 'lucide-react';

import { ErrorState } from '@/components/error/error-state';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { buildMartinUrl } from '@/lib/api';
import type { Font } from '@/lib/types';
import { CopyLinkButton } from '../ui/copy-link-button';
import { DisabledNonInteractiveButton } from '../ui/disabled-non-interactive-button';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/tooltip';

interface FontCatalogProps {
  fonts?: { [name: string]: Font };
  searchQuery?: string;
  onSearchChangeAction?: (query: string) => void;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

export function FontCatalog({
  fonts,
  searchQuery = '',
  onSearchChangeAction = () => {},
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
}: FontCatalogProps) {
  if (isLoading) {
    return <CatalogSkeleton description="Preview all available font assets" title="Font Catalog" />;
  }

  if (error) {
    return (
      <ErrorState
        description="Unable to fetch font catalog from the server"
        error={error}
        isRetrying={isRetrying}
        onRetry={onRetry}
        showDetails={true}
        title="Failed to Load Fonts"
        variant="server"
      />
    );
  }

  const filteredFonts = Object.entries(fonts || {}).filter(
    ([name, font]) =>
      name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      font.family.toLowerCase().includes(searchQuery.toLowerCase()) ||
      font.style.toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row md:items-center items-start justify-between gap-5">
        <div>
          <h2 className="text-2xl font-bold text-foreground">Font Catalog</h2>
          <p className="text-muted-foreground">Preview all available font assets</p>
        </div>
        <div className="relative w-full md:w-64">
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
          <Input
            className="pl-10 md:w-64 w-full bg-card"
            onChange={(e) => onSearchChangeAction(e.target.value)}
            placeholder="Search fonts..."
            value={searchQuery}
          />
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        {filteredFonts.map(([name, font]) => (
          <Card className="hover:shadow-lg transition-shadow" key={name}>
            <CardHeader>
              <div className="flex items-center justify-between">
                <div className="flex items-center space-x-2">
                  <Type className="w-5 h-5 text-primary" />
                  <CardTitle className="text-lg font-mono">{name}</CardTitle>
                </div>
                {font.format && (
                  <Badge className="uppercase" variant="secondary">
                    {font.format}
                  </Badge>
                )}
              </div>
              <CardDescription>
                Family: {font.family} • Style: {font.style}
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="space-y-4">
                <div className="p-3 bg-gray-50 text-gray-900 rounded-lg">
                  <p className="text-sm font-medium mb-2 text-gray-900">Preview:</p>
                  <Tooltip>
                    <TooltipTrigger className="cursor-help">
                      <p
                        className="text-base text-gray-900 blur-sm animate-pulse"
                        style={{ fontFamily: font.family, fontWeight: 500 }}
                      >
                        The quick brown fox jumps over the lazy dog
                      </p>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Not currently implemented in the frontend</p>
                    </TooltipContent>
                  </Tooltip>
                </div>
                <div className="space-y-2 text-sm text-muted-foreground">
                  <div className="flex justify-between">
                    <span>Glyph count:</span>
                    <span>{font.glyphs}</span>
                  </div>
                </div>
                <div className="flex flex-col md:flex-row items-center gap-2 mt-4">
                  <CopyLinkButton
                    className="flex-1 bg-transparent w-full"
                    link={buildMartinUrl(`/font/${name}/{range}`)}
                    size="sm"
                    toastMessage="Font link copied!"
                    variant="outline"
                  />
                  <Tooltip>
                    <TooltipTrigger className="flex-1 flex cursor-help w-full">
                      <DisabledNonInteractiveButton
                        className="flex-1 grow w-full"
                        disabled
                        size="sm"
                      >
                        <Eye className="w-4 h-4 mr-2" />
                        Details
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

      {filteredFonts.length === 0 && (
        <div className="text-center py-12">
          <p className="text-muted-foreground mb-2">
            {searchQuery ? <>No fonts found matching "{searchQuery}".</> : 'No fonts found.'}
          </p>
          <Button asChild size="lg" variant="link">
            <a
              href="https://maplibre.org/martin/sources-fonts.html"
              rel="noopener noreferrer"
              target="_blank"
            >
              Learn how to configure Font Sources
            </a>
          </Button>
        </div>
      )}
    </div>
  );
}
