import { Download } from 'lucide-react';
import { Suspense } from 'react';
import { LoadingSpinner } from '@/components/loading/loading-spinner';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { SpriteCollection } from '@/lib/types';
import { SpritePreview } from '../sprite/SpritePreview';

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
          <div>
            <DialogHeader className="mb-6">
              <DialogTitle className="text-2xl flex gap-4">
                {name}
                <Button onClick={() => onDownloadAction(sprite)} size="sm" variant="outline">
                  <Download className="h-4 w-4 mr-2" />
                  Download
                </Button>
              </DialogTitle>
              <DialogDescription>Preview the selected sprite.</DialogDescription>
            </DialogHeader>
            <div className="pace-y-4 bg-gray-50 rounded-lg text-gray-900">
              <Suspense
                fallback={
                  <div className="flex justify-center py-12">
                    <LoadingSpinner size="lg" />
                  </div>
                }
              >
                <SpritePreview
                  className="w-full grid grid-cols-2 sm:grid-cols-4 md:grid-cols-6 gap-4"
                  spriteIds={sprite.images}
                  spriteUrl={`/sprite/${name}`}
                />
              </Suspense>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
