import maplibregl from 'maplibre-gl';
import 'maplibre-gl/dist/maplibre-gl.css';
import { useCallback, useEffect, useRef } from 'react';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { buildMartinUrl } from '@/lib/api';

type SdfMapPreviewProps = {
  spriteUrl: string;
  spriteIds: readonly string[];
  iconColor: string;
  haloColor: string;
  haloWidth: number;
  haloBlur: number;
  iconSize: number;
};

const LAYER_ID = 'sdf-icons';
const SOURCE_ID = 'sdf-grid';
const MAX_COLS = 6;
const INITIAL_ZOOM = 15;
const TEXT_OFFSET_Y = 1.5;
const BASE_CELL_H = 130;

const EMPTY_FC: GeoJSON.FeatureCollection = {
  features: [],
  type: 'FeatureCollection',
};

function cellHeight(iconSize: number): number {
  const iconPx = 22 * iconSize;
  const textPx = 16;
  const offsetPx = TEXT_OFFSET_Y * textPx;
  return Math.max(BASE_CELL_H, iconPx + offsetPx + textPx * 2 + 20);
}

function buildGridFeatures(
  map: maplibregl.Map,
  spriteIds: readonly string[],
  cols: number,
  cellH: number,
): GeoJSON.Feature<GeoJSON.Point>[] {
  const canvasW = map.getCanvas().clientWidth;
  const cellW = canvasW / cols;

  return spriteIds.map((id, i) => {
    const col = i % cols;
    const row = Math.floor(i / cols);
    const px = col * cellW + cellW / 2;
    const py = row * cellH + cellH / 2;
    const lngLat = map.unproject([px, py]);
    return {
      geometry: {
        coordinates: [lngLat.lng, lngLat.lat],
        type: 'Point',
      },
      properties: { icon: id, label: id },
      type: 'Feature',
    };
  });
}

function queryIconAtPoint(map: maplibregl.Map, point: [number, number]): string | undefined {
  const features = map.queryRenderedFeatures(point, { layers: [LAYER_ID] });
  return features[0]?.properties?.icon as string | undefined;
}

export function SdfMapPreview({
  spriteUrl,
  spriteIds,
  iconColor,
  haloColor,
  haloWidth,
  haloBlur,
  iconSize,
}: SdfMapPreviewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const { copy } = useCopyToClipboard({
    successMessage: 'Sprite ID copied to clipboard',
  });
  const copyRef = useRef(copy);
  copyRef.current = copy;

  const cols = Math.min(spriteIds.length, MAX_COLS);
  const rows = Math.ceil(spriteIds.length / Math.max(cols, 1));
  const cellH = cellHeight(iconSize);

  const styleRef = useRef({ haloBlur, haloColor, haloWidth, iconColor, iconSize });
  styleRef.current = { haloBlur, haloColor, haloWidth, iconColor, iconSize };

  const handleClick = useCallback((e: MouseEvent) => {
    const map = mapRef.current;
    if (!map || !map.isStyleLoaded()) return;

    const rect = map.getCanvas().getBoundingClientRect();
    const point: [number, number] = [e.clientX - rect.left, e.clientY - rect.top];
    const name = queryIconAtPoint(map, point);
    if (name) copyRef.current(name);
  }, []);

  const handleMouseMove = useCallback((e: MouseEvent) => {
    const map = mapRef.current;
    const container = containerRef.current;
    if (!map || !map.isStyleLoaded() || !container) return;

    const rect = map.getCanvas().getBoundingClientRect();
    const point: [number, number] = [e.clientX - rect.left, e.clientY - rect.top];
    const name = queryIconAtPoint(map, point);
    container.style.cursor = name ? 'pointer' : '';
  }, []);

  useEffect(() => {
    if (!containerRef.current || spriteIds.length === 0) return;

    const sdfSpriteUrl = buildMartinUrl(spriteUrl.replace('/sprite/', '/sdf_sprite/'));

    const map = new maplibregl.Map({
      center: [0, 0],
      container: containerRef.current,
      // pixelRatio: 2,
      interactive: false,
      style: {
        glyphs: 'https://demotiles.maplibre.org/font/{fontstack}/{range}.pbf',
        layers: [
          {
            id: 'background',
            paint: { 'background-color': '#f9fafb' },
            type: 'background',
          },
          {
            id: LAYER_ID,
            layout: {
              'icon-allow-overlap': true,
              'icon-ignore-placement': true,
              'icon-image': ['get', 'icon'],
              'icon-size': styleRef.current.iconSize,
              'text-allow-overlap': true,
              'text-anchor': 'top',
              'text-field': ['get', 'label'],
              'text-font': ['Open Sans Regular', 'Arial Unicode MS Regular'],
              'text-ignore-placement': true,
              'text-max-width': 8,
              'text-offset': [0, TEXT_OFFSET_Y],
              'text-size': 13,
            },
            paint: {
              'icon-color': styleRef.current.iconColor,
              'icon-halo-blur': styleRef.current.haloBlur,
              'icon-halo-color': styleRef.current.haloColor,
              'icon-halo-width': styleRef.current.haloWidth,
              'text-color': '#6b7280',
            },
            source: SOURCE_ID,
            type: 'symbol',
          },
        ],
        sources: {
          [SOURCE_ID]: { data: EMPTY_FC, type: 'geojson' },
        },
        sprite: sdfSpriteUrl,
        version: 8,
      },
      zoom: INITIAL_ZOOM,
    });

    map.once('load', () => {
      const features = buildGridFeatures(map, spriteIds, cols, cellH);
      const source = map.getSource(SOURCE_ID) as maplibregl.GeoJSONSource;
      source.setData({ features, type: 'FeatureCollection' });
    });

    const canvas = map.getCanvas();
    canvas.addEventListener('click', handleClick);
    canvas.addEventListener('mousemove', handleMouseMove);

    mapRef.current = map;

    return () => {
      canvas.removeEventListener('click', handleClick);
      canvas.removeEventListener('mousemove', handleMouseMove);
      map.remove();
      mapRef.current = null;
    };
  }, [spriteUrl, spriteIds, cols, cellH, handleClick, handleMouseMove]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;

    const update = () => {
      if (!map.getLayer(LAYER_ID)) return;
      map.setLayoutProperty(LAYER_ID, 'icon-size', iconSize);
      map.setPaintProperty(LAYER_ID, 'icon-color', iconColor);
      map.setPaintProperty(LAYER_ID, 'icon-halo-color', haloColor);
      map.setPaintProperty(LAYER_ID, 'icon-halo-width', haloWidth);
      map.setPaintProperty(LAYER_ID, 'icon-halo-blur', haloBlur);
    };

    if (map.isStyleLoaded()) {
      update();
    } else {
      map.once('style.load', update);
    }

    return () => {
      map.off('style.load', update);
    };
  }, [iconColor, iconSize, haloColor, haloWidth, haloBlur]);

  const totalHeight = rows * cellH;

  return (
    <div className="w-full rounded-lg border" ref={containerRef} style={{ height: totalHeight }} />
  );
}
