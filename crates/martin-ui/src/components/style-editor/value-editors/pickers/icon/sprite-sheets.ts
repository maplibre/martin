import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { useEffect, useState } from 'react';

/** One icon of a sprite sheet with everything needed to crop it out of the sheet image. */
export interface SpriteIcon {
  /** Name as referenced from `icon-image`: `id:name` for sheets whose id is not `default`. */
  name: string;
  /** Id of the sheet the icon lives in. */
  sheetId: string;
  /** Absolute URL of the sheet image. */
  imageUrl: string;
  /** Left edge in sheet image pixels. */
  x: number;
  /** Top edge in sheet image pixels. */
  y: number;
  /** Width in sheet image pixels. */
  width: number;
  /** Height in sheet image pixels. */
  height: number;
  /** Device pixels per CSS pixel the icon was drawn for. */
  pixelRatio: number;
  /** True when the icon is a monochrome mask recolored by `icon-color`. */
  sdf: boolean;
  /** Natural size of the sheet image in pixels. */
  sheetWidth: number;
  /** Natural height of the sheet image in pixels. */
  sheetHeight: number;
}

/** Loading state of the style's sprite sheets. */
export interface SpriteIcons {
  /** Whether the sheets are still loading, loaded, or failed. */
  status: 'idle' | 'loading' | 'ready' | 'error';
  /** Every icon of every sheet in sheet order. */
  icons: SpriteIcon[];
  /** Ids of the sheets in order. */
  sheetIds: string[];
  /** Short failure description when `status` is `error`. */
  error: string | undefined;
}

interface IndexEntry {
  x: number;
  y: number;
  width: number;
  height: number;
  pixelRatio?: number;
  sdf?: boolean;
}

interface Sheet {
  id: string;
  imageUrl: string;
  index: Record<string, IndexEntry>;
}

interface Size {
  width: number;
  height: number;
}

const sheetCache = new Map<string, Promise<Sheet>>();
const sizeCache = new Map<string, Promise<Size>>();

/** URL of one sprite file: `suffix` such as `@2x.json` goes on the path of `base`, keeping its query and hash. */
export function spriteFileUrl(base: string, suffix: string): string {
  const url = new URL(base, window.location.href);
  url.pathname += suffix;
  return url.href;
}

async function fetchIndex(url: string): Promise<Record<string, IndexEntry> | undefined> {
  const res = await fetch(url);
  if (!res.ok) return undefined;
  return (await res.json()) as Record<string, IndexEntry>;
}

async function fetchSheet(id: string, base: string): Promise<Sheet> {
  const hd = await fetchIndex(spriteFileUrl(base, '@2x.json')).catch(() => undefined);
  if (hd) return { id, imageUrl: spriteFileUrl(base, '@2x.png'), index: hd };
  const sd = await fetchIndex(spriteFileUrl(base, '.json'));
  if (!sd) throw new Error(`No sprite index at ${spriteFileUrl(base, '.json')}`);
  return { id, imageUrl: spriteFileUrl(base, '.png'), index: sd };
}

function loadSheet(id: string, base: string): Promise<Sheet> {
  const key = `${id}\n${base}`;
  let hit = sheetCache.get(key);
  if (!hit) {
    hit = fetchSheet(id, base);
    sheetCache.set(key, hit);
    hit.catch(() => sheetCache.delete(key));
  }
  return hit;
}

function measure(url: string): Promise<Size> {
  let hit = sizeCache.get(url);
  if (!hit) {
    hit = new Promise<Size>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve({ height: img.naturalHeight, width: img.naturalWidth });
      img.onerror = () => reject(new Error(`Could not load ${url}`));
      img.src = url;
    });
    sizeCache.set(url, hit);
    hit.catch(() => sizeCache.delete(url));
  }
  return hit;
}

/** The sheets declared by a style's `sprite` as `{id, url}` pairs. */
export function spriteSheets(sprite: StyleSpecification['sprite']): { id: string; url: string }[] {
  if (!sprite) return [];
  return typeof sprite === 'string' ? [{ id: 'default', url: sprite }] : sprite;
}

function extent(index: Record<string, IndexEntry>): Size {
  let width = 1;
  let height = 1;
  for (const e of Object.values(index)) {
    width = Math.max(width, e.x + e.width);
    height = Math.max(height, e.y + e.height);
  }
  return { height, width };
}

function toIcons(sheet: Sheet, size: Size): SpriteIcon[] {
  const prefix = sheet.id === 'default' ? '' : `${sheet.id}:`;
  return Object.entries(sheet.index).map(([name, e]) => ({
    height: e.height,
    imageUrl: sheet.imageUrl,
    name: prefix + name,
    pixelRatio: e.pixelRatio ?? 1,
    sdf: e.sdf === true,
    sheetHeight: size.height,
    sheetId: sheet.id,
    sheetWidth: size.width,
    width: e.width,
    x: e.x,
    y: e.y,
  }));
}

interface Loaded {
  key: string;
  sheets: Sheet[] | string;
}

/** Loads the sprite sheets of a style and lists their icons; sheet sizes refine once the images arrive. */
export function useSpriteIcons(sprite: StyleSpecification['sprite']): SpriteIcons {
  const sources = spriteSheets(sprite);
  const key = JSON.stringify(sources);
  const [loaded, setLoaded] = useState<Loaded | undefined>();
  const [sizes, setSizes] = useState<Record<string, Size>>({});

  useEffect(() => {
    let live = true;
    const list = JSON.parse(key) as { id: string; url: string }[];
    Promise.all(list.map((s) => loadSheet(s.id, s.url))).then(
      (sheets) => {
        if (!live) return;
        setLoaded({ key, sheets });
        for (const sheet of sheets) {
          measure(sheet.imageUrl).then(
            (size) => live && setSizes((prev) => ({ ...prev, [sheet.imageUrl]: size })),
            () => undefined,
          );
        }
      },
      (e: unknown) => {
        if (live) setLoaded({ key, sheets: e instanceof Error ? e.message : String(e) });
      },
    );
    return () => {
      live = false;
    };
  }, [key]);

  const sheetIds = sources.map((s) => s.id);
  if (sources.length === 0) return { error: undefined, icons: [], sheetIds, status: 'idle' };
  if (!loaded || loaded.key !== key) {
    return { error: undefined, icons: [], sheetIds, status: 'loading' };
  }
  if (typeof loaded.sheets === 'string') {
    return { error: loaded.sheets, icons: [], sheetIds, status: 'error' };
  }
  const icons = loaded.sheets.flatMap((sheet) =>
    toIcons(sheet, sizes[sheet.imageUrl] ?? extent(sheet.index)),
  );
  return { error: undefined, icons, sheetIds, status: 'ready' };
}
