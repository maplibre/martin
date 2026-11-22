import type { SourceSpecification, StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { createContext, type ReactNode, useContext, useEffect, useState } from 'react';
import { buildMartinUrl } from '@/lib/api';

/** Style-level resources the icon and font pickers browse. */
export interface StyleResources {
  /** The style's `sprite`: one base URL or a list of `{id, url}` sheets (non-default ids prefix icon names as `id:name`). */
  sprite: StyleSpecification['sprite'];
  /** The style's `glyphs` URL template with `{fontstack}` and `{range}`. */
  glyphs: string | undefined;
  /** Every font name used by any `text-font` in the style, most used first. */
  fontsInStyle: string[];
  /** The style's sources by id. */
  sources: Record<string, SourceSpecification>;
  /** Source layers of a vector source, from its TileJSON and loaded tiles. */
  sourceLayers: (source: string) => string[];
  /** Tile sources served by this Martin that can be added to the style. */
  martinSources: MartinTileSource[];
  /** Font names served by this Martin. */
  martinFonts: string[];
  /** Sprite ids served by this Martin. */
  martinSprites: string[];
}

/** A tile source from Martin's catalog, ready to become a style source. */
export interface MartinTileSource {
  /** Catalog id, also the suggested style source id. */
  id: string;
  /** Display name from the catalog, falling back to the id. */
  name: string;
  /** Description from the catalog. */
  description?: string;
  /** Style source definition pointing at Martin's TileJSON for this source. */
  spec: SourceSpecification;
}

type CatalogTiles = Record<
  string,
  { content_type: string; name?: string | null; description?: string | null }
>;

/** What this Martin serves that a style can use. */
export interface MartinCatalog {
  /** Tile sources, ready to become style sources. */
  tiles: MartinTileSource[];
  /** Font names. */
  fonts: string[];
  /** Sprite ids. */
  sprites: string[];
}

/** The `glyphs` URL template that loads fonts from this Martin. */
export function martinGlyphsUrl(): string {
  return `${buildMartinUrl('/font')}/{fontstack}/{range}`;
}

/** The `sprite` URL for one of this Martin's sprites; `sdf` selects the recolorable variant. */
export function martinSpriteUrl(id: string, sdf = false): string {
  return buildMartinUrl(`/${sdf ? 'sdf_sprite' : 'sprite'}/${id}`);
}

/** Fetches Martin's catalog once and maps its tile sources, fonts and sprites to what a style can reference. */
export function useMartinCatalog(): MartinCatalog {
  const [catalog, setCatalog] = useState<MartinCatalog>({ fonts: [], sprites: [], tiles: [] });
  useEffect(() => {
    const controller = new AbortController();
    fetch(buildMartinUrl('/catalog'), { signal: controller.signal })
      .then((r) => (r.ok ? r.json() : undefined))
      .then((c?: { tiles?: CatalogTiles; fonts?: object; sprites?: object }) => {
        const tiles = Object.entries(c?.tiles ?? {}).flatMap(([id, t]): MartinTileSource[] => {
          const url = buildMartinUrl(`/${id}`);
          const vector = /protobuf|mvt|mlt/.test(t.content_type);
          const raster = t.content_type.startsWith('image/');
          if (!vector && !raster) return [];
          const spec: SourceSpecification = vector
            ? { type: 'vector', url }
            : { tileSize: 256, type: 'raster', url };
          return [{ description: t.description ?? undefined, id, name: t.name || id, spec }];
        });
        setCatalog({
          fonts: Object.keys(c?.fonts ?? {}).sort(),
          sprites: Object.keys(c?.sprites ?? {}).sort(),
          tiles: tiles.sort((a, b) => a.name.localeCompare(b.name)),
        });
      })
      .catch(() => undefined);
    return () => controller.abort();
  }, []);
  return catalog;
}

const EMPTY: StyleResources = {
  fontsInStyle: [],
  glyphs: undefined,
  martinFonts: [],
  martinSources: [],
  martinSprites: [],
  sourceLayers: () => [],
  sources: {},
  sprite: undefined,
};

const StyleResourcesContext = createContext<StyleResources>(EMPTY);

/** Makes the style's sprite, glyphs and fonts available to the pickers below it. */
export function StyleResourcesProvider({
  value,
  children,
}: {
  value: Partial<StyleResources>;
  children: ReactNode;
}) {
  return <StyleResourcesContext value={{ ...EMPTY, ...value }}>{children}</StyleResourcesContext>;
}

/** The style's sprite, glyphs and fonts; empty outside the editor. */
export function useStyleResources(): StyleResources {
  return useContext(StyleResourcesContext);
}

/** Font names found in literal `text-font` arrays and in string outputs of `text-font` expressions, most used first. */
export function collectFonts(style: StyleSpecification): string[] {
  const counts = new Map<string, number>();
  const walk = (v: unknown) => {
    if (typeof v === 'string') counts.set(v, (counts.get(v) ?? 0) + 1);
    else if (Array.isArray(v)) {
      for (const x of v) if (typeof x === 'string' || Array.isArray(x)) walk(x);
    }
  };
  for (const layer of style.layers) {
    const fonts = (layer as { layout?: Record<string, unknown> }).layout?.['text-font'];
    if (Array.isArray(fonts)) {
      for (const f of fonts) {
        if (
          typeof f === 'string' &&
          !['literal', 'get', 'match', 'case', 'step', 'coalesce'].includes(f)
        ) {
          counts.set(f, (counts.get(f) ?? 0) + 1);
        } else if (Array.isArray(f)) walk(f);
      }
    }
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([name]) => name);
}
