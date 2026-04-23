'use client';
import { Suspense, useState, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { TileSource } from '@/lib/types';
import '@maplibre/maplibre-gl-inspect/dist/maplibre-gl-inspect.css';
import { buildMartinUrl } from '@/lib/api';
import { Database, Link, Copy, Check } from 'lucide-react';
import { LoadingSpinner } from '../loading/loading-spinner';
import { TileInspectDialogMap } from './tile-inspect-map';

interface TileInspectDialogProps {
  name: string;
  source: TileSource;
  onCloseAction: () => void;
}

function TileMapLoading() {
  return (
    <div className="flex justify-center items-center text-white text-3xl w-full h-125">
      <LoadingSpinner />
    </div>
  );
}

function CopyableUrl({ label, url }: { label: string; url: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    await navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [url]);

  return (
    <p>
      <span className="font-medium">{label}:</span>
      <br />
      <span className="flex items-center gap-2 mt-1">
        <code className="text-xs break-all flex-1">{url}</code>
        <button
          onClick={handleCopy}
          className="shrink-0 p-1 rounded hover:bg-muted transition-colors text-muted-foreground hover:text-foreground cursor-pointer"
          title={`Copy ${label}`}
        >
          {copied ? <Check className="w-3.5 h-3.5 text-green-500" /> : <Copy className="w-3.5 h-3.5" />}
        </button>
      </span>
    </p>
  );
}

export function TileInspectDialog({ name, source, onCloseAction }: TileInspectDialogProps) {
  const tileJsonUrl = buildMartinUrl(`/${name}`);
  const xyzUrl = buildMartinUrl(`/${name}/{z}/{x}/{y}`);

  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
      <DialogContent className="max-w-6xl w-full p-6 max-h-[90vh] overflow-auto">
        <DialogHeader className="mb-6 truncate">
          <DialogTitle className="text-2xl flex items-center justify-between">
            <span>
              Inspect Tile Source: <code>{name}</code>
            </span>
          </DialogTitle>
          <DialogDescription>
            Inspect the tile source to explore tile boundaries and properties.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <section className="border rounded-lg overflow-hidden">
            <Suspense fallback={<TileMapLoading />}>
              <TileInspectDialogMap name={name} source={source} />
            </Suspense>
          </section>
          {/* Source Information */}
          <section className="bg-muted/30 p-4 rounded-lg">
            <div className="flex items-center gap-2 mb-2">
              <Database className="w-5 h-5 text-muted-foreground" />
              <h3 className="font-semibold">Source Information</h3>
            </div>
            <div className="flex flex-col md:grid md:grid-cols-2 gap-y-4 text-sm">
              <p>
                <span className="font-medium">Content Type:</span>
                <br />
                <code>{source.content_type}</code>
              </p>
              {source.content_encoding && (
                <p>
                  <span className="font-medium">Encoding:</span>
                  <br />
                  <code>{source.content_encoding}</code>
                </p>
              )}
              {source.name && (
                <p>
                  <span className="font-medium">Name:</span>
                  <br />
                  <span>{source.name}</span>
                </p>
              )}
              {source.description && (
                <p className="col-span-2">
                  <span className="font-medium">Description:</span>
                  <br />
                  <span>{source.description}</span>
                </p>
              )}
              {source.layerCount && (
                <p>
                  <span className="font-medium">Layer Count:</span>
                  <br />
                  <span>{source.layerCount}</span>
                </p>
              )}
              {source.attribution && (
                <p className="col-span-2">
                  <span className="font-medium">Attribution:</span>
                  <br />
                  <span>{source.attribution}</span>
                </p>
              )}
            </div>
          </section>

          {/* Tile URLs */}
          <section className="bg-muted/30 p-4 rounded-lg">
            <div className="flex items-center gap-2 mb-2">
              <Link className="w-5 h-5 text-muted-foreground" />
              <h3 className="font-semibold">Tile URLs</h3>
            </div>
            <div className="flex flex-col gap-y-4 text-sm">
              <CopyableUrl label="TileJSON" url={tileJsonUrl} />
              <CopyableUrl label="XYZ Tiles" url={xyzUrl} />
            </div>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
