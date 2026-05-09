import { Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { SpriteCollection } from '@/lib/types';
import { SpriteMapPreview } from '../sprite/SpriteMapPreview';

interface SpritePreviewDialogProps {
  name: string;
  sprite: SpriteCollection;
  onCloseAction: () => void;
  onDownloadAction: (sprite: SpriteCollection) => void;
}

export function SpritePreviewDialog({
  name,
  sprite,
  onDownloadAction,
  onCloseAction,
}: SpritePreviewDialogProps) {
  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
      <DialogContent className="max-w-4xl w-full p-6 max-h-[80vh] overflow-auto">
        {sprite && (
          <>
            <DialogHeader className="mb-4 truncate">
              <DialogTitle className="text-2xl flex gap-4">{name}</DialogTitle>
              <DialogDescription>
                <span>
                  Preview sprites as rendered by MapLibre GL. Toggle between PNG and SDF modes.
                </span>
                <br />
                <Button onClick={() => onDownloadAction(sprite)} size="sm" variant="outline">
                  <Download className="h-4 w-4 mr-2" />
                  Download
                </Button>
              </DialogDescription>
            </DialogHeader>
            <SpriteMapPreview spriteIds={sprite.images} spriteName={name} />
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
