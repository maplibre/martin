const STYLE_WORDS =
  'regular|normal|book|roman|bold|italic|oblique|medium|light|semi|demi|extra|ultra|black|thin|heavy|condensed|narrow|semibold|demibold|extrabold|extralight';
const SUFFIX = new RegExp(`^(.+?)[\\s_-]+(${STYLE_WORDS})$`, 'i');

/** One selectable font of a family. */
export interface FontVariant {
  /** Short label such as "Bold Italic"; "Regular" when the name has no style suffix. */
  label: string;
  /** Full font name as used in `text-font` and in the glyph URL. */
  name: string;
}

/** Fonts that share a family name once weight and style suffixes are stripped. */
export interface FontFamily {
  /** Family name without weight or style words. */
  family: string;
  /** Variants, "Regular" first. */
  variants: FontVariant[];
}

/** Splits a font name into its family and the weight/style words that follow it. */
export function splitFontName(name: string): { family: string; style: string } {
  let family = name.trim();
  const words: string[] = [];
  for (let m = SUFFIX.exec(family); m; m = SUFFIX.exec(family)) {
    family = m[1];
    words.unshift(m[2]);
  }
  return { family, style: words.join(' ') };
}

/** Groups font names into families, keeping first-seen family order. */
export function groupFamilies(names: string[]): FontFamily[] {
  const families = new Map<string, FontFamily>();
  for (const name of new Set(names)) {
    const { family, style } = splitFontName(name);
    const entry = families.get(family) ?? { family, variants: [] };
    entry.variants.push({ label: style || 'Regular', name });
    families.set(family, entry);
  }
  return [...families.values()].map((f) => ({
    ...f,
    variants: f.variants.toSorted(
      (a, b) => Number(b.label === 'Regular') - Number(a.label === 'Regular'),
    ),
  }));
}

/** Keeps the families and variants that match a case-insensitive search. */
export function filterFamilies(families: FontFamily[], query: string): FontFamily[] {
  const q = query.trim().toLowerCase();
  if (!q) return families;
  return families.flatMap((f) => {
    if (f.family.toLowerCase().includes(q)) return [f];
    const variants = f.variants.filter((v) => v.name.toLowerCase().includes(q));
    return variants.length ? [{ ...f, variants }] : [];
  });
}
