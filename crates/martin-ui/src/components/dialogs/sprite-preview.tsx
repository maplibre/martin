import { Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { Catalog } from '@/lib/types.gen';
import { SpriteMapPreview } from '../sprite/SpriteMapPreview';

interface SpritePreviewDialogProps {
  name: string;
  sprite: Catalog['sprites'][string];
  onCloseAction: () => void;
  onDownloadAction: (sprite: Catalog['sprites'][string]) => void;
}

export function SpritePreviewDialog({
  name,
  sprite,
  onDownloadAction,
  onCloseAction,
}: SpritePreviewDialogProps) {
  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
      <DialogContent className="sm:max-w-4xl">
        {sprite ? (
          <>
            <DialogHeader>
              <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 pr-8">
                <DialogTitle className="min-w-0 font-mono text-lg font-medium">{name}</DialogTitle>
                <Button
                  aria-label="Download sprite"
                  className="shrink-0 max-sm:size-11 max-sm:px-0"
                  onClick={() => onDownloadAction(sprite)}
                  size="sm"
                  variant="outline"
                >
                  <Download />
                  <span className="max-sm:sr-only">Download</span>
                </Button>
                <DialogDescription className="order-last basis-full">
                  Preview sprites as rendered by MapLibre GL. Toggle between PNG and SDF modes.
                </DialogDescription>
              </div>
            </DialogHeader>
            <SpriteMapPreview spriteIds={sprite.images} spriteName={name} />
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
