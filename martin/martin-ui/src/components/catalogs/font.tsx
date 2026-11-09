import { Type } from 'lucide-react';
import { CatalogEmpty } from '@/components/catalogs/catalog-empty';
import { CatalogHeader } from '@/components/catalogs/catalog-header';

import { ErrorState } from '@/components/error/error-state';
import { FontPreviewCanvas } from '@/components/font-preview-canvas';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { TooltipCopyText } from '@/components/ui/tooltip-copy-text';
import { buildMartinUrl } from '@/lib/api';
import type { Catalog } from '@/lib/types.gen';
import { CopyLinkButton } from '../ui/copy-link-button';

interface FontCatalogProps {
  fonts?: Catalog['fonts'];
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
      (font.style ?? '').toLowerCase().includes(searchQuery.toLowerCase()),
  );

  return (
    <div className="space-y-6">
      <CatalogHeader
        count={filteredFonts.length}
        description="Preview all available font assets"
        onSearchChange={onSearchChangeAction}
        searchPlaceholder="Search fonts..."
        searchQuery={searchQuery}
        title="Font Catalog"
      />

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 lg:gap-5">
        {filteredFonts.map(([name, font]) => (
          <Card
            className="flex min-w-0 flex-col transition-shadow hover:shadow-card-hover"
            key={name}
          >
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between gap-2">
                <Type aria-hidden="true" className="size-4 text-brand" />
                {font.format ? (
                  <Badge className="uppercase" variant="secondary">
                    {font.format}
                  </Badge>
                ) : null}
              </div>
              <CardTitle>
                <TooltipCopyText
                  className="font-sans text-base font-semibold [&_code]:font-[inherit]"
                  text={name}
                />
              </CardTitle>
              <CardDescription className="[overflow-wrap:anywhere]">
                Family: {font.family}
                {font.style ? ` • Style: ${font.style}` : ''}
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-1 flex-col gap-4">
              <div className="rounded-lg border bg-sunken px-3 py-2 text-foreground">
                <FontPreviewCanvas fontName={name} />
              </div>
              <div className="flex justify-between gap-3 text-sm">
                <span className="text-muted-foreground">Glyph count</span>
                <span className="tabular-nums">{font.glyphs}</span>
              </div>
              <div className="mt-auto flex gap-2">
                <CopyLinkButton
                  className="min-w-0 flex-1"
                  link={buildMartinUrl(`/font/${name}/{range}`)}
                  size="sm"
                  toastMessage="Font link copied!"
                  variant="outline"
                />
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {filteredFonts.length === 0 && (
        <CatalogEmpty
          icon={Type}
          linkHref="https://maplibre.org/martin/sources-fonts.html"
          linkLabel="Learn how to configure Font Sources"
          message={searchQuery ? `No fonts found matching "${searchQuery}".` : 'No fonts found.'}
        />
      )}
    </div>
  );
}
