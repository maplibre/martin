import type { MapEventType, Map as MapLibre } from 'maplibre-gl';
import { useEffect, useRef } from 'react';

/** Subscribes to a map event once per map, always calling the latest `handler` so callers need no memoisation. */
export function useMapEvent<T extends keyof MapEventType>(
  map: MapLibre | null,
  type: T,
  handler: (event: MapEventType[T]) => void,
): void {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    if (!map) return;
    const listener = (event: MapEventType[T]) => latest.current(event);
    map.on(type, listener);
    return () => {
      map.off(type, listener);
    };
  }, [map, type]);
}
