import type {
  FillLayerSpecification,
  LineLayerSpecification,
  Map as MapLibreMap,
} from 'maplibre-gl';
import { useCallback, useRef } from 'react';
import type { FilterState } from '@/components/demo/FilterPanel';
import { buildMartinTileUrl } from '@/lib/demo-config';
import type { DemoLayerEntry } from '@/types/demo';

function buildTileUrl(
  layer: DemoLayerEntry,
  martinBaseUrl: string,
  filterState: FilterState,
): string {
  const base = buildMartinTileUrl(martinBaseUrl, layer.url);
  if (!layer.allowedParameters?.length) return base;
  const params = new URLSearchParams();
  for (const param of layer.allowedParameters) {
    const v = filterState[param.name];
    if (v !== undefined && v !== '') params.set(param.name, String(v));
  }
  const qs = params.toString();
  return qs ? `${base}?${qs}` : base;
}

export interface UseMapLayersOptions {
  tileSources: DemoLayerEntry[];
  martinBaseUrl: string;
  filterState: FilterState;
}

/**
 * Returns a stable `setLayer` callback that swaps the active vector tile source
 * and layer(s) on the given MapLibre map instance.
 *
 * Prop values are accessed via refs so `setLayer` never needs to be recreated.
 */
export function useMapLayers({ tileSources, martinBaseUrl, filterState }: UseMapLayersOptions) {
  const tileSourcesRef = useRef(tileSources);
  const martinBaseUrlRef = useRef(martinBaseUrl);
  const filterStateRef = useRef(filterState);
  tileSourcesRef.current = tileSources;
  martinBaseUrlRef.current = martinBaseUrl;
  filterStateRef.current = filterState;

  const setLayer = useCallback(
    async (map: MapLibreMap, layerId: string, mlModule?: typeof import('maplibre-gl')) => {
      const ml = mlModule ?? (await import('maplibre-gl'));
      const src = tileSourcesRef.current.find((s) => s.id === layerId);
      if (!src) return;

      for (const s of tileSourcesRef.current) {
        if (map.getLayer(`martin-${s.id}`)) map.removeLayer(`martin-${s.id}`);
        if (map.getLayer(`martin-${s.id}-hover`)) map.removeLayer(`martin-${s.id}-hover`);
        if (map.getSource(`martin-${s.id}`)) map.removeSource(`martin-${s.id}`);
      }

      const tileUrl = buildTileUrl(src, martinBaseUrlRef.current, filterStateRef.current);
      const promoteIdField = src.promoteIdField ?? 'id';
      map.addSource(`martin-${src.id}`, {
        maxzoom: 14,
        minzoom: 0,
        promoteId: { [src.sourceLayer]: promoteIdField },
        tiles: [tileUrl],
        type: 'vector',
      });

      if (src.layerType === 'line') {
        map.addLayer({
          id: `martin-${src.id}`,
          paint: src.paint as LineLayerSpecification['paint'],
          source: `martin-${src.id}`,
          'source-layer': src.sourceLayer,
          type: 'line',
        });
      } else {
        map.addLayer({
          id: `martin-${src.id}`,
          paint: src.paint as FillLayerSpecification['paint'],
          source: `martin-${src.id}`,
          'source-layer': src.sourceLayer,
          type: 'fill',
        });
        map.addLayer({
          id: `martin-${src.id}-hover`,
          paint: {
            'fill-color': '#95BEFA',
            'fill-opacity': ['case', ['boolean', ['feature-state', 'hover'], false], 0.35, 0],
          },
          source: `martin-${src.id}`,
          'source-layer': src.sourceLayer,
          type: 'fill',
        });
      }

      void ml;
    },
    [],
  );

  return { setLayer };
}
