import { Copy, CopyCheck } from 'lucide-react';

import { useState } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { TooltipCopyText } from '@/components/ui/tooltip-copy-text';
import { useToast } from '@/hooks/use-toast';
import { buildMartinUrl } from '@/lib/api';
import type { SpriteCollection } from '@/lib/types';

interface SpriteDownloadDialogProps {
  name: string;
  sprite: SpriteCollection;
  onCloseAction: () => void;
}

interface SpriteFormat {
  label: string;
  url: string;
  description: string;
}

export function SpriteDownloadDialog({ name, sprite, onCloseAction }: SpriteDownloadDialogProps) {
  const [copiedUrl, setCopiedUrl] = useState<string | null>(null);
  const { toast } = useToast();
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

  const handleCopyUrl = async (url: string, label: string) => {
    try {
      await navigator.clipboard.writeText(url);

      setCopiedUrl(url);
      toast({
        description: `URL of ${label} copied to clipboard`,
        title: 'URL Copied',
      });

      // Reset copied state after 2 seconds
      setTimeout(() => {
        setCopiedUrl(null);
      }, 2000);
    } catch {
      toast({
        description: 'Failed to copy URL to clipboard',
        title: 'Copy Failed',
        variant: 'destructive',
      });
    }
  };

  return (
    <Dialog onOpenChange={(v: boolean) => !v && onCloseAction()} open={true}>
      <DialogContent className="max-w-2xl w-full max-h-[90vh] overflow-auto">
        <DialogHeader className="truncate">
          <DialogTitle className="text-2xl">
            Download{' '}
            <code className="font-mono">
              <TooltipCopyText side="bottom" text={name} />
            </code>
          </DialogTitle>
          <DialogDescription>
            Download the sprite in various formats or copy the download URL.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-6">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            {/* PNG Format */}
            <div className="p-4 border rounded-lg bg-blue-50 border-blue-200">
              <div className="flex items-center mb-3">
                <Badge className="bg-blue-100 text-blue-800 mr-2" variant="secondary">
                  PNG
                </Badge>
                <h4 className="font-semibold text-blue-900">Standard Format</h4>
              </div>
              <p className="text-sm text-blue-800 mb-4">
                Standard sprite format with multiple colors and transparency.
              </p>
              <ul className="text-xs text-blue-700 my-6 ml-6 list-disc [&>li]:mt-2">
                <li>Full color support</li>
                <li>No runtime recoloring</li>
                <li>Compatible with all mapping libraries</li>
                <li>Fixed resolution</li>
              </ul>
            </div>

            {/* SDF Format */}
            <div className="p-4 border rounded-lg bg-purple-50 border-purple-200">
              <div className="flex items-center mb-3">
                <Badge className="bg-purple-100 text-purple-800 mr-2" variant="secondary">
                  SDF
                </Badge>
                <h4 className="font-semibold text-purple-900">Signed Distance Field</h4>
              </div>
              <p className="text-sm text-purple-800 mb-4">For dynamic coloring at runtime.</p>
              <ul className="text-xs text-purple-700  my-6 ml-6 list-disc [&>li]:mt-2">
                <li>Single color per sprite - Layer multiple SDFs for multi-color icons</li>
                <li>
                  Customizable color via{' '}
                  <code className="bg-purple-200 font-semibold font-monospace text-purple-950 p-0.5 rounded-xs">
                    icon-color
                  </code>{' '}
                  property
                </li>
                <li>Supported by MapLibre and Mapbox</li>
                <li>
                  <a
                    className="text-purple-950 underline hover:text-purple-900"
                    href="https://steamcdn-a.akamaihd.net/apps/valve/2007/SIGGRAPH2007_AlphaTestedMagnification.pdf"
                    rel="noopener noreferrer"
                    target="_blank"
                  >
                    SVG-Like
                  </a>{' '}
                  zooming
                </li>
              </ul>
            </div>
          </div>

          {/* Download Options */}
          <div className="space-y-6">
            {/* PNG Downloads */}
            <div>
              <h4 className="font-semibold mb-3 text-blue-900 flex items-center">
                <Badge className="bg-blue-100 text-blue-800 mr-2" variant="secondary">
                  PNG
                </Badge>
                Standard Sprites
              </h4>
              <div className="space-y-3">
                {pngFormats.map((format) => (
                  <div
                    className="flex items-center justify-between p-3 border rounded-lg hover:bg-gray-50"
                    key={format.url}
                  >
                    <div className="flex-1">
                      <div className="flex items-center mb-1 font-medium">{format.label}</div>
                      <p className="text-sm text-muted-foreground">{format.description}</p>
                    </div>
                    <Button
                      className="ml-4"
                      onClick={() => handleCopyUrl(format.url, format.label)}
                      size="sm"
                      variant="outline"
                    >
                      {copiedUrl === format.url ? (
                        <>
                          <CopyCheck className="h-4 w-4 mr-2 text-green-600" />
                          Copied
                        </>
                      ) : (
                        <>
                          <Copy className="h-4 w-4 mr-2" />
                          Copy URL
                        </>
                      )}
                    </Button>
                  </div>
                ))}
              </div>
            </div>

            {/* SDF Downloads */}
            <div>
              <h4 className="font-semibold mb-3 text-purple-900 flex items-center">
                <Badge className="bg-purple-100 text-purple-800 mr-2" variant="secondary">
                  SDF
                </Badge>
                Runtime Colorable Sprites
              </h4>
              <div className="space-y-3">
                {sdfFormats.map((format) => (
                  <div
                    className="flex items-center justify-between p-3 border rounded-lg hover:bg-gray-50"
                    key={format.url}
                  >
                    <div className="flex-1">
                      <div className="flex items-center mb-1 font-medium">{format.label}</div>
                      <p className="text-sm text-muted-foreground">{format.description}</p>
                    </div>
                    <Button
                      className="ml-4"
                      onClick={() => handleCopyUrl(format.url, format.label)}
                      size="sm"
                      variant="outline"
                    >
                      {copiedUrl === format.url ? (
                        <>
                          <CopyCheck className="h-4 w-4 mr-2 text-green-600" />
                          Copied
                        </>
                      ) : (
                        <>
                          <Copy className="h-4 w-4 mr-2" />
                          Copy URL
                        </>
                      )}
                    </Button>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
