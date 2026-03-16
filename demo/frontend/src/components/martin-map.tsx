import type { Map as MapLibreMap, Popup, StyleSpecification } from 'maplibre-gl';
import { useCallback, useEffect, useRef, useState } from 'react';
import { buildHoveredFeature } from '@/lib/map-hover';
import { useMapLayers } from '@/lib/use-map-layers';
import { useMapTour } from '@/lib/use-map-tour';
import type { DemoLayerEntry, HoveredFeature } from '@/types/demo';
import type { FilterState } from './demo/FilterPanel';

export type { HoveredFeature } from '@/types/demo';

const DEMO_SPRITE_ID = 'icons';

/** Font stack id -> MapLibre text-font value when overriding layers */
const FONT_STACK_LAYER_FONT: Record<string, string[]> = {
  noto: ['Noto Sans Regular'],
};

function rewriteStyleForDemo(
  style: Record<string, unknown>,
  baseUrl: string,
  styling: { spriteType: 'sdf' | 'plain'; fontStack: string },
): void {
  const base = baseUrl.replace(/\/$/, '');

  if (style.sprite != null && typeof style.sprite === 'string') {
    const path =
      styling.spriteType === 'sdf' ? `/sdf_sprite/${DEMO_SPRITE_ID}` : `/sprite/${DEMO_SPRITE_ID}`;
    style.sprite = `${base}${path}`;
  }

  if (style.glyphs != null && typeof style.glyphs === 'string') {
    style.glyphs = `${base}/font/{fontstack}/{range}`;
  }

  if (styling.fontStack !== 'default') {
    const layerFont = FONT_STACK_LAYER_FONT[styling.fontStack];
    if (layerFont && Array.isArray(style.layers)) {
      for (const layer of style.layers as Record<string, unknown>[]) {
        if (layer.type === 'symbol' && layer.layout && typeof layer.layout === 'object') {
          const layout = layer.layout as Record<string, unknown>;
          layout['text-font'] = layerFont;
        }
      }
    }
  }
}

export interface MapStylingOptions {
  spriteType: 'sdf' | 'plain';
  styleId: string;
  fontStack: string;
}

export interface MartinMapProps {
  tileSources: DemoLayerEntry[];
  filterState?: FilterState;
  martinBaseUrl: string;
  activeLayer?: string;
  onActiveLayerChange?: (layerId: string) => void;
  onHoveredChange?: (feature: HoveredFeature | null) => void;
  styling?: MapStylingOptions;
}

export default function MartinMap({
  tileSources,
  filterState = {},
  martinBaseUrl,
  activeLayer: activeLayerProp,
  onActiveLayerChange: _onActiveLayerChange,
  onHoveredChange,
  styling = { fontStack: 'default', spriteType: 'sdf', styleId: 'dark' },
}: MartinMapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const popupRef = useRef<Popup | null>(null);
  const mapLoadFiredRef = useRef(false);

  const [internalLayer, setInternalLayer] = useState(() => tileSources[0]?.id ?? '');
  const activeLayer = activeLayerProp !== undefined ? activeLayerProp : internalLayer;
  const [mapLoaded, setMapLoaded] = useState(false);
  const [mapLoadError, setMapLoadError] = useState<string | null>(null);
  const [tileSourceError, setTileSourceError] = useState(false);
  const [webglError, setWebglError] = useState(false);

  const onHoveredChangeRef = useRef(onHoveredChange);
  onHoveredChangeRef.current = onHoveredChange;
  const notifyHovered = useCallback((value: HoveredFeature | null) => {
    onHoveredChangeRef.current?.(value);
  }, []);

  const sources = tileSources.length > 0 ? tileSources : [];
  const activeSrc = sources.find((s) => s.id === activeLayer) ?? sources[0];

  const { setLayer } = useMapLayers({ filterState, martinBaseUrl, tileSources });
  const { startTour, cancelTour } = useMapTour();

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-creates the map when source count or styling changes; sources content, activeLayer, and setLayer are accessed via stable refs or closures inside the effect
  useEffect(() => {
    const container = containerRef.current;
    if (!container || sources.length === 0) return;
    const initialActiveLayer = sources[0]?.id ?? '';
    if (activeLayerProp === undefined) setInternalLayer(initialActiveLayer);

    setMapLoadError(null);
    setTileSourceError(false);
    setWebglError(false);
    mapLoadFiredRef.current = false;

    let map: MapLibreMap | undefined;
    const hoveredIds: Record<string, string | number | null> = {};

    const init = async () => {
      const ml = await import('maplibre-gl');
      const base = martinBaseUrl.replace(/\/$/, '');
      const styleUrl =
        styling.styleId === 'light' ? `${base}/style/positron` : `${base}/style/toner`;

      let styleObj: Record<string, unknown>;
      try {
        const res = await fetch(styleUrl);
        if (!res.ok) {
          setMapLoadError(`Style failed to load (${res.status})`);
          return;
        }
        styleObj = (await res.json()) as Record<string, unknown>;
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        setMapLoadError(message);
        return;
      }

      rewriteStyleForDemo(styleObj, base, styling);

      try {
        map = new ml.Map({
          attributionControl: false,
          center: [10, 25],
          container,
          style: styleObj as StyleSpecification,
          zoom: 3.2,
        });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        try {
          const structured = JSON.parse(message) as { type?: string };
          if (structured.type === 'webglcontextcreationerror') {
            setWebglError(true);
            return;
          }
        } catch {
          // not JSON
        }
        throw err;
      }

      // From here on `map` is definitely assigned; capture as const for closure safety.
      const m = map;

      m.on('error', (e: { error?: Error }) => {
        const msg = e.error?.message ?? 'Map failed to load';
        if (!mapLoadFiredRef.current) setMapLoadError(msg);
        else setTileSourceError(true);
      });

      m.addControl(new ml.AttributionControl({ compact: true }), 'bottom-right');

      popupRef.current = new ml.Popup({
        className: 'martin-popup',
        closeButton: false,
        closeOnClick: false,
      });

      for (const src of sources) {
        if (src.layerType !== 'fill') continue;
        const layerId = `martin-${src.id}`;

        m.on('mousemove', layerId, (e) => {
          if (!e.features?.length) return;
          m.getCanvas().style.cursor = 'pointer';
          const feat = e.features[0];
          const prevId = hoveredIds[src.id];
          if (prevId !== null && prevId !== undefined) {
            m.setFeatureState(
              { id: prevId, source: `martin-${src.id}`, sourceLayer: src.sourceLayer },
              { hover: false },
            );
          }
          const nextId = feat.id ?? null;
          hoveredIds[src.id] = nextId;
          if (nextId !== null) {
            m.setFeatureState(
              { id: nextId, source: `martin-${src.id}`, sourceLayer: src.sourceLayer },
              { hover: true },
            );
          }
          notifyHovered(buildHoveredFeature(src, feat.properties as Record<string, unknown>));
        });

        m.on('mouseleave', layerId, () => {
          m.getCanvas().style.cursor = '';
          const prevId = hoveredIds[src.id];
          if (prevId !== null && prevId !== undefined) {
            m.setFeatureState(
              { id: prevId, source: `martin-${src.id}`, sourceLayer: src.sourceLayer },
              { hover: false },
            );
          }
          hoveredIds[src.id] = null;
          notifyHovered(null);
        });
      }

      m.on('style.load', () => {
        m.setProjection({ type: 'globe' });
      });

      m.on('sourcedata', (e: { sourceId?: string; isSourceLoaded?: boolean }) => {
        if (e.isSourceLoaded && e.sourceId?.startsWith('martin-')) setTileSourceError(false);
      });

      m.on('load', async () => {
        mapLoadFiredRef.current = true;
        mapRef.current = m;
        await setLayer(m, initialActiveLayer, ml);
        setMapLoaded(true);

        const onCancel = () => cancelTour();
        m.on('dragstart', onCancel);
        m.getContainer().addEventListener('touchstart', onCancel, { passive: true });
        m.getContainer().addEventListener('wheel', onCancel, { passive: true });
        startTour(m);
      });
    };

    init();

    return () => {
      cancelTour();
      map?.remove();
      mapRef.current = null;
    };
  }, [sources.length, styling]);

  useEffect(() => {
    if (sources.length > 0 && !activeLayer && activeLayerProp === undefined) {
      setInternalLayer(sources[0].id);
    }
  }, [sources, activeLayer, activeLayerProp]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !mapLoaded || !activeSrc) return;

    setLayer(map, activeLayer);

    if (activeSrc.viewCenter) {
      cancelTour();
      if (activeSrc.viewBounds) map.setMaxBounds(activeSrc.viewBounds);
      map.flyTo({ center: activeSrc.viewCenter, zoom: activeSrc.viewZoom });
    } else {
      map.setMaxBounds(undefined);
    }
  }, [activeLayer, mapLoaded, setLayer, activeSrc, cancelTour]);

  if (sources.length === 0) return <div className="relative w-full h-full bg-muted/20" />;

  return (
    <div className="absolute inset-x-0" style={{ bottom: '-33%', top: 0 }}>
      <div className="relative w-full h-full">
        <div className="w-full h-full" ref={containerRef} />
        {webglError && (
          <div
            className="absolute inset-0 flex flex-col items-center justify-center gap-4 bg-sky-100 dark:bg-sky-950/90 text-sky-800 dark:text-sky-200 p-4 text-center"
            role="alert"
          >
            <p className="text-sm max-w-md leading-relaxed">
              We are sorry, but it seems that <strong>your browser does not support WebGL</strong>,
              a technology for rendering 3D graphics on the web.
            </p>
            <p className="text-sm font-semibold">WebGL is required to display this map.</p>
            <a
              className="text-sm font-bold text-sky-700 dark:text-sky-300 underline hover:no-underline"
              href="https://wiki.openstreetmap.org/wiki/This_map_requires_WebGL"
              rel="noopener noreferrer"
              target="_blank"
            >
              Read more
            </a>
          </div>
        )}
        {mapLoadError && !webglError && (
          <div
            className="absolute inset-0 flex items-center justify-center bg-muted/95 p-4 text-center"
            role="alert"
          >
            <p className="text-sm text-foreground max-w-md">
              Map style could not be loaded. Tile server may be unavailable.
            </p>
          </div>
        )}
        {tileSourceError && !mapLoadError && !webglError && (
          <output className="absolute bottom-2 left-1/2 -translate-x-1/2 rounded bg-foreground/90 px-3 py-2 text-[11px] text-background shadow">
            Tiles are not loading. Check connection to the tile server.
          </output>
        )}
      </div>
    </div>
  );
}
