import React, { useEffect, useRef } from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { SpriteMeta } from "./SpriteCache";

type SpriteCanvasProps = {
  meta?: SpriteMeta;
  image?: HTMLImageElement;
  label: string;
  previewMode?: boolean;
};

const SpriteCanvas = ({ meta, image, label, previewMode = false }: SpriteCanvasProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !meta || !image) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    // Clear
    ctx.clearRect(0, 0, meta.width, meta.height);
    // Draw the sprite sub-image
    ctx.drawImage(image, meta.x, meta.y, meta.width, meta.height, 0, 0, meta.width, meta.height);
  }, [canvasRef, meta, image]);

  if (previewMode)
    return (
      <div className="flex flex-col items-center justify-center m-1.5 h-7 w-7">
        {!meta || !image ? (
          <div className="w-7 h-7 animate-pulse bg-purple-200 rounded flex items-center justify-center"></div>
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <canvas
                aria-label={"Icon for " + label}
                className="h-7 w-7 object-contain block"
                height={meta.height}
                ref={canvasRef}
                width={meta.width}
              />
            </TooltipTrigger>
            <TooltipContent>
              <p>
                Sprite:{" "}
                <code className="bg-purple-200 font-semibold font-monospace text-purple-950 p-1 rounded-sm">
                  {label}
                </code>
              </p>
            </TooltipContent>
          </Tooltip>
        )}
      </div>
    );

  return (
    <div className="flex flex-col items-center justify-center m-4 h-32 w-24">
      <div className="flex flex-1 items-center justify-center w-full">
        {!meta || !image ? (
          <div className="w-24 h-24 animate-pulse bg-purple-200 rounded flex items-center justify-center"></div>
        ) : (
          <div className="flex items-center justify-center h-20 w-20">
            <canvas
              aria-label={"Icon for " + label}
              className="h-20 w-20 object-contain block"
              height={meta.height}
              ref={canvasRef}
              width={meta.width}
            />
          </div>
        )}
      </div>
      <code className="text-monospace text-gray-700 break-all text-center mt-2">{label}</code>
    </div>
  );
};

export default SpriteCanvas;
