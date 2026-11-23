import type {
  ExpressionSpecification,
  LayerSpecification,
  SourceSpecification,
  StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';
import { validateStyleMin } from '@maplibre/maplibre-gl-style-spec';
import { hex6 } from '../colors';
import { paletteTokens } from '../palette/model';
import type { LayerType } from '../style-spec';

/** Color used for new layers when the style has no saturated color to reuse. */
export const FALLBACK_COLOR = '#e6005c';

/** Attributes of one source layer, by name with their TileJSON type. */
export interface SourceLayerInfo {
  /** Source layer name. */
  id: string;
  /** Attribute names mapped to `String`, `Number` or `Boolean`. */
  fields: Record<string, string>;
}

/** What the new layer reads from. */
export interface LayerDraft {
  /** Id of the new layer. */
  id: string;
  /** Layer type. */
  type: LayerType;
  /** Source id, absent for a background. */
  source?: string;
  /** Vector source layer, absent for other sources. */
  sourceLayer?: string;
  /** Attributes of the source layer, when known. */
  fields: Record<string, string>;
  /** First font of the style's `text-font`s, when any. */
  font?: string;
}

/** One-line description and label of each layer type. */
export const TYPE_INFO: Record<LayerType, { label: string; description: string }> = {
  background: { description: 'A solid color behind everything', label: 'Background' },
  circle: { description: 'Points as dots', label: 'Circle' },
  'color-relief': { description: 'Elevation colored by a ramp', label: 'Color relief' },
  fill: { description: 'Polygons filled with a color', label: 'Fill' },
  'fill-extrusion': { description: 'Polygons extruded into 3D', label: 'Extrusion' },
  heatmap: { description: 'Point density as a heat gradient', label: 'Heatmap' },
  hillshade: { description: 'Terrain shading from elevation', label: 'Hillshade' },
  line: { description: 'Lines and outlines', label: 'Line' },
  raster: { description: 'Raster tiles or images', label: 'Raster' },
  symbol: { description: 'Text and icons', label: 'Symbol' },
};

/** The style's most used saturated palette color as `#rrggbb`, else `FALLBACK_COLOR`. */
export function startingColor(style: StyleSpecification): string {
  for (const token of paletteTokens(style)) {
    const { r, g, b, a } = token.color;
    if (a > 0 && Math.max(r, g, b) - Math.min(r, g, b) > 0.15 * a) return hex6(token.color);
  }
  return FALLBACK_COLOR;
}

function firstStringField(fields: Record<string, string>): string | undefined {
  return Object.keys(fields).find((name) => fields[name] === 'String');
}

/** A complete layer of the draft's type with paint that shows up on the map right away. */
export function buildLayer(style: StyleSpecification, draft: LayerDraft): LayerSpecification {
  const { id, source, sourceLayer, fields } = draft;
  const color = startingColor(style);
  const base = {
    id,
    source: source ?? '',
    ...(sourceLayer ? { 'source-layer': sourceLayer } : {}),
  };
  switch (draft.type) {
    case 'fill':
      return { ...base, paint: { 'fill-color': color, 'fill-opacity': 0.6 }, type: 'fill' };
    case 'line':
      return { ...base, paint: { 'line-color': color, 'line-width': 2 }, type: 'line' };
    case 'circle':
      return {
        ...base,
        paint: {
          'circle-color': color,
          'circle-radius': 5,
          'circle-stroke-color': '#ffffff',
          'circle-stroke-width': 1,
        },
        type: 'circle',
      };
    case 'symbol': {
      const attribute = 'name' in fields ? 'name' : (firstStringField(fields) ?? 'name');
      const textField: ExpressionSpecification = ['get', attribute];
      return {
        ...base,
        layout: {
          'text-field': textField,
          ...(draft.font ? { 'text-font': [draft.font] } : {}),
          'text-size': 12,
        },
        paint: { 'text-color': '#000000', 'text-halo-color': '#ffffff', 'text-halo-width': 1.5 },
        type: 'symbol',
      };
    }
    case 'fill-extrusion': {
      const height: ExpressionSpecification = ['get', 'render_height'];
      return {
        ...base,
        paint: {
          'fill-extrusion-color': color,
          'fill-extrusion-height': 'render_height' in fields ? height : 10,
        },
        type: 'fill-extrusion',
      };
    }
    case 'heatmap':
      return { ...base, type: 'heatmap' };
    case 'raster':
      return { ...base, type: 'raster' };
    case 'hillshade':
      return { ...base, type: 'hillshade' };
    case 'color-relief':
      return { ...base, type: 'color-relief' };
    case 'background':
      return { id, paint: { 'background-color': color }, type: 'background' };
  }
}

/** Spec validation messages for the new layer, checked in a copy of the style that also holds the new source. */
export function validateNewLayer(
  style: StyleSpecification,
  layer: LayerSpecification,
  newSource?: { id: string; spec: SourceSpecification },
): string[] {
  const copy = structuredClone(style);
  if (newSource) copy.sources[newSource.id] = newSource.spec;
  copy.layers.push(layer);
  const prefix = `layers[${copy.layers.length - 1}]`;
  return validateStyleMin(copy)
    .map((e) => e.message)
    .filter((m) => m.startsWith(prefix) || m.startsWith(`sources.${newSource?.id}`));
}

/** A source id not used in the style yet, derived from `base`. */
export function uniqueSourceId(style: StyleSpecification, base: string): string {
  if (!(base in style.sources)) return base;
  let n = 2;
  while (`${base}-${n}` in style.sources) n++;
  return `${base}-${n}`;
}

/** The `vector_layers` of a TileJSON document, empty when it has none. */
export function parseVectorLayers(tileJson: unknown): SourceLayerInfo[] {
  const layers = (tileJson as { vector_layers?: unknown } | null)?.vector_layers;
  if (!Array.isArray(layers)) return [];
  return layers.flatMap((l: { id?: unknown; fields?: unknown }): SourceLayerInfo[] =>
    typeof l?.id === 'string'
      ? [{ fields: (l.fields as Record<string, string> | undefined) ?? {}, id: l.id }]
      : [],
  );
}
