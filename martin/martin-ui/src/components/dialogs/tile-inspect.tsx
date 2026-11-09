'use client';
import { type ReactNode, Suspense } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { Catalog } from '@/lib/types.gen';
import '@maplibre/maplibre-gl-inspect/dist/maplibre-gl-inspect.css';
import { Database, ExternalLink } from 'lucide-react';
import { CopyableUrl } from '@/components/ui/copyable-url';
import { buildMartinUrl } from '@/lib/api';
import { LoadingSpinner } from '../loading/loading-spinner';
import { TileInspectDialogMap } from './tile-inspect-map';

interface TileInspectDialogProps {
  name: string;
  source: Catalog['tiles'][string];
  onCloseAction: () => void;
}

function TileMapLoading() {
  return (
    <div className="tile-grid flex h-72 w-full items-center justify-center text-muted-foreground">
      <LoadingSpinner />
    </div>
  );
}

function InfoRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-1 gap-0.5 py-2 sm:grid-cols-[8rem_1fr] sm:gap-4">
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd className="min-w-0 text-sm [overflow-wrap:anywhere]">{children}</dd>
    </div>
  );
}

export function TileInspectDialog({ name, source, onCloseAction }: TileInspectDialogProps) {
  const tileJsonUrl = buildMartinUrl(`/${name}`);
  const xyzUrl = buildMartinUrl(`/${name}/{z}/{x}/{y}`);

  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
      <DialogContent className="sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle className="text-lg">
            <span className="block text-sm font-medium text-muted-foreground sm:inline sm:text-lg sm:font-semibold sm:text-foreground">
              Inspect Tile Source:
            </span>{' '}
            <code className="font-mono text-base font-medium [overflow-wrap:anywhere] sm:text-lg">
              {name}
            </code>
          </DialogTitle>
          <DialogDescription>
            Inspect the tile source to explore tile boundaries and properties.
          </DialogDescription>
        </DialogHeader>
        <section
          aria-label="Tile map"
          className="shrink-0 overflow-hidden rounded-lg border bg-sunken"
        >
          <Suspense fallback={<TileMapLoading />}>
            <TileInspectDialogMap name={name} source={source} />
          </Suspense>
        </section>
        <div className="grid min-w-0 gap-4 md:grid-cols-2">
          <section className="min-w-0 rounded-lg border bg-card p-4">
            <h3 className="mb-1 flex items-center gap-2 text-sm font-semibold">
              <Database aria-hidden="true" className="size-4 text-brand" />
              Source Information
            </h3>
            <dl className="divide-y">
              <InfoRow label="Content Type">
                <code className="text-xs">{source.content_type}</code>
              </InfoRow>
              {source.content_encoding ? (
                <InfoRow label="Encoding">
                  <code className="text-xs">{source.content_encoding}</code>
                </InfoRow>
              ) : null}
              {source.name ? <InfoRow label="Name">{source.name}</InfoRow> : null}
              {source.description && source.description !== source.name ? (
                <InfoRow label="Description">{source.description}</InfoRow>
              ) : null}
              {source.layer_count ? (
                <InfoRow label="Layer Count">{source.layer_count}</InfoRow>
              ) : null}
              {source.attribution ? (
                <InfoRow label="Attribution">{source.attribution}</InfoRow>
              ) : null}
            </dl>
          </section>

          <section className="min-w-0 rounded-lg border bg-card p-4">
            <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold">
              <ExternalLink aria-hidden="true" className="size-4 text-brand" />
              Tile URLs
            </h3>
            <div className="flex flex-col gap-4">
              <CopyableUrl label="TileJSON" url={tileJsonUrl} />
              <CopyableUrl label="XYZ Tiles" url={xyzUrl} />
            </div>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
