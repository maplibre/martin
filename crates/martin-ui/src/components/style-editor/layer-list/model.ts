import type { LayerSpecification } from '@maplibre/maplibre-gl-style-spec';

/** A run of consecutive layers (topmost first) sharing a source layer. */
export interface Run {
  /** Id of the topmost layer, stable key for the run. */
  key: string;
  /** Shared source layer, or undefined for a single ungrouped layer. */
  sourceLayer: string | undefined;
  /** Layers of the run, topmost first. */
  layers: LayerSpecification[];
}

/** Whether a layer is hidden by its layout visibility. */
export function isHidden(layer: LayerSpecification): boolean {
  return layer.layout?.visibility === 'none';
}

/** The source layer of a layer, if it has one. */
export function sourceLayerOf(layer: LayerSpecification): string | undefined {
  return 'source-layer' in layer ? layer['source-layer'] : undefined;
}

/** Folds consecutive layers with the same source layer into runs without reordering. */
export function groupRuns(layers: LayerSpecification[]): Run[] {
  const runs: Run[] = [];
  for (const layer of layers) {
    const sourceLayer = sourceLayerOf(layer);
    const last = runs.at(-1);
    if (last && sourceLayer !== undefined && last.sourceLayer === sourceLayer)
      last.layers.push(layer);
    else runs.push({ key: layer.id, layers: [layer], sourceLayer });
  }
  return runs;
}

/** Whether a layer matches the search text on id, source layer or type. */
export function matches(layer: LayerSpecification, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [layer.id, sourceLayerOf(layer) ?? '', layer.type].some((s) =>
    s.toLowerCase().includes(q),
  );
}

/** Where a dragged row lands relative to a target row, in display order. */
export type DropSide = 'above' | 'below';

/** Index in `layers` to pass to `onMove` when dropping `dragged` above or below `target` on screen. */
export function dropIndex(
  layers: LayerSpecification[],
  dragged: string,
  target: string,
  side: DropSide,
): number {
  const rest = layers.filter((l) => l.id !== dragged);
  const t = rest.findIndex((l) => l.id === target);
  // Display is reversed: above on screen means later in draw order.
  return side === 'above' ? t + 1 : t;
}
