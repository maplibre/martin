import type { MapRef } from '@vis.gl/react-maplibre';
import { Layer, Map as MapLibreMap, Source } from '@vis.gl/react-maplibre';
import type { ExpressionSpecification, MapLayerMouseEvent, StyleSpecification } from 'maplibre-gl';
import { useId, useRef, useState } from 'react';
import { Breakable } from '@/components/ui/breakable';
import { SegmentedControl } from '@/components/ui/segmented-control';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { buildMartinUrl } from '@/lib/api';
import 'maplibre-gl/dist/maplibre-gl.css';

type SpriteMapPreviewProps = {
  spriteName: string;
  spriteIds: readonly string[];
};

export function SpriteMapPreview({ spriteName, spriteIds }: SpriteMapPreviewProps) {
  const [mode, setMode] = useState<'png' | 'sdf'>('sdf');
  const [canvasTone, setCanvasTone] = useState<'light' | 'dark'>('light');
  const [iconSize, setIconSize] = useState(2.0);
  const [iconColor, setIconColor] = useState(() => {
    try {
      const ctx = document.createElement('canvas').getContext('2d');
      if (!ctx) return '#7c3aed';
      ctx.fillStyle = getComputedStyle(document.documentElement)
        .getPropertyValue('--primary')
        .trim();
      return ctx.fillStyle; // normalized to #rrggbb
    } catch {
      return '#7c3aed';
    }
  });
  const [haloColor, setHaloColor] = useState('#ffffff');
  const [haloWidth, setHaloWidth] = useState(1);
  const [baseZoom, setBaseZoom] = useState<number | null>(null);
  const [labelFrame, setLabelFrame] = useState<{
    cell: number;
    points: { x: number; y: number }[];
    scale: number;
  }>();
  const mapRef = useRef<MapRef>(null);
  const id = useId();
  const layerId = `${id}-sprite-icons`;
  const { copy } = useCopyToClipboard();

  // Labels are HTML so they use the app font; MapLibre can only draw glyphs from a font server
  const syncLabels = (map: ReturnType<NonNullable<MapRef['getMap']>>) => {
    const scale = baseZoom == null ? 1 : 2 ** (map.getZoom() - baseZoom);
    const points = spriteIds.map((_, i) => {
      const point = map.project([(i % cols) * spacing, -Math.floor(i / cols) * spacing]);
      return { x: point.x, y: point.y };
    });
    setLabelFrame({
      cell: points.length > 1 ? Math.abs(points[1].x - points[0].x) : 96,
      points,
      scale,
    });
  };

  const handleMapClick = (e: MapLayerMouseEvent) => {
    const features = e.target.queryRenderedFeatures(e.point, { layers: [layerId] });
    const iconName = features?.[0]?.properties?.icon;
    if (iconName) {
      copy(iconName, `Sprite ID "${iconName}" copied to clipboard`);
    }
  };

  // Use a wider grid to fill the map's landscape aspect ratio (~1.8:1)
  const cols =
    spriteIds.length <= 6
      ? Math.max(1, spriteIds.length)
      : Math.ceil(Math.sqrt(spriteIds.length * 1.8));
  const rows = Math.ceil(spriteIds.length / cols);
  const spacing = 0.0004;
  // Frame at least a 3x2 cell area so a handful of icons stay grouped in the middle
  const cx = ((cols - 1) * spacing) / 2;
  const cy = (-(rows - 1) * spacing) / 2;
  const halfW = (Math.max(cols, 2) * spacing) / 2;
  const halfH = (Math.max(rows, 1.5) * spacing) / 2;

  const geojson = {
    features: spriteIds.map((spriteId, i) => ({
      geometry: {
        coordinates: [(i % cols) * spacing, -Math.floor(i / cols) * spacing],
        type: 'Point' as const,
      },
      properties: { icon: spriteId },
      type: 'Feature' as const,
    })),
    type: 'FeatureCollection' as const,
  };

  const spriteUrl =
    mode === 'sdf'
      ? buildMartinUrl(`/sdf_sprite/${spriteName}`)
      : buildMartinUrl(`/sprite/${spriteName}`);

  const mapStyle: StyleSpecification = {
    layers: [
      {
        id: 'bg',
        paint: { 'background-color': canvasTone === 'light' ? '#f3f3fa' : '#16172e' },
        type: 'background',
      },
    ],
    sources: {},
    sprite: spriteUrl,
    version: 8,
  };

  const layerLayout = {
    'icon-allow-overlap': true,
    'icon-image': ['get', 'icon'] as ExpressionSpecification,
    'icon-padding': 0,
    'icon-size':
      baseZoom != null
        ? ([
            'interpolate',
            ['exponential', 2],
            ['zoom'],
            baseZoom,
            iconSize,
            baseZoom + 10,
            iconSize * 1024,
          ] as unknown as number)
        : iconSize,
  };

  const layerPaint = {
    ...(mode === 'sdf' && {
      'icon-color': iconColor,
      'icon-halo-color': haloColor,
      'icon-halo-width': haloWidth,
    }),
  };

  if (spriteIds.length === 0) {
    return <div className="p-4 text-center text-muted-foreground">No sprites to display.</div>;
  }

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="flex flex-col gap-3 rounded-lg border bg-card p-3">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-3">
          <SegmentedControl
            label="Sprite format"
            onChange={setMode}
            options={[
              { label: 'PNG', value: 'png' },
              { label: 'SDF', value: 'sdf' },
            ]}
            value={mode}
          />
          <SegmentedControl
            label="Canvas background"
            onChange={setCanvasTone}
            options={[
              { label: 'Light', value: 'light' },
              { label: 'Dark', value: 'dark' },
            ]}
            value={canvasTone}
          />
          {mode === 'sdf' && (
            <div className="flex items-center gap-4">
              <label className="flex items-center gap-2 text-sm">
                <span className="text-muted-foreground">Color</span>
                <input
                  className="h-10 w-11 cursor-pointer rounded-md border bg-card p-0.5 sm:h-9"
                  onChange={(e) => setIconColor(e.target.value)}
                  type="color"
                  value={iconColor}
                />
              </label>
              <label className="flex items-center gap-2 text-sm">
                <span className="text-muted-foreground">Halo</span>
                <input
                  className="h-10 w-11 cursor-pointer rounded-md border bg-card p-0.5 sm:h-9"
                  onChange={(e) => setHaloColor(e.target.value)}
                  type="color"
                  value={haloColor}
                />
              </label>
            </div>
          )}
        </div>

        <div className="grid gap-3 sm:grid-cols-2 sm:gap-x-8">
          <label className="grid grid-cols-[5.5rem_minmax(0,1fr)_2.75rem] items-center gap-3 text-sm">
            <span className="whitespace-nowrap text-muted-foreground">Size</span>
            <input
              className="min-w-0 accent-primary"
              max="4"
              min="0.1"
              onChange={(e) => setIconSize(Number(e.target.value))}
              step="0.1"
              type="range"
              value={iconSize}
            />
            <output className="font-mono text-sm tabular-nums">{iconSize}x</output>
          </label>
          {mode === 'sdf' && (
            <label className="grid grid-cols-[5.5rem_minmax(0,1fr)_2.75rem] items-center gap-3 text-sm">
              <span className="whitespace-nowrap text-muted-foreground">Halo width</span>
              <input
                className="min-w-0 accent-primary"
                max="5"
                min="0"
                onChange={(e) => setHaloWidth(Number(e.target.value))}
                step="0.1"
                type="range"
                value={haloWidth}
              />
              <output className="font-mono text-sm tabular-nums">{haloWidth}</output>
            </label>
          )}
        </div>
      </div>

      <div className="relative">
        <MapLibreMap
          attributionControl={false}
          initialViewState={{ latitude: 0, longitude: 0, zoom: 15 }}
          interactiveLayerIds={[layerId]}
          mapStyle={mapStyle}
          maxBounds={[
            cx - halfW - spacing,
            cy - halfH - spacing,
            cx + halfW + spacing,
            cy + halfH + spacing,
          ]}
          onClick={handleMapClick}
          onLoad={(e) => {
            const map = e.target;
            map.fitBounds(
              [
                [cx - halfW, cy - halfH],
                [cx + halfW, cy + halfH],
              ],
              { animate: false, padding: { bottom: 40, left: 24, right: 24, top: 24 } },
            );
            const zoom = map.getZoom();
            map.setMinZoom(zoom);
            map.getCanvas().style.cursor = 'grab';
            setBaseZoom(zoom);
            syncLabels(map);
          }}
          onMouseEnter={() => {
            const canvas = mapRef.current?.getMap()?.getCanvas();
            if (canvas) canvas.style.cursor = 'pointer';
          }}
          onMouseLeave={() => {
            const canvas = mapRef.current?.getMap()?.getCanvas();
            if (canvas) canvas.style.cursor = 'grab';
          }}
          onMove={(e) => syncLabels(e.target)}
          pixelRatio={2}
          ref={mapRef}
          style={{
            borderRadius: 'var(--radius)',
            height: `min(${Math.min(24, 8 + rows * 5)}rem, 55dvh)`,
            minHeight: '11rem',
            width: '100%',
          }}
        >
          <Source data={geojson} id={`${id}-sprites`} type="geojson">
            <Layer id={layerId} layout={layerLayout} paint={layerPaint} type="symbol" />
          </Source>
        </MapLibreMap>
        <div aria-hidden="true" className="pointer-events-none absolute inset-0 overflow-hidden">
          {labelFrame?.points.map((point, i) => (
            <span
              className={`absolute -translate-x-1/2 text-center text-[0.8125rem] leading-tight [overflow-wrap:anywhere] ${canvasTone === 'light' ? 'text-[#4a4a66]' : 'text-[#c8c9e6]'}`}
              key={spriteIds[i]}
              style={{
                left: point.x,
                top: point.y + (iconSize * 12 + 6) * labelFrame.scale,
                width: Math.min(112, Math.max(48, labelFrame.cell - 8)),
              }}
            >
              <Breakable text={spriteIds[i]} />
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
