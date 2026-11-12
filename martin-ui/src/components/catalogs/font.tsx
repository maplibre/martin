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

type FontInfo = NonNullable<Catalog['fonts']>[string];

/** Family and style when the name does not already say them, then the glyph count. */
function fontFacts(name: string, font: FontInfo) {
  const facts: string[] = [];
  if (!name.includes(font.family)) facts.push(font.family);
  if (font.style && !name.includes(font.style)) facts.push(font.style);
  facts.push(`${font.glyphs.toLocaleString()} glyphs`);
  return facts;
}

interface FontCatalogProps {
  fonts?: Catalog['fonts'];
  searchQuery?: string;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

export function FontCatalog({
  fonts,
  searchQuery = '',
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
        noun={['font', 'fonts']}
        searchQuery={searchQuery}
        total={Object.keys(fonts ?? {}).length}
      />

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 lg:gap-5">
        {filteredFonts.map(([name, font]) => (
          <Card
            className="flex min-w-0 flex-col transition-shadow hover:shadow-card-hover"
            key={name}
          >
            <CardHeader className="pb-3">
              <div className="flex items-start justify-between gap-3">
                <CardTitle className="min-w-0">
                  <TooltipCopyText text={name} />
                </CardTitle>
                {font.format ? (
                  <Badge className="uppercase" variant="secondary">
                    {font.format}
                  </Badge>
                ) : null}
              </div>
              <CardDescription className="tabular-nums [overflow-wrap:anywhere]">
                {fontFacts(name, font).join(' · ')}
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-1 flex-col gap-4">
              <div className="rounded-lg border bg-sunken px-3 py-2 text-foreground">
                <FontPreviewCanvas fontName={name} />
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
