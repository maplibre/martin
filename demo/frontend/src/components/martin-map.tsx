import type {
  FillLayerSpecification,
  LineLayerSpecification,
  Map as MapLibreMap,
  Popup,
} from 'maplibre-gl';
import { useCallback, useEffect, useRef, useState } from 'react';
import { buildMartinTileUrl } from '@/lib/demo-config';
import type { DemoLayerEntry } from '@/types/demo';
import type { FilterState } from './demo/FilterPanel';

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

export interface HoveredFeature {
  name: string;
  pop_est?: number;
  continent?: string;
  /** Trip stats from get_trips layer */
  locationid?: number;
  trips?: number;
  trips_price?: number;
  trips_duration?: number;
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

const NYC_CENTER: [number, number] = [-74.01, 40.71];
const NYC_BOUNDS: [[number, number], [number, number]] = [
  [-74.26, 40.48], // SW
  [-73.7, 40.93], // NE
];
const NYC_ZOOM = 10;

const TOUR_CITIES: ReadonlyArray<{ center: [number, number] }> = [
  { center: [11.58, 48.14] }, // Munich
  { center: [72.58, 23.03] }, // Ahmedabad
  { center: [81.63, 21.25] }, // Raipur
  { center: [139.69, 35.69] }, // Tokyo
  { center: [-73.25, -3.75] }, // Iquitos
  { center: [-74.01, 40.71] }, // NYC
];

const TOUR_CITY_ZOOM = 13;
const TOUR_TRANSITION_ZOOM = 5;

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
  const userHasMovedMapRef = useRef(false);
  const tourTimeoutsRef = useRef<ReturnType<typeof setTimeout>[]>([]);
  const [internalLayer, setInternalLayer] = useState(() => tileSources[0]?.id ?? '');
  const activeLayer = activeLayerProp !== undefined ? activeLayerProp : internalLayer;
  const [mapLoaded, setMapLoaded] = useState(false);
  const [, setHovered] = useState<HoveredFeature | null>(null);

  const onHoveredChangeRef = useRef(onHoveredChange);
  onHoveredChangeRef.current = onHoveredChange;
  const setHoveredAndNotify = useCallback((value: HoveredFeature | null) => {
    setHovered(value);
    onHoveredChangeRef.current?.(value);
  }, []);

  const tileSourcesRef = useRef(tileSources);
  const filterStateRef = useRef(filterState);
  const martinBaseUrlRef = useRef(martinBaseUrl);
  tileSourcesRef.current = tileSources;
  filterStateRef.current = filterState;
  martinBaseUrlRef.current = martinBaseUrl;

  const sources = tileSources.length > 0 ? tileSources : [];
  const activeSrc = sources.find((s) => s.id === activeLayer) ?? sources[0];

  const setLayer = useCallback(
    async (map: MapLibreMap, layerId: string, mlModule?: typeof import('maplibre-gl')) => {
      const ml = mlModule ?? (await import('maplibre-gl'));
      const src = tileSourcesRef.current.find((s) => s.id === layerId);
      if (!src) return;

      const all = tileSourcesRef.current;
      for (const s of all) {
        if (map.getLayer(`martin-${s.id}`)) map.removeLayer(`martin-${s.id}`);
        if (map.getLayer(`martin-${s.id}-hover`)) map.removeLayer(`martin-${s.id}-hover`);
        if (map.getSource(`martin-${s.id}`)) map.removeSource(`martin-${s.id}`);
      }

      const tileUrl = buildTileUrl(src, martinBaseUrlRef.current, filterStateRef.current);
      const promoteId =
        src.id === 'get_trips' ? { [src.sourceLayer]: 'locationid' } : { [src.sourceLayer]: 'id' };
      map.addSource(`martin-${src.id}`, {
        maxzoom: 14,
        minzoom: 0,
        promoteId,
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

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-creates the map when source count or styling changes; sources content, activeLayer, and setLayer are accessed via stable refs inside the effect
  useEffect(() => {
    const currentSources = tileSourcesRef.current;
    const container = containerRef.current;
    if (!container || currentSources.length === 0) return;
    const initialActiveLayer = currentSources[0]?.id ?? '';
    if (activeLayerProp === undefined) setInternalLayer(initialActiveLayer);

    let map: MapLibreMap;
    let cancelTour: (() => void) | undefined;
    const hoveredIds: Record<string, string | number | null> = {};

    const init = async () => {
      const ml = await import('maplibre-gl');

      const styleUrl =
        styling.styleId === 'light'
          ? `${martinBaseUrl.replace(/\/$/, '')}/style/positron`
          : `${martinBaseUrl.replace(/\/$/, '')}/style/toner`;

      map = new ml.Map({
        attributionControl: false,
        center: [10, 25],
        container,
        style: styleUrl,
        zoom: 3.2,
      });

      map.addControl(new ml.AttributionControl({ compact: true }), 'bottom-right');

      popupRef.current = new ml.Popup({
        className: 'martin-popup',
        closeButton: false,
        closeOnClick: false,
      });

      for (const src of currentSources) {
        if (src.layerType !== 'fill') continue;
        const layerId = `martin-${src.id}`;
        map.on('mousemove', layerId, (e) => {
          if (!e.features?.length) return;
          map.getCanvas().style.cursor = 'pointer';
          const feat = e.features[0];
          const prevId = hoveredIds[src.id];
          if (prevId !== null && prevId !== undefined) {
            map.setFeatureState(
              { id: prevId, source: `martin-${src.id}`, sourceLayer: src.sourceLayer },
              { hover: false },
            );
          }
          const nextId = feat.id ?? null;
          hoveredIds[src.id] = nextId;
          if (nextId !== null) {
            map.setFeatureState(
              { id: nextId, source: `martin-${src.id}`, sourceLayer: src.sourceLayer },
              { hover: true },
            );
          }
          const props = feat.properties as Record<string, unknown>;
          const locationid = props.locationid as number | undefined;
          const trips = props.trips as number | undefined;
          const tripsPrice = props.trips_price as number | undefined;
          const tripsDuration = props.trips_duration as number | undefined;
          if (
            typeof locationid !== 'undefined' ||
            typeof trips !== 'undefined' ||
            typeof tripsPrice !== 'undefined' ||
            typeof tripsDuration !== 'undefined'
          ) {
            setHoveredAndNotify({
              locationid,
              name: typeof locationid !== 'undefined' ? `Zone ${locationid}` : 'Zone',
              trips,
              trips_duration: tripsDuration,
              trips_price: tripsPrice,
            });
          } else {
            setHoveredAndNotify({
              continent: props.continent as string | undefined,
              name: String(props.name ?? ''),
              pop_est: props.pop_est as number | undefined,
            });
          }
        });
        map.on('mouseleave', layerId, () => {
          map.getCanvas().style.cursor = '';
          const prevId = hoveredIds[src.id];
          if (prevId !== null && prevId !== undefined) {
            map.setFeatureState(
              { id: prevId, source: `martin-${src.id}`, sourceLayer: src.sourceLayer },
              { hover: false },
            );
          }
          hoveredIds[src.id] = null;
          setHoveredAndNotify(null);
        });
      }

      map.on('style.load', () => {
        map.setProjection({ type: 'globe' });
      });

      map.on('load', async () => {
        mapRef.current = map;
        await setLayer(map, initialActiveLayer, ml);
        setMapLoaded(true);

        if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
        userHasMovedMapRef.current = false;
        tourTimeoutsRef.current = [];

        cancelTour = () => {
          userHasMovedMapRef.current = true;
          for (const id of tourTimeoutsRef.current) clearTimeout(id);
          tourTimeoutsRef.current = [];
        };
        map.on('dragstart', cancelTour);
        map.getContainer().addEventListener('touchstart', cancelTour, { passive: true });
        map.getContainer().addEventListener('wheel', cancelTour, { passive: true });

        const FLY_DURATION_MS = 5500;
        const PAUSE_BETWEEN_MS = 2200;
        const TRANSITION_ZOOM_START_MS = 4000; // Start zoom-in before we fully stop at transition zoom
        let index = 0;
        const runTour = () => {
          const go = () => {
            if (userHasMovedMapRef.current) return;
            const city = TOUR_CITIES[index % TOUR_CITIES.length];
            index += 1;
            // Fly to city at transition zoom (zoomed out) so users keep sense of direction
            map.flyTo({
              center: city.center,
              duration: FLY_DURATION_MS,
              zoom: TOUR_TRANSITION_ZOOM,
            });
            // Start zooming in before we fully stop at 5 so the motion flows continuously
            const zoomInTimeoutId = setTimeout(() => {
              if (userHasMovedMapRef.current) return;
              map.flyTo({
                center: city.center,
                duration: FLY_DURATION_MS,
                zoom: TOUR_CITY_ZOOM,
              });
              map.once('moveend', () => {
                if (userHasMovedMapRef.current) return;
                const id = setTimeout(go, PAUSE_BETWEEN_MS);
                tourTimeoutsRef.current.push(id);
              });
            }, TRANSITION_ZOOM_START_MS);
            tourTimeoutsRef.current.push(zoomInTimeoutId);
          };
          go();
        };
        runTour();
      });
    };

    init();

    return () => {
      for (const id of tourTimeoutsRef.current) clearTimeout(id);
      tourTimeoutsRef.current = [];
      if (cancelTour != null) {
        map?.getContainer().removeEventListener('touchstart', cancelTour);
        map?.getContainer().removeEventListener('wheel', cancelTour);
      }
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

    if (activeLayer === 'get_trips') {
      // Cancel the city tour and lock the view to NYC
      userHasMovedMapRef.current = true;
      for (const id of tourTimeoutsRef.current) clearTimeout(id);
      tourTimeoutsRef.current = [];
      map.setMaxBounds(NYC_BOUNDS);
      map.flyTo({ center: NYC_CENTER, zoom: NYC_ZOOM });
    } else {
      map.setMaxBounds(undefined);
    }
  }, [activeLayer, mapLoaded, setLayer, activeSrc]);

  if (sources.length === 0) return <div className="relative w-full h-full bg-muted/20" />;

  return (
    <div className="absolute inset-x-0" style={{ bottom: '-33%', top: 0 }}>
      <div className="w-full h-full" ref={containerRef} />
    </div>
  );
}
