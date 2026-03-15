import type { Map as MapLibreMap } from 'maplibre-gl';
import { useCallback, useRef } from 'react';

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
const FLY_DURATION_MS = 5500;
const PAUSE_BETWEEN_MS = 2200;
const TRANSITION_ZOOM_START_MS = 4000;

export interface MapTourControls {
  /** Start the city tour on the given map instance. No-op if reduced motion is preferred. */
  startTour: (map: MapLibreMap) => void;
  /** Cancel any in-progress tour and prevent it from resuming. */
  cancelTour: () => void;
}

/**
 * Manages the auto-fly city tour shown before the user interacts with the map.
 * Returns stable `startTour` / `cancelTour` callbacks safe to pass as event handlers.
 */
export function useMapTour(): MapTourControls {
  const cancelledRef = useRef(false);
  const timeoutsRef = useRef<ReturnType<typeof setTimeout>[]>([]);

  const cancelTour = useCallback(() => {
    cancelledRef.current = true;
    for (const id of timeoutsRef.current) clearTimeout(id);
    timeoutsRef.current = [];
  }, []);

  const startTour = useCallback((map: MapLibreMap) => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    cancelledRef.current = false;
    timeoutsRef.current = [];

    let index = 0;
    const go = () => {
      if (cancelledRef.current) return;
      const city = TOUR_CITIES[index % TOUR_CITIES.length];
      index += 1;
      map.flyTo({ center: city.center, duration: FLY_DURATION_MS, zoom: TOUR_TRANSITION_ZOOM });
      const zoomInId = setTimeout(() => {
        if (cancelledRef.current) return;
        map.flyTo({ center: city.center, duration: FLY_DURATION_MS, zoom: TOUR_CITY_ZOOM });
        map.once('moveend', () => {
          if (cancelledRef.current) return;
          const pauseId = setTimeout(go, PAUSE_BETWEEN_MS);
          timeoutsRef.current.push(pauseId);
        });
      }, TRANSITION_ZOOM_START_MS);
      timeoutsRef.current.push(zoomInId);
    };

    go();
  }, []);

  return { cancelTour, startTour };
}
