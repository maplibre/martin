import React, { useEffect, useState } from "react";
import { fetchSpriteImage, fetchSpriteIndex, SpriteMeta, SpriteIndex } from "./SpriteCache";
import SpriteCanvas from "./SpriteCanvas";
import { cn } from "@/lib/utils";

type SpritePreviewProps = {
  /**
   * Base URL for the sprite (without .json/.png or @2x).
   * Example: "https://example.com/sprite"
   */
  spriteUrl: string;
  /**
   * List of sprite IDs to display, in order. If not provided, all from the index are shown.
   */
  spriteIds: string[];
  /**
   * If true, only display the first PREVIEW_LIMIT sprites and use smaller icon size (for catalog previews).
   */
  previewMode?: boolean;
  /**
   * Optional className for the container.
   */
  className?: string;

};

type SpriteState =
  | { status: "loading" }
  | { status: "error"; error: string }
  | { status: "ready"; sprites: [string, SpriteMeta][]; image: HTMLImageElement };

export const SpritePreview: React.FC<SpritePreviewProps> = ({
  spriteUrl,
  spriteIds,
  previewMode,
  className,
}) => {
  const PREVIEW_LIMIT = 18;
  const [state, setState] = useState<SpriteState>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setState({ status: "loading" });

      try {
        // we always use @2x high-DPI assets since we display them a little larger than one would on a map
        const [index, image] = await Promise.all([
          fetchSpriteIndex(`${spriteUrl}@2x.json`),
          fetchSpriteImage(`${spriteUrl}@2x.png`),
        ]);
        if (cancelled) return;

        let sprites = Object.entries(index);

        setState({
          status: "ready",
          sprites,
          image,
        });
      } catch (err: any) {
        if (cancelled) return;
        setState({
          status: "error",
          error: err?.message || "Failed to load sprite",
        });
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [spriteUrl]);

  // --- Determine which sprites to display ---
  let ids = spriteIds;
  if (previewMode && spriteIds.length > PREVIEW_LIMIT) {
    ids = ids.slice(0, PREVIEW_LIMIT - 1);
  }

  // --- Main grid of sprites ---
  if (state.status === "error") {
    return (
      <div className={`text-red-600 text-center p-4 ${className ?? ""}`}>
        <span>Error: {state.error}</span>
      </div>
    );
  }

  if (state.status === "ready" && state.sprites.length === 0) {
    return (
      <div className={`text-center p-4 ${className ?? ""}`}>
        <span>No sprites found.</span>
      </div>
    );
  }

  // Build a metaMap for fast lookup if ready
  const metaMap: Record<string, SpriteMeta> =
    state.status === "ready"
      ? Object.fromEntries(state.sprites)
      : {};

  return (
    <div
      className={cn(`flex flex-wrap gap-3 justify-start items-start min-h-[120px]`, className)}
    >
      {ids.map((id) =>
        <SpriteCanvas
          key={id}
          meta={metaMap[id]}
          image={state.status === "ready" ? state.image : undefined}
          label={id}
          previewMode={previewMode}
        />
      )}

      {/* +N bubble */}
      {previewMode && spriteIds.length > PREVIEW_LIMIT && (
        <div
          data-spritecnt = {spriteIds.length}
          className={`
            ${previewMode ? "h-7" : "h-12"}
            flex items-center justify-center
            ${previewMode ? "text-sm" : "text-lg"}
            text-gray-600 font-semibold
            ${previewMode ? "m-1.5" : "m-4"}
          `}
        >
          +{spriteIds.length - PREVIEW_LIMIT + 1}
        </div>
      )}
    </div>
  );
};

export default SpritePreview;
