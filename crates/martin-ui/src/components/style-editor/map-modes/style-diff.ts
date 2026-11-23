import type { LayerSpecification, StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

/** How one layer differs from the earlier version. */
export interface LayerChange {
  /** Layer id. */
  id: string;
  /** Whether the layer is new or was edited. */
  kind: 'added' | 'changed';
  /** Changed paint and layout property names plus other changed keys such as `filter` or `minzoom`; empty for added layers. */
  properties: string[];
}

/** Layer-level differences between two versions of a style. */
export interface StyleDiff {
  /** Layers in `next` that are new or differ from `base`, in draw order. */
  layers: readonly LayerChange[];
  /** Ids of {@link layers}, for lookups. */
  changed: ReadonlySet<string>;
  /** Layer ids in `base` that `next` no longer has. */
  removed: readonly string[];
  /** Whether anything outside `layers` differs, e.g. sources, sprite or glyphs. */
  root: boolean;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

function fields(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object'
    ? Object.fromEntries(Object.entries(value))
    : {};
}

function changedProperties(before: LayerSpecification, after: LayerSpecification): string[] {
  const left = fields(before);
  const right = fields(after);
  const names: string[] = [];
  for (const key of new Set([...Object.keys(left), ...Object.keys(right)])) {
    if (same(left[key], right[key])) continue;
    if (key === 'paint' || key === 'layout') {
      const a = fields(left[key]);
      const b = fields(right[key]);
      for (const name of new Set([...Object.keys(a), ...Object.keys(b)])) {
        if (!same(a[name], b[name])) names.push(name);
      }
    } else {
      names.push(key);
    }
  }
  return names;
}

/** Compares two styles layer by layer; layer order alone does not count as a change. */
export function diffStyles(base: StyleSpecification, next: StyleSpecification): StyleDiff {
  const before = new Map(base.layers.map((l) => [l.id, l]));
  const after = new Set(next.layers.map((l) => l.id));
  const layers: LayerChange[] = [];
  for (const layer of next.layers) {
    const old = before.get(layer.id);
    if (!old) layers.push({ id: layer.id, kind: 'added', properties: [] });
    else if (!same(old, layer)) {
      layers.push({ id: layer.id, kind: 'changed', properties: changedProperties(old, layer) });
    }
  }
  const removed = base.layers.filter((l) => !after.has(l.id)).map((l) => l.id);
  const { layers: _a, ...baseRoot } = base;
  const { layers: _b, ...nextRoot } = next;
  return {
    changed: new Set(layers.map((l) => l.id)),
    layers,
    removed,
    root: !same(baseRoot, nextRoot),
  };
}
