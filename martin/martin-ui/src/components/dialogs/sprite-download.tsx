import { ChevronDown, Copy, CopyCheck, Download, ExternalLink } from 'lucide-react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { buildMartinUrl } from '@/lib/api';
import type { Catalog } from '@/lib/types.gen';

interface SpriteDownloadDialogProps {
  name: string;
  sprite: Catalog['sprites'][string];
  onCloseAction: () => void;
}

interface SpriteFormat {
  label: string;
  url: string;
  description: string;
  primary?: boolean;
}

export function SpriteDownloadDialog({ name, sprite, onCloseAction }: SpriteDownloadDialogProps) {
  const { copy, copiedValue } = useCopyToClipboard();
  if (!sprite) return null;

  // Generate sprite format URLs
  const pngFormats: SpriteFormat[] = [
    {
      description: 'Sprite coordinates and metadata',
      label: 'PNG JSON',
      url: buildMartinUrl(`/sprite/${name}.json`),
    },
    {
      description: 'Standard sprite format with full color support',
      label: 'PNG Spritesheet',
      primary: true,
      url: buildMartinUrl(`/sprite/${name}.png`),
    },
    {
      description: 'High resolution sprites for retina displays',
      label: 'High DPI PNG Spritesheet',
      url: buildMartinUrl(`/sprite/${name}@2x.png`),
    },
  ];

  const sdfFormats: SpriteFormat[] = [
    {
      description: 'For runtime coloring with single color',
      label: 'SDF Spritesheet',
      primary: true,
      url: buildMartinUrl(`/sdf_sprite/${name}.png`),
    },
    {
      description: 'SDF sprite coordinates and metadata',
      label: 'SDF JSON',
      url: buildMartinUrl(`/sdf_sprite/${name}.json`),
    },
    {
      description: 'High resolution sprites for retina displays',
      label: 'High DPI SDF Spritesheet',
      url: buildMartinUrl(`/sdf_sprite/${name}@2x.png`),
    },
  ];

  const renderFormats = (formats: SpriteFormat[]) => (
    <ul className="space-y-2">
      {formats.map((format) => (
        <li
          className="flex flex-col gap-2 rounded-lg border bg-card p-3 transition-colors hover:bg-accent/50 sm:flex-row sm:items-center sm:justify-between sm:gap-4"
          key={format.url}
        >
          <div className="min-w-0 flex-1">
            <div className="text-sm font-medium">{format.label}</div>
            <p className="hidden text-sm text-muted-foreground sm:block">{format.description}</p>
          </div>
          <div className="flex shrink-0 gap-2">
            <Button
              asChild
              className="flex-1 sm:flex-none"
              size="sm"
              variant={format.primary ? 'default' : 'secondary'}
            >
              <a
                aria-label={`Download ${format.label}`}
                download={format.url.split('/').pop()}
                href={format.url}
              >
                <Download />
                Download
              </a>
            </Button>
            <Button
              aria-label={`Copy URL of ${format.label}`}
              className="flex-1 sm:flex-none"
              onClick={() => copy(format.url, `URL of ${format.label} copied to clipboard`)}
              size="sm"
              variant="outline"
            >
              {copiedValue === format.url ? (
                <>
                  <CopyCheck className="text-success" />
                  Copied
                </>
              ) : (
                <>
                  <Copy />
                  Copy URL
                </>
              )}
            </Button>
          </div>
        </li>
      ))}
    </ul>
  );

  return (
    <Dialog onOpenChange={(v: boolean) => !v && onCloseAction()} open={true}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-xl">
            Download <code className="font-mono text-lg">{name}</code>
          </DialogTitle>
          <DialogDescription>
            Download the sprite sheet and its JSON index, or copy a URL to use in a style.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-6">
          <details className="group rounded-lg border bg-sunken/60">
            <summary className="flex min-h-11 cursor-pointer list-none items-center justify-between gap-2 rounded-lg px-4 text-sm font-medium [&::-webkit-details-marker]:hidden">
              PNG or SDF? Compare the two formats
              <ChevronDown
                aria-hidden="true"
                className="size-4 transition-transform group-open:rotate-180"
              />
            </summary>
            <div className="grid grid-cols-1 gap-3 p-3 pt-0 md:grid-cols-2">
              {/* PNG Format */}
              <div className="rounded-lg border border-t-4 border-t-brand/60 bg-card p-4">
                <div className="mb-2 flex items-center gap-2">
                  <Badge variant="secondary">PNG</Badge>
                  <h4 className="font-semibold">Standard Format</h4>
                </div>
                <p className="mb-3 text-sm text-muted-foreground">
                  Standard sprite format with multiple colors and transparency.
                </p>
                <ul className="list-disc space-y-1.5 pl-5 text-sm marker:text-brand">
                  <li>Full color support</li>
                  <li>No runtime recoloring</li>
                  <li>Compatible with all mapping libraries</li>
                  <li>Fixed resolution</li>
                </ul>
              </div>
  
              {/* SDF Format */}
              <div className="rounded-lg border border-t-4 border-t-primary bg-card p-4">
                <div className="mb-2 flex items-center gap-2">
                  <Badge variant="default">SDF</Badge>
                  <h4 className="font-semibold">Signed Distance Field</h4>
                </div>
                <p className="mb-3 text-sm text-muted-foreground">For dynamic coloring at runtime.</p>
                <ul className="list-disc space-y-1.5 pl-5 text-sm marker:text-brand">
                  <li>One color per sprite. Layer several SDFs for multi-color icons.</li>
                  <li>
                    Customizable color via{' '}
                    <code className="rounded-sm bg-muted px-1 py-0.5 text-xs font-semibold">
                      icon-color
                    </code>{' '}
                    property
                  </li>
                  <li>Supported by MapLibre and Mapbox</li>
                  <li>
                    <a
                      className="inline-flex items-center gap-0.5 font-medium text-brand underline underline-offset-2"
                      href="https://steamcdn-a.akamaihd.net/apps/valve/2007/SIGGRAPH2007_AlphaTestedMagnification.pdf"
                      rel="noopener noreferrer"
                      target="_blank"
                    >
                      Sharp
                      <ExternalLink aria-hidden="true" className="size-3" />
                    </a>{' '}
                    at any zoom level (how SDF works)
                  </li>
                </ul>
              </div>
            </div>
          </details>

          {/* Download Options */}
          <div className="space-y-6">
            <div>
              <h4 className="mb-3 flex items-center gap-2 font-semibold">
                <Badge variant="secondary">PNG</Badge>
                Standard Sprites
              </h4>
              {renderFormats(pngFormats)}
            </div>

            <div>
              <h4 className="mb-3 flex items-center gap-2 font-semibold">
                <Badge variant="default">SDF</Badge>
                Runtime Colorable Sprites
              </h4>
              {renderFormats(sdfFormats)}
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
