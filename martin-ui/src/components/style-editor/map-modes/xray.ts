import type {
  ExpressionSpecification,
  LayerSpecification,
  StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';

/** One block of data X-ray can draw: a source layer of a vector source, or a whole GeoJSON or raster source. */
export interface XrayEntry {
  /** Stable id, `source` or `source/sourceLayer`. */
  key: string;
  /** Style source id. */
  source: string;
  /** Source layer inside a vector source. */
  sourceLayer?: string;
  /** How the data is drawn. */
  kind: 'vector' | 'raster';
  /** Color the entry is drawn in. */
  color: string;
  /** Style layers that read this data. */
  usedBy: string[];
}

/** Prefix of every layer id X-ray adds. */
export const XRAY_PREFIX = 'xray:';

const BACKGROUND = '#0d0f12';
/** Entry hues run from orange to violet, staying clear of the pink editor accent the focus overlay uses. */
const HUE_START = 20;
const HUE_SPAN = 260;
/** Steps hues by the golden ratio so entries next to each other in the list never get similar colors. */
const GOLDEN = 0.618_034;
const POLYGON: ExpressionSpecification = [
  'match',
  ['geometry-type'],
  ['Polygon', 'MultiPolygon'],
  true,
  false,
];
const LINE_OR_POLYGON: ExpressionSpecification = [
  'match',
  ['geometry-type'],
  ['LineString', 'MultiLineString', 'Polygon', 'MultiPolygon'],
  true,
  false,
];
const POINT: ExpressionSpecification = [
  'match',
  ['geometry-type'],
  ['Point', 'MultiPoint'],
  true,
  false,
];

/** Key of the data a style layer reads, or `undefined` for layers without a source. */
export function entryKeyOf(layer: LayerSpecification): string | undefined {
  if (!('source' in layer) || typeof layer.source !== 'string') return undefined;
  const sourceLayer = 'source-layer' in layer ? layer['source-layer'] : undefined;
  return sourceLayer ? `${layer.source}/${sourceLayer}` : layer.source;
}

/** Lists every block of data in the style's sources, each with a distinct color. */
export function xrayEntries(
  style: StyleSpecification,
  sourceLayers: (source: string) => readonly string[],
): XrayEntry[] {
  const usedBy = new Map<string, string[]>();
  for (const layer of style.layers) {
    const key = entryKeyOf(layer);
    if (key) usedBy.set(key, [...(usedBy.get(key) ?? []), layer.id]);
  }
  const blocks: Omit<XrayEntry, 'color' | 'usedBy'>[] = [];
  for (const [source, spec] of Object.entries(style.sources)) {
    if (spec.type === 'vector') {
      for (const sourceLayer of sourceLayers(source)) {
        blocks.push({ key: `${source}/${sourceLayer}`, kind: 'vector', source, sourceLayer });
      }
    } else if (spec.type === 'geojson') {
      blocks.push({ key: source, kind: 'vector', source });
    } else if (spec.type === 'raster' || spec.type === 'raster-dem') {
      blocks.push({ key: source, kind: 'raster', source });
    }
  }
  return blocks.map((block, i) => ({
    ...block,
    color: `hsl(${Math.round(HUE_START + ((i * GOLDEN) % 1) * HUE_SPAN)} 80% 62%)`,
    usedBy: usedBy.get(block.key) ?? [],
  }));
}

type Part = 'raster' | 'fill' | 'line' | 'point';

function entryLayer(
  entry: XrayEntry,
  part: Part,
  visible: boolean,
  dimmed: boolean,
): LayerSpecification | undefined {
  const id = `${XRAY_PREFIX}${entry.key}`;
  const layout = { visibility: visible ? ('visible' as const) : ('none' as const) };
  if (entry.kind === 'raster') {
    if (part !== 'raster') return undefined;
    return {
      id,
      layout,
      paint: {
        'raster-brightness-max': 0.5,
        'raster-opacity': dimmed ? 0.2 : 0.5,
        'raster-saturation': -1,
      },
      source: entry.source,
      type: 'raster',
    };
  }
  const from = entry.sourceLayer
    ? { source: entry.source, 'source-layer': entry.sourceLayer }
    : { source: entry.source };
  switch (part) {
    case 'raster':
      return undefined;
    case 'fill':
      return {
        ...from,
        filter: POLYGON,
        id: `${id}:fill`,
        layout,
        paint: { 'fill-color': entry.color, 'fill-opacity': dimmed ? 0.02 : 0.06 },
        type: 'fill',
      };
    case 'line':
      return {
        ...from,
        filter: LINE_OR_POLYGON,
        id: `${id}:line`,
        layout,
        paint: { 'line-color': entry.color, 'line-opacity': dimmed ? 0.15 : 0.85, 'line-width': 1 },
        type: 'line',
      };
    case 'point':
      return {
        ...from,
        filter: POINT,
        id: `${id}:point`,
        layout,
        paint: {
          'circle-color': entry.color,
          'circle-opacity': dimmed ? 0.15 : 1,
          'circle-radius': 2.5,
        },
        type: 'circle',
      };
  }
}

function focusLayers(focus: LayerSpecification, accent: string): LayerSpecification[] {
  if (!('source' in focus) || typeof focus.source !== 'string') return [];
  const from = {
    ...('source-layer' in focus && focus['source-layer']
      ? { 'source-layer': focus['source-layer'] }
      : {}),
    ...(focus.filter ? { filter: focus.filter } : {}),
    ...(focus.minzoom === undefined ? {} : { minzoom: focus.minzoom }),
    ...(focus.maxzoom === undefined ? {} : { maxzoom: focus.maxzoom }),
    source: focus.source,
  };
  const id = `${XRAY_PREFIX}focus`;
  switch (focus.type) {
    case 'fill':
    case 'fill-extrusion':
      return [
        {
          ...from,
          id: `${id}:fill`,
          paint: { 'fill-color': accent, 'fill-opacity': 0.4 },
          type: 'fill',
        },
        {
          ...from,
          id: `${id}:line`,
          paint: { 'line-color': accent, 'line-width': 1.5 },
          type: 'line',
        },
      ];
    case 'line':
      return [
        {
          ...from,
          id: `${id}:line`,
          paint: { 'line-color': accent, 'line-width': 2.5 },
          type: 'line',
        },
      ];
    case 'circle':
    case 'symbol':
    case 'heatmap':
      return [
        {
          ...from,
          id: `${id}:point`,
          paint: {
            'circle-color': accent,
            'circle-radius': 4,
            'circle-stroke-color': '#fff',
            'circle-stroke-width': 1,
          },
          type: 'circle',
        },
      ];
    default:
      return [];
  }
}

/** Options for {@link xrayStyle}. */
export interface XrayOptions {
  /** Keys of entries to leave out. */
  hidden: ReadonlySet<string>;
  /** Style layer whose matching features are drawn on top in `accent`. */
  focus?: LayerSpecification;
  /** Color of the focus overlay. */
  accent: string;
  /** Entry to bring forward; every other entry is dimmed. */
  emphasis?: string;
}

/** Draw order: areas under lines under points, so no geometry hides another kind. */
const PARTS: readonly Part[] = ['raster', 'fill', 'line', 'point'];

/** Replaces the style's layers with an unstyled drawing of every entry, keeping sources, sprite and glyphs. */
export function xrayStyle(
  style: StyleSpecification,
  entries: readonly XrayEntry[],
  { hidden, focus, accent, emphasis }: XrayOptions,
): StyleSpecification {
  const { sky: _sky, terrain: _terrain, ...rest } = style;
  return {
    ...rest,
    layers: [
      {
        id: `${XRAY_PREFIX}background`,
        paint: { 'background-color': BACKGROUND },
        type: 'background',
      },
      ...PARTS.flatMap((part) =>
        entries.flatMap((e) => {
          const layer = entryLayer(
            e,
            part,
            !hidden.has(e.key),
            emphasis !== undefined && e.key !== emphasis,
          );
          return layer ? [layer] : [];
        }),
      ),
      ...(focus ? focusLayers(focus, accent) : []),
    ],
  };
}

/** Entry key of an X-ray layer id, or `undefined` for other layers. */
export function entryKeyOfXrayLayer(layerId: string): string | undefined {
  if (!layerId.startsWith(XRAY_PREFIX)) return undefined;
  const rest = layerId.slice(XRAY_PREFIX.length);
  if (rest === 'background' || rest.startsWith('focus')) return undefined;
  return rest.replace(/:(fill|line|point)$/, '');
}

/** A rendered feature as far as {@link countXrayFeatures} needs it. */
export interface RenderedFeature {
  /** Id of the layer that drew it. */
  layerId: string;
  /** Feature id, when the tile has one. */
  id?: string | number;
  /** Feature properties, used to tell features apart when there is no id. */
  properties: Readonly<Record<string, unknown>>;
}

/** Counts distinct features per entry among those X-ray drew, ignoring the copies tile edges and layers produce. */
export function countXrayFeatures(features: Iterable<RenderedFeature>): Map<string, number> {
  const seen = new Map<string, Set<string>>();
  for (const f of features) {
    const key = entryKeyOfXrayLayer(f.layerId);
    if (!key) continue;
    const ids = seen.get(key) ?? new Set();
    ids.add(f.id === undefined ? JSON.stringify(f.properties) : String(f.id));
    seen.set(key, ids);
  }
  return new Map([...seen].map(([key, ids]) => [key, ids.size]));
}
