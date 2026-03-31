import { Download } from 'lucide-react';
import { Suspense, useId, useState } from 'react';
import { LoadingSpinner } from '@/components/loading/loading-spinner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Switch } from '@/components/ui/switch';
import type { SpriteCollection } from '@/lib/types';
import { cn } from '@/lib/utils';
import { SpritePreview } from '../sprite/SpritePreview';

interface SpritePreviewDialogProps {
  name: string;
  sprite: SpriteCollection;
  onCloseAction: () => void;
  onDownloadAction: (sprite: SpriteCollection) => void;
}

const SIZE_MIN = 16;
const SIZE_MAX = 128;
const SIZE_DEFAULT = 80;

const SDF_SCALE_MIN = 0.5;
const SDF_SCALE_MAX = 5;
const SDF_SCALE_DEFAULT = 2;

const HALO_DEFAULT = 0;
const HALO_MAX = 10;
const HALO_BLUR_DEFAULT = 0;
const HALO_BLUR_MAX = 10;

export function SpritePreviewDialog({
  name,
  sprite,
  onDownloadAction,
  onCloseAction,
}: SpritePreviewDialogProps) {
  const uid = useId();
  const iconColorId = `${uid}-icon-color`;
  const haloColorId = `${uid}-halo-color`;

  const [sdfMode, setSdfMode] = useState(false);
  const [displaySize, setDisplaySize] = useState(SIZE_DEFAULT);
  const [sdfScale, setSdfScale] = useState(SDF_SCALE_DEFAULT);
  const [iconColor, setIconColor] = useState('#1a1a2e');
  const [haloColor, setHaloColor] = useState('#ffffff');
  const [haloWidth, setHaloWidth] = useState(HALO_DEFAULT);
  const [haloBlur, setHaloBlur] = useState(HALO_BLUR_DEFAULT);

  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
      <DialogContent className="max-w-4xl w-full p-6 max-h-[80vh] overflow-auto">
        {sprite && (
          <>
            <DialogHeader className="mb-4 truncate">
              <DialogTitle className="text-2xl flex gap-4">{name}</DialogTitle>
              <DialogDescription>
                <span>Preview the selected sprite.</span>
                <br />
                <Button onClick={() => onDownloadAction(sprite)} size="sm" variant="outline">
                  <Download className="h-4 w-4 mr-2" />
                  Download
                </Button>
              </DialogDescription>
            </DialogHeader>

            {/* Toolbar */}
            <div className="flex flex-nowrap items-center gap-4 mb-4 p-3 rounded-lg border bg-muted/40 overflow-x-auto">
              <div className="flex shrink-0 items-center gap-2">
                {!sdfMode ? (
                  <Badge
                    className="border-transparent bg-blue-100 text-blue-800 dark:bg-blue-950 dark:text-blue-200"
                    variant="secondary"
                  >
                    PNG
                  </Badge>
                ) : (
                  <span className="text-sm font-medium text-muted-foreground select-none px-2 py-0.5">
                    PNG
                  </span>
                )}
                <Switch
                  aria-label="Toggle SDF mode"
                  checked={sdfMode}
                  onCheckedChange={setSdfMode}
                />
                {sdfMode ? (
                  <Badge
                    className="border-transparent bg-blue-100 text-blue-800 dark:bg-blue-950 dark:text-blue-200"
                    variant="secondary"
                  >
                    SDF
                  </Badge>
                ) : (
                  <span className="text-sm font-medium text-muted-foreground select-none px-2 py-0.5">
                    SDF
                  </span>
                )}
              </div>

              <div className="h-5 w-px bg-border shrink-0" />

              <div className="flex shrink-0 items-center gap-2 min-w-[160px]">
                {sdfMode ? (
                  <>
                    <span className="text-sm font-medium text-muted-foreground select-none whitespace-nowrap">
                      Scale: {sdfScale.toFixed(1)}×
                    </span>
                    <input
                      aria-label="SDF icon scale"
                      className="w-24 accent-purple-600 cursor-pointer"
                      max={SDF_SCALE_MAX}
                      min={SDF_SCALE_MIN}
                      onChange={(e) => setSdfScale(Number(e.target.value))}
                      step={0.1}
                      type="range"
                      value={sdfScale}
                    />
                  </>
                ) : (
                  <>
                    <span className="text-sm font-medium text-muted-foreground select-none whitespace-nowrap">
                      Size: {displaySize}px
                    </span>
                    <input
                      aria-label="Sprite display size"
                      className="w-24 accent-purple-600 cursor-pointer"
                      max={SIZE_MAX}
                      min={SIZE_MIN}
                      onChange={(e) => setDisplaySize(Number(e.target.value))}
                      step={4}
                      type="range"
                      value={displaySize}
                    />
                  </>
                )}
              </div>

              <div
                aria-hidden={!sdfMode}
                className={cn(
                  'flex shrink-0 items-center gap-4',
                  !sdfMode && 'invisible pointer-events-none select-none',
                )}
              >
                <div className="h-5 w-px bg-border shrink-0" />

                <div className="flex items-center gap-2">
                  <label
                    className="text-sm font-medium text-muted-foreground select-none whitespace-nowrap"
                    htmlFor={iconColorId}
                  >
                    Icon
                  </label>
                  <input
                    className="w-8 h-8 rounded cursor-pointer border border-border p-0.5 bg-transparent"
                    disabled={!sdfMode}
                    id={iconColorId}
                    onChange={(e) => setIconColor(e.target.value)}
                    title="Icon color"
                    type="color"
                    value={iconColor}
                  />
                </div>

                <div className="flex items-center gap-2">
                  <label
                    className="text-sm font-medium text-muted-foreground select-none whitespace-nowrap"
                    htmlFor={haloColorId}
                  >
                    Halo
                  </label>
                  <input
                    className="w-8 h-8 rounded cursor-pointer border border-border p-0.5 bg-transparent"
                    disabled={!sdfMode}
                    id={haloColorId}
                    onChange={(e) => setHaloColor(e.target.value)}
                    title="Halo color"
                    type="color"
                    value={haloColor}
                  />
                </div>

                <div className="flex items-center gap-2 min-w-[150px]">
                  <span className="text-sm font-medium text-muted-foreground select-none whitespace-nowrap">
                    Halo: {haloWidth}px
                  </span>
                  <input
                    aria-label="Halo width"
                    className="w-24 accent-purple-600 cursor-pointer"
                    disabled={!sdfMode}
                    max={HALO_MAX}
                    min={0}
                    onChange={(e) => setHaloWidth(Number(e.target.value))}
                    step={0.5}
                    type="range"
                    value={haloWidth}
                  />
                </div>

                <div className="flex items-center gap-2 min-w-[150px]">
                  <span className="text-sm font-medium text-muted-foreground select-none whitespace-nowrap">
                    Blur: {haloBlur}px
                  </span>
                  <input
                    aria-label="Halo blur"
                    className="w-24 accent-purple-600 cursor-pointer"
                    disabled={!sdfMode}
                    max={HALO_BLUR_MAX}
                    min={0}
                    onChange={(e) => setHaloBlur(Number(e.target.value))}
                    step={0.5}
                    type="range"
                    value={haloBlur}
                  />
                </div>
              </div>
            </div>

            <div className="bg-gray-50 rounded-lg text-gray-900 px-4 pb-5 pt-5 mt-1">
              <Suspense
                fallback={
                  <div className="flex justify-center py-12">
                    <LoadingSpinner size="lg" />
                  </div>
                }
              >
                <SpritePreview
                  className="w-full grid grid-cols-2 sm:grid-cols-4 md:grid-cols-6 gap-4"
                  displaySize={displaySize}
                  haloBlur={haloBlur}
                  haloColor={haloColor}
                  haloWidth={haloWidth}
                  iconColor={iconColor}
                  iconSize={sdfScale}
                  sdfMode={sdfMode}
                  spriteIds={sprite.images}
                  spriteUrl={`/sprite/${name}`}
                />
              </Suspense>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
