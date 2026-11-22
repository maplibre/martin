import { Color, type StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { groupValues, type PropertyGroup, propertySpec } from './style-spec';

export { Color };

/** Path from a property value down to one nested literal. */
export type ValuePath = (string | number)[];

/** One literal color inside a color-typed property, addressed precisely enough to rewrite only it. */
export interface ColorSite {
  /** Layer id. */
  layer: string;
  /** Group of the property. */
  group: PropertyGroup;
  /** Property name, e.g. `fill-color`. */
  name: string;
  /** Path inside the property value; empty for a plain literal. */
  path: ValuePath;
  /** Parsed color. */
  color: Color;
  /** Canonical `#rrggbb` or `#rrggbbaa` used to group identical colors. */
  key: string;
}

const byte = (x: number) =>
  Math.round(x * 255)
    .toString(16)
    .padStart(2, '0');

/** Canonical `#rrggbb`, or `#rrggbbaa` when translucent. */
export function colorKey(c: Color): string {
  const [r, g, b, a] = c.rgb;
  return a >= 1 ? `#${byte(r)}${byte(g)}${byte(b)}` : `#${byte(r)}${byte(g)}${byte(b)}${byte(a)}`;
}

/** Opaque `#rrggbb`, what `<input type=color>` accepts. */
export function hex6(c: Color): string {
  return colorKey(c).slice(0, 7);
}

/** The string written into the style: hex when opaque, `rgba()` otherwise. */
export function formatColor(c: Color): string {
  const [r, g, b, a] = c.rgb;
  if (a >= 1) return hex6(c);
  return `rgba(${[r, g, b].map((x) => Math.round(x * 255)).join(',')},${Math.round(a * 1000) / 1000})`;
}

/** The same color with a new alpha in 0..1. */
export function withAlpha(c: Color, alpha: number): Color {
  const [r, g, b] = c.rgb;
  return new Color(r, g, b, alpha, false);
}

/** The RGB of `rgbFrom` with the alpha of `c`. */
export function withRgb(c: Color, rgbFrom: Color): Color {
  const [r, g, b] = rgbFrom.rgb;
  return new Color(r, g, b, c.rgb[3], false);
}

/** Perceptual distance in OKLab scaled to roughly CIE ΔE units; below about 2 people cannot tell colors apart. */
export function deltaE(a: Color, b: Color): number {
  const [l1, a1, b1] = a.oklab;
  const [l2, a2, b2] = b.oklab;
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2) * 100;
}

/** `n` distinguishable colors spread evenly around the OKLCH hue circle from `base`, at a readable lightness and chroma. */
export function categoricalColors(base: Color, n: number): Color[] {
  const [l, c, h] = base.oklch;
  const lightness = Math.min(Math.max(l, 0.55), 0.8);
  const chroma = Math.max(c, 0.13);
  const start = Number.isFinite(h) ? h : 25;
  return Array.from(
    { length: n },
    (_, i) => Color.parse(`oklch(${lightness} ${chroma} ${(start + (i * 360) / n) % 360})`) ?? base,
  );
}

/** WCAG 2 contrast ratio, ignoring alpha. */
export function contrastRatio(a: Color, b: Color): number {
  const lum = (c: Color) => {
    const [r, g, bl] = c.rgb
      .slice(0, 3)
      .map((s) => (s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi, lo] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (hi + 0.05) / (lo + 0.05);
}

function walkOutputs(v: unknown, path: ValuePath, emit: (path: ValuePath, raw: string) => void) {
  if (typeof v === 'string') {
    emit(path, v);
    return;
  }
  if (v && typeof v === 'object' && !Array.isArray(v)) {
    const fn = v as { stops?: [unknown, unknown][]; default?: unknown };
    fn.stops?.forEach((s, i) => {
      walkOutputs(s[1], [...path, 'stops', i, 1], emit);
    });
    if (fn.default !== undefined) walkOutputs(fn.default, [...path, 'default'], emit);
    return;
  }
  if (!Array.isArray(v)) return;
  const at = (i: number) => walkOutputs(v[i], [...path, i], emit);
  const op = v[0];
  if (op === 'literal' || op === 'to-color' || op === 'coalesce') {
    for (let i = 1; i < v.length; i++) at(i);
  } else if (op === 'match') {
    for (let i = 3; i < v.length - 1; i += 2) at(i);
    at(v.length - 1);
  } else if (op === 'case') {
    for (let i = 2; i < v.length - 1; i += 2) at(i);
    at(v.length - 1);
  } else if (op === 'step') {
    at(2);
    for (let i = 4; i < v.length; i += 2) at(i);
  } else if (typeof op === 'string' && op.startsWith('interpolate')) {
    for (let i = 4; i < v.length; i += 2) at(i);
  } else if (op === 'let') {
    at(v.length - 1);
  }
}

/** Every literal color in every color-typed property; only expression output positions are visited, so match labels never count. */
export function colorSites(style: StyleSpecification): ColorSite[] {
  const sites: ColorSite[] = [];
  for (const layer of style.layers) {
    for (const group of ['paint', 'layout'] as const) {
      for (const [name, value] of Object.entries(groupValues(layer, group))) {
        if (propertySpec(layer.type, group, name)?.type !== 'color') continue;
        walkOutputs(value, [], (path, raw) => {
          const color = Color.parse(raw);
          if (color)
            sites.push({ color, group, key: colorKey(color), layer: layer.id, name, path });
        });
      }
    }
  }
  return sites;
}

function setAt(value: unknown, path: ValuePath, next: unknown): unknown {
  if (path.length === 0) return next;
  const [head, ...rest] = path;
  const copy = (Array.isArray(value) ? [...value] : { ...(value as object) }) as Record<
    string | number,
    unknown
  >;
  copy[head] = setAt(copy[head], rest, next);
  return copy;
}

/** Rewrites each site in place inside `style`, leaving every other value untouched. */
export function writeColorSites(
  style: StyleSpecification,
  sites: ColorSite[],
  next: (site: ColorSite) => Color,
) {
  for (const site of sites) {
    const layer = style.layers.find((l) => l.id === site.layer);
    if (!layer) continue;
    const holder = layer as unknown as Record<PropertyGroup, Record<string, unknown> | undefined>;
    const values = holder[site.group] ?? {};
    holder[site.group] = values;
    values[site.name] = setAt(values[site.name], site.path, formatColor(next(site)));
  }
}

/** Short role of a color property for filtering, e.g. `halo` for `text-halo-color`. */
export function colorRole(name: string): string {
  if (name.includes('halo')) return 'halo';
  if (name.startsWith('text')) return 'text';
  if (name.startsWith('icon')) return 'icon';
  if (name.startsWith('background')) return 'background';
  if (name.includes('outline') || name.includes('stroke')) return 'outline';
  if (name.startsWith('line')) return 'line';
  if (name.startsWith('circle')) return 'circle';
  return 'fill';
}
