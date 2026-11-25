import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import {
  Color,
  type ColorSite,
  colorRole,
  colorSites,
  contrastRatio,
  deltaE,
  formatColor,
} from '../colors';
import { groupValues } from '../style-spec';

/** One distinct color and everywhere it is used. */
export interface Token {
  /** Canonical color key from `colorKey`. */
  key: string;
  /** The color. */
  color: Color;
  /** Every place it appears. */
  sites: ColorSite[];
  /** Ids of layers that use it. */
  layers: Set<string>;
  /** Roles from `colorRole`, e.g. `fill`, `halo`. */
  roles: Set<string>;
}

/** Distinct colors of the style, most used first. */
export function paletteTokens(style: StyleSpecification): Token[] {
  const byKey = new Map<string, Token>();
  for (const site of colorSites(style)) {
    let token = byKey.get(site.key);
    if (!token) {
      token = { color: site.color, key: site.key, layers: new Set(), roles: new Set(), sites: [] };
      byKey.set(site.key, token);
    }
    token.sites.push(site);
    token.layers.add(site.layer);
    token.roles.add(colorRole(site.name));
  }
  return [...byKey.values()].sort((a, b) => b.sites.length - a.sites.length);
}

/** Groups of tokens closer than `threshold` OKLab ΔE with equal alpha; most used first in each group. */
export function nearDuplicates(tokens: Token[], threshold = 2.5): Token[][] {
  const groups: Token[][] = [];
  for (const token of [...tokens].sort((a, b) => b.sites.length - a.sites.length)) {
    const home = groups.find(
      ([first]) =>
        first.color.rgb[3] === token.color.rgb[3] && deltaE(first.color, token.color) < threshold,
    );
    if (home) home.push(token);
    else groups.push([token]);
  }
  return groups.filter((group) => group.length > 1);
}

/** A label whose literal text color is hard to read. */
export interface LabelIssue {
  /** Layer id. */
  layer: string;
  /** Text color. */
  text: Color;
  /** Color it is read against. */
  against: Color;
  /** Whether `against` is the halo or the background. */
  againstLabel: 'halo' | 'background';
  /** WCAG contrast ratio. */
  ratio: number;
}

function literal(value: unknown): Color | undefined {
  return typeof value === 'string' ? (Color.parse(value) ?? undefined) : undefined;
}

/** Symbol layers whose literal text color reads below 3:1 against its halo, or the background when there is no halo. */
export function labelIssues(style: StyleSpecification): LabelIssue[] {
  const backgroundLayer = style.layers.find((l) => l.type === 'background');
  const background =
    literal(
      backgroundLayer ? groupValues(backgroundLayer, 'paint')['background-color'] : undefined,
    ) ?? Color.white;
  return style.layers.flatMap((layer) => {
    if (layer.type !== 'symbol' || !('text-field' in groupValues(layer, 'layout'))) return [];
    const paint = groupValues(layer, 'paint');
    const rawText = paint['text-color'];
    if (rawText !== undefined && typeof rawText !== 'string') return [];
    const text = Color.parse(rawText ?? '#000000');
    if (!text) return [];
    const halo = literal(paint['text-halo-color']);
    const width = paint['text-halo-width'];
    const useHalo =
      halo !== undefined && typeof width === 'number' && width > 0 && halo.rgb[3] >= 0.5;
    const against = useHalo ? halo : background;
    const ratio = contrastRatio(text, against);
    if (ratio >= 3) return [];
    return [
      { against, againstLabel: useHalo ? 'halo' : 'background', layer: layer.id, ratio, text },
    ];
  });
}

/** One concrete way to raise a label's contrast, with the exact properties it writes. */
export interface ContrastFix {
  /** Short button text, e.g. `Darker text`. */
  label: string;
  /** Paint properties and values the fix writes. */
  changes: { name: string; value: string | number }[];
  /** Text color after the fix. */
  text: Color;
  /** Color the text is read against after the fix. */
  against: Color;
  /** WCAG contrast ratio after the fix. */
  ratio: number;
}

const TARGET_RATIO = 4.5;

function shiftLightness(text: Color, against: Color): Color | undefined {
  const [l, c, h] = text.oklch;
  const hue = Number.isFinite(h) ? h : 0;
  const darker = contrastRatio(against, Color.black) > contrastRatio(against, Color.white);
  for (let step = 1; step <= 40; step++) {
    const next = Math.min(1, Math.max(0, l + (darker ? -0.025 : 0.025) * step));
    const candidate = Color.parse(`oklch(${next} ${c} ${hue})`);
    if (candidate && contrastRatio(candidate, against) >= TARGET_RATIO) return candidate;
    if (next === 0 || next === 1) break;
  }
  return undefined;
}

/** The fixes offered for a low-contrast label: recolor the text keeping its hue, or put a black or white halo behind it. */
export function contrastFixes(style: StyleSpecification, issue: LabelIssue): ContrastFix[] {
  const fixes: ContrastFix[] = [];
  const text = shiftLightness(issue.text, issue.against);
  if (text) {
    const darker = text.oklch[0] < issue.text.oklch[0];
    fixes.push({
      against: issue.against,
      changes: [{ name: 'text-color', value: formatColor(text) }],
      label: darker ? 'Darker text' : 'Lighter text',
      ratio: contrastRatio(text, issue.against),
      text,
    });
  }
  const white = contrastRatio(issue.text, Color.white) >= contrastRatio(issue.text, Color.black);
  const halo = white ? Color.white : Color.black;
  const layer = style.layers.find((l) => l.id === issue.layer);
  const width = layer ? Number(groupValues(layer, 'paint')['text-halo-width']) : 0;
  fixes.push({
    against: halo,
    changes: [
      { name: 'text-halo-color', value: formatColor(halo) },
      ...(width > 0 ? [] : [{ name: 'text-halo-width', value: 1.5 }]),
    ],
    label: white ? 'White halo' : 'Black halo',
    ratio: contrastRatio(issue.text, halo),
    text: issue.text,
  });
  return fixes;
}
