import { Copy } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import type { SpriteMeta } from './SpriteCache';

type SpriteCanvasProps = {
  meta?: SpriteMeta;
  image?: HTMLImageElement;
  label: string;
  previewMode?: boolean;
};

const SpriteCanvas = ({ meta, image, label, previewMode = false }: SpriteCanvasProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  // not using copied since on-click the tooltip closes
  const { copy } = useCopyToClipboard({
    successMessage: `Sprite ID "${label}" copied to clipboard`,
  });

  const handleClick = () => copy(label);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !meta || !image) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    // Clear
    ctx.clearRect(0, 0, meta.width, meta.height);
    // Draw the sprite sub-image
    ctx.drawImage(image, meta.x, meta.y, meta.width, meta.height, 0, 0, meta.width, meta.height);
  }, [meta, image]);

  if (previewMode)
    return (
      <div className="flex size-11 flex-col items-center justify-center">
        {!meta || !image ? (
          <div className="size-9 animate-pulse rounded-sm bg-paper-foreground/15"></div>
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <canvas
                aria-label={`Icon for ${label}`}
                className="block size-9 cursor-pointer object-contain transition-opacity hover:opacity-75"
                height={meta.height}
                onClick={handleClick}
                ref={canvasRef}
                width={meta.width}
              />
            </TooltipTrigger>
            <TooltipContent>
              <p>
                <code className="font-mono font-semibold [overflow-wrap:anywhere]">{label}</code>
              </p>
              <p className="mt-1 flex items-center gap-1 text-xs text-muted-foreground">
                <Copy className="size-3" /> Click to copy
              </p>
            </TooltipContent>
          </Tooltip>
        )}
      </div>
    );

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          className="flex w-full min-w-0 cursor-pointer flex-col items-center gap-2 rounded-lg p-2 transition-colors hover:bg-black/5"
          onClick={handleClick}
          type="button"
        >
          <div className="flex size-16 items-center justify-center">
            {!meta || !image ? (
              <div className="size-14 animate-pulse rounded-sm bg-paper-foreground/15"></div>
            ) : (
              <canvas
                aria-label={`Icon for ${label}`}
                className="block size-14 object-contain"
                height={meta.height}
                ref={canvasRef}
                width={meta.width}
              />
            )}
          </div>
          <code className="w-full text-center font-mono text-xs leading-tight text-paper-foreground [overflow-wrap:anywhere]">
            {label}
          </code>
        </button>
      </TooltipTrigger>
      <TooltipContent>
        <div className="text-xs flex flex-row gap-1 p-0.5">
          <Copy className="h-3 w-3 mb-0.5" /> Click to copy
        </div>
      </TooltipContent>
    </Tooltip>
  );
};

export default SpriteCanvas;
