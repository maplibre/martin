// Deduplicated fetching and caching for sprite PNG and JSON index files.

export type SpriteMeta = {
  width: number;
  height: number;
  x: number;
  y: number;
  pixelRatio: number;
};

export type SpriteIndex = Record<string, SpriteMeta>;

// --- PNG Image Cache ---

type SpriteImageCacheEntry = {
  image: HTMLImageElement | null;
  promise: Promise<HTMLImageElement>;
};

const spriteImageCache: Record<string, SpriteImageCacheEntry> = {};

/**
 * Fetches a sprite PNG image, deduplicated by URL.
 * Returns a Promise that resolves to an HTMLImageElement.
 */
export function fetchSpriteImage(url: string): Promise<HTMLImageElement> {
  if (spriteImageCache[url]) {
    return spriteImageCache[url].promise;
  }

  const promise = new Promise<HTMLImageElement>((resolve, reject) => {
    const img = new window.Image();
    img.onload = () => {
      spriteImageCache[url].image = img;
      resolve(img);
    };
    img.onerror = (_err) => {
      reject(new Error(`Failed to load sprite image: ${url}`));
    };
    img.crossOrigin = 'anonymous';
    img.src = url;
  });

  spriteImageCache[url] = { image: null, promise };
  return promise;
}

// --- JSON Index Cache ---

type SpriteIndexCacheEntry = {
  index: SpriteIndex | null;
  promise: Promise<SpriteIndex>;
};

const spriteIndexCache: Record<string, SpriteIndexCacheEntry> = {};

/**
 * Fetches a sprite JSON index, deduplicated by URL.
 * Returns a Promise that resolves to the parsed SpriteIndex.
 */
export function fetchSpriteIndex(url: string): Promise<SpriteIndex> {
  if (spriteIndexCache[url]) {
    return spriteIndexCache[url].promise;
  }

  const promise = fetch(url, { credentials: 'omit' }).then(async (res) => {
    if (!res.ok) {
      throw new Error(`Failed to fetch sprite index: ${url}`);
    }
    const json = await res.json();
    // Basic validation
    if (typeof json !== 'object' || json === null) {
      throw new Error('Sprite index JSON is not an object');
    }
    return json as SpriteIndex;
  });

  spriteIndexCache[url] = { index: null, promise };
  return promise;
}
