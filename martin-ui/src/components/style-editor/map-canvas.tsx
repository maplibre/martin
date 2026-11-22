import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { Map as MapLibre, NavigationControl } from 'maplibre-gl';
import { type CSSProperties, useEffect, useRef } from 'react';
import 'maplibre-gl/dist/maplibre-gl.css';
import { mltAcceptTransformRequest } from '@/lib/mlt';

interface MapCanvasProps {
  /** Style to render; changes are applied as a diff. */
  style: StyleSpecification;
  /** CSS filter on the canvas, used for color-vision simulation. */
  filter?: string;
  /** Called once with the map instance. */
  onReady?: (map: MapLibre) => void;
}

function cameraFromHash(): { zoom: number; center: [number, number] } | undefined {
  const m = /map=([\d.]+)\/(-?[\d.]+)\/(-?[\d.]+)/.exec(window.location.hash);
  return m ? { center: [Number(m[3]), Number(m[2])], zoom: Number(m[1]) } : undefined;
}

function writeHash(m: MapLibre) {
  const { lat, lng } = m.getCenter();
  const url = new URL(window.location.href);
  url.hash = `map=${m.getZoom().toFixed(2)}/${lat.toFixed(5)}/${lng.toFixed(5)}`;
  window.history.replaceState(window.history.state, '', url);
}

/** MapLibre map filling its parent; the camera is kept in the URL hash so reloads keep the view. */
export function MapCanvas({ style, filter, onReady }: MapCanvasProps) {
  const container = useRef<HTMLDivElement>(null);
  const map = useRef<MapLibre | null>(null);
  const applied = useRef(style);

  useEffect(() => {
    if (!container.current) return;
    const m = new MapLibre({
      attributionControl: { compact: true },
      container: container.current,
      style,
      transformRequest: mltAcceptTransformRequest(() => map.current ?? undefined),
      ...cameraFromHash(),
    });
    m.on('moveend', () => writeHash(m));
    m.addControl(new NavigationControl({ visualizePitch: true }), 'bottom-right');
    map.current = m;
    onReady?.(m);
    return () => {
      m.remove();
      map.current = null;
    };
  }, []);

  useEffect(() => {
    if (applied.current === style) return;
    applied.current = style;
    map.current?.setStyle(style as Parameters<MapLibre['setStyle']>[0], { diff: true });
  }, [style]);

  const css: CSSProperties = { filter, inset: 0, position: 'absolute' };
  // MapLibre's unlayered `.maplibregl-map { position: relative }` beats Tailwind's layered utilities, so position inline.
  return <div ref={container} style={css} />;
}
