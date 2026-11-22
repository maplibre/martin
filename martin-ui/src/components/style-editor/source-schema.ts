import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import type { Map as MapLibre } from 'maplibre-gl';
import { useEffect, useRef, useState } from 'react';

/** What the editor knows about one feature attribute of a source layer. */
export interface AttributeInfo {
  /** Attribute name as used in `['get', name]`. */
  name: string;
  /** Description from the TileJSON `vector_layers` entry, if any. */
  description?: string;
  /** Observed value type; `unknown` when only the TileJSON names it. */
  kind: 'string' | 'number' | 'boolean' | 'mixed' | 'unknown';
  /** Distinct observed values, most frequent first, capped at 100. */
  values: (string | number | boolean)[];
  /** Observed numeric range, when numeric. */
  range?: [number, number];
}

/** Looks up attributes of the features a layer draws. */
export interface SourceSchema {
  /** Attributes of `sourceLayer` in `source`, sorted by name; empty when nothing is known yet. */
  attributes: (source: string, sourceLayer: string | undefined) => AttributeInfo[];
  /** Source layers of a vector source from its TileJSON plus those seen in loaded tiles, sorted. */
  sourceLayers: (source: string) => string[];
}

interface Sample {
  description?: string;
  counts: Map<string | number | boolean, number>;
  types: Set<string>;
  min: number;
  max: number;
}

type Store = Map<string, Map<string, Sample>>;

const MAX_DISTINCT = 500;
const MAX_FEATURES = 3000;

function sampleFor(store: Store, key: string, attr: string): Sample {
  let layer = store.get(key);
  if (!layer) {
    layer = new Map();
    store.set(key, layer);
  }
  let s = layer.get(attr);
  if (!s) {
    s = {
      counts: new Map(),
      max: Number.NEGATIVE_INFINITY,
      min: Number.POSITIVE_INFINITY,
      types: new Set(),
    };
    layer.set(attr, s);
  }
  return s;
}

/** Collects attribute names from TileJSON and observed values from loaded tiles, growing as the map is panned. */
export function useSourceSchema(
  style: StyleSpecification | null,
  map: MapLibre | null,
): SourceSchema {
  const store = useRef<Store>(new Map());
  const layerIds = useRef<Map<string, Set<string>>>(new Map());
  const [version, setVersion] = useState(0);

  useEffect(() => {
    if (!style) return;
    const controller = new AbortController();
    for (const [id, source] of Object.entries(style.sources)) {
      if (source.type !== 'vector' || !source.url) continue;
      fetch(source.url, { signal: controller.signal })
        .then((r) => (r.ok ? r.json() : undefined))
        .then(
          (tilejson?: { vector_layers?: { id: string; fields?: Record<string, string> }[] }) => {
            for (const vl of tilejson?.vector_layers ?? []) {
              layerIds.current.set(id, (layerIds.current.get(id) ?? new Set()).add(vl.id));
              for (const [attr, description] of Object.entries(vl.fields ?? {})) {
                sampleFor(store.current, `${id}/${vl.id}`, attr).description = description;
              }
            }
            setVersion((v) => v + 1);
          },
        )
        .catch(() => undefined);
    }
    return () => controller.abort();
  }, [style?.sources]);

  useEffect(() => {
    if (!map || !style) return;
    const pairs = new Set<string>();
    for (const l of style.layers) {
      if ('source' in l && 'source-layer' in l && l['source-layer'])
        pairs.add(`${l.source}/${l['source-layer']}`);
    }
    const collect = () => {
      for (const pair of pairs) {
        const [source, sourceLayer] = pair.split('/');
        if (!map.getSource(source)) continue;
        const features = map.querySourceFeatures(source, { sourceLayer }).slice(0, MAX_FEATURES);
        for (const f of features) {
          for (const [attr, value] of Object.entries(f.properties ?? {})) {
            const s = sampleFor(store.current, pair, attr);
            s.types.add(typeof value);
            if (s.counts.size < MAX_DISTINCT || s.counts.has(value))
              s.counts.set(value, (s.counts.get(value) ?? 0) + 1);
            if (typeof value === 'number') {
              s.min = Math.min(s.min, value);
              s.max = Math.max(s.max, value);
            }
          }
        }
      }
      setVersion((v) => v + 1);
    };
    map.on('idle', collect);
    return () => {
      map.off('idle', collect);
    };
  }, [map, style?.layers]);

  return {
    attributes: (source, sourceLayer) => {
      void version;
      const layer = store.current.get(`${source}/${sourceLayer ?? ''}`);
      if (!layer) return [];
      return [...layer.entries()]
        .map(([name, s]): AttributeInfo => {
          const types = [...s.types].filter((t) => ['string', 'number', 'boolean'].includes(t));
          const kind =
            types.length === 0
              ? 'unknown'
              : types.length > 1
                ? 'mixed'
                : (types[0] as AttributeInfo['kind']);
          const values = [...s.counts.entries()]
            .sort((a, b) => b[1] - a[1])
            .slice(0, 100)
            .map(([v]) => v);
          const range: [number, number] | undefined = Number.isFinite(s.min)
            ? [s.min, s.max]
            : undefined;
          return { description: s.description, kind, name, range, values };
        })
        .sort((a, b) => a.name.localeCompare(b.name));
    },
    sourceLayers: (source) => {
      void version;
      const names = new Set(layerIds.current.get(source));
      for (const key of store.current.keys()) {
        if (key.startsWith(`${source}/`)) names.add(key.slice(source.length + 1));
      }
      return [...names].sort();
    },
  };
}
