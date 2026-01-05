import { Copy } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useToast } from '@/hooks/use-toast';
import { copyToClipboard } from '@/lib/utils';
import type { SpriteMeta } from './SpriteCache';

type SpriteCanvasProps = {
  meta?: SpriteMeta;
  image?: HTMLImageElement;
  label: string;
  previewMode?: boolean;
};

const SpriteCanvas = ({ meta, image, label, previewMode = false }: SpriteCanvasProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const { toast } = useToast();

  const handleClick = async () => {
    try {
      await copyToClipboard(label);
      toast({
        description: `Sprite ID "${label}" copied to clipboard`,
        title: 'Copied!',
      });
    } catch (err) {
      console.error('Failed to copy sprite ID:', err);
      toast({
        description: 'Failed to copy sprite ID to clipboard',
        title: 'Error',
        variant: 'destructive',
      });
    }
  };

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
      <div className="flex flex-col items-center justify-center m-1.5 h-7 w-7">
        {!meta || !image ? (
          <div className="w-7 h-7 animate-pulse bg-purple-200 rounded-sm flex items-center justify-center"></div>
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <canvas
                aria-label={`Icon for ${label}`}
                className="h-7 w-7 object-contain block cursor-pointer hover:opacity-75 transition-opacity"
                height={meta.height}
                onClick={handleClick}
                ref={canvasRef}
                width={meta.width}
              />
            </TooltipTrigger>
            <TooltipContent>
              <p>
                Sprite:{' '}
                <code className="bg-purple-200 font-semibold font-monospace text-purple-950 p-1 m-1 rounded-xs">
                  {label}
                </code>
                <br />
                <div className="pt-4 text-sm flex flex-row justify-center p-0.5">
                  <Copy className="h-3 w-3 mr-2" /> Click to copy
                </div>
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
          className="flex flex-col items-center justify-center m-4 h-32 w-24"
          onClick={handleClick}
          type="button"
        >
          <div className="flex flex-1 items-center justify-center w-full">
            {!meta || !image ? (
              <div className="w-24 h-24 animate-pulse bg-purple-200 rounded-sm flex items-center justify-center cursor-pointer hover:bg-purple-300 transition-colors"></div>
            ) : (
              <div className="flex items-center justify-center h-20 w-20">
                <canvas
                  aria-label={`Icon for ${label}`}
                  className="h-20 w-20 object-contain block cursor-pointer hover:opacity-75 transition-opacity"
                  height={meta.height}
                  onClick={handleClick}
                  ref={canvasRef}
                  width={meta.width}
                />
              </div>
            )}
          </div>
          <code className="text-monospace text-sm text-gray-700 break-all text-center mt-2 cursor-pointer hover:text-gray-900 transition-colors">
            {label}
          </code>
        </button>
      </TooltipTrigger>
      <TooltipContent>
        <div className="text-xs flex flex-row p-0.5">
          <Copy className="h-3 w-3 mr-2" /> Click to copy
        </div>
      </TooltipContent>
    </Tooltip>
  );
};

export default SpriteCanvas;
