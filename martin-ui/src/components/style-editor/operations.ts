import type {
  LayerSpecification,
  SourceSpecification,
  StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';
import type { PropertyGroup, PropertyRef } from './style-spec';

type Holder = Record<PropertyGroup, Record<string, unknown> | undefined>;

function findLayer(style: StyleSpecification, id: string): LayerSpecification | undefined {
  return style.layers.find((l) => l.id === id);
}

/** Sets one property, or removes it when `value` is `undefined`; an emptied group is dropped too. */
export function setProperty(style: StyleSpecification, ref: PropertyRef, value: unknown) {
  const layer = findLayer(style, ref.layer);
  if (!layer) return;
  const holder = layer as unknown as Partial<Holder>;
  const values = holder[ref.group] ?? {};
  if (value === undefined) delete values[ref.name];
  else values[ref.name] = value;
  if (Object.keys(values).length) holder[ref.group] = values;
  else delete holder[ref.group];
}

/** Shows or hides layers through `layout.visibility`, removing the key when visible. */
export function setVisibility(style: StyleSpecification, ids: string[], visible: boolean) {
  for (const id of ids)
    setProperty(
      style,
      { group: 'layout', layer: id, name: 'visibility' },
      visible ? undefined : 'none',
    );
}

/** Sets or clears a layer's zoom range. */
export function setZoomRange(
  style: StyleSpecification,
  id: string,
  min: number | undefined,
  max: number | undefined,
) {
  const layer = findLayer(style, id);
  if (!layer) return;
  if (min === undefined) delete layer.minzoom;
  else layer.minzoom = min;
  if (max === undefined) delete layer.maxzoom;
  else layer.maxzoom = max;
}

/** Moves a layer to a new index in draw order (0 draws first). */
export function moveLayer(style: StyleSpecification, id: string, toIndex: number) {
  const from = style.layers.findIndex((l) => l.id === id);
  if (from < 0) return;
  const [layer] = style.layers.splice(from, 1);
  style.layers.splice(Math.max(0, Math.min(toIndex, style.layers.length)), 0, layer);
}

/** A layer id not used yet, derived from `base`. */
export function uniqueLayerId(style: StyleSpecification, base: string): string {
  const ids = new Set(style.layers.map((l) => l.id));
  if (!ids.has(base)) return base;
  let n = 2;
  while (ids.has(`${base}-${n}`)) n++;
  return `${base}-${n}`;
}

/** Inserts a copy right above the original and returns the copy's id. */
export function duplicateLayer(style: StyleSpecification, id: string): string | undefined {
  const index = style.layers.findIndex((l) => l.id === id);
  if (index < 0) return undefined;
  const copy = structuredClone(style.layers[index]);
  copy.id = uniqueLayerId(style, `${id}-copy`);
  style.layers.splice(index + 1, 0, copy);
  return copy.id;
}

/** Removes a layer. */
export function deleteLayer(style: StyleSpecification, id: string) {
  style.layers = style.layers.filter((l) => l.id !== id);
}

/** Renames a layer; returns false when the new id is empty or taken. */
export function renameLayer(style: StyleSpecification, id: string, next: string): boolean {
  if (!next || style.layers.some((l) => l.id === next)) return false;
  const layer = findLayer(style, id);
  if (!layer) return false;
  layer.id = next;
  return true;
}

/** Sets a layer's filter, or removes it when `filter` is `undefined`. */
export function setFilter(style: StyleSpecification, id: string, filter: unknown) {
  const layer = findLayer(style, id) as { filter?: unknown } | undefined;
  if (!layer) return;
  if (filter === undefined) delete layer.filter;
  else layer.filter = filter;
}

/** Inserts a layer at `index` in draw order (0 draws first), appending when the index is past the end. */
export function addLayer(style: StyleSpecification, layer: LayerSpecification, index: number) {
  style.layers.splice(Math.max(0, Math.min(index, style.layers.length)), 0, layer);
}

/** Adds or replaces a source definition. */
export function setSource(style: StyleSpecification, id: string, source: SourceSpecification) {
  style.sources[id] = source;
}

/** Points a layer at another source and source layer; `sourceLayer` is dropped for sources without layers. */
export function setLayerSource(
  style: StyleSpecification,
  id: string,
  source: string,
  sourceLayer: string | undefined,
) {
  const layer = findLayer(style, id) as { source?: string; 'source-layer'?: string } | undefined;
  if (!layer) return;
  layer.source = source;
  if (sourceLayer) layer['source-layer'] = sourceLayer;
  else delete layer['source-layer'];
}

/** Sets a top-level style field such as `glyphs`, `light` or `center`; `undefined` removes it. */
export function setRoot<K extends keyof StyleSpecification>(
  style: StyleSpecification,
  key: K,
  value: StyleSpecification[K] | undefined,
) {
  if (value === undefined) delete style[key];
  else style[key] = value;
}

/** Ids of layers that draw from a source. */
export function layersUsingSource(style: StyleSpecification, source: string): string[] {
  return style.layers.filter((l) => 'source' in l && l.source === source).map((l) => l.id);
}

/** Removes a source together with every layer that draws from it, and terrain pointing at it. */
export function removeSource(style: StyleSpecification, id: string) {
  delete style.sources[id];
  style.layers = style.layers.filter((l) => !('source' in l && l.source === id));
  if (style.terrain?.source === id) delete style.terrain;
}

/** Renames a source and updates every layer and the terrain that reference it; false when the new id is empty or taken. */
export function renameSource(style: StyleSpecification, from: string, to: string): boolean {
  if (!to || to in style.sources || !(from in style.sources)) return false;
  style.sources[to] = style.sources[from];
  delete style.sources[from];
  for (const layer of style.layers) {
    if ('source' in layer && layer.source === from) layer.source = to;
  }
  if (style.terrain?.source === from) style.terrain.source = to;
  return true;
}
