import { Layer, Map as MapLibreMap, Source } from '@vis.gl/react-maplibre';
import type { ExpressionSpecification, StyleSpecification } from 'maplibre-gl';
import { useId, useState } from 'react';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { buildMartinUrl } from '@/lib/api';
import 'maplibre-gl/dist/maplibre-gl.css';

type SpriteMapPreviewProps = {
  spriteName: string;
  spriteIds: readonly string[];
};

export function SpriteMapPreview({ spriteName, spriteIds }: SpriteMapPreviewProps) {
  const [mode, setMode] = useState<'png' | 'sdf'>('sdf');
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
  const [cursor, setCursor] = useState('grab');
  const id = useId();
  const layerId = `${id}-sprite-icons`;
  const { copy } = useCopyToClipboard();

  const handleMapClick = (e: maplibregl.MapLayerMouseEvent) => {
    const features = e.target.queryRenderedFeatures(e.point, { layers: [layerId] });
    const iconName = features?.[0]?.properties?.icon;
    if (iconName) {
      copy(iconName, `Sprite ID "${iconName}" copied to clipboard`);
    }
  };

  // Use a wider grid to fill the map's landscape aspect ratio (~1.8:1)
  const cols = Math.max(1, Math.ceil(Math.sqrt(spriteIds.length * 1.8)));
  const rows = Math.ceil(spriteIds.length / cols);
  const spacing = 0.0004;

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
    glyphs: 'https://demotiles.maplibre.org/font/{fontstack}/{range}.pbf',
    layers: [
      {
        id: 'bg',
        paint: { 'background-color': '#f8f9fa' },
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
    'text-allow-overlap': true,
    'text-anchor': 'top' as const,
    'text-field': ['get', 'icon'] as ExpressionSpecification,
    'text-font': ['Open Sans Regular'],
    'text-max-width': 6,
    'text-offset': [0, iconSize + 0.5] as [number, number],
    'text-size':
      baseZoom != null
        ? ([
            'interpolate',
            ['exponential', 2],
            ['zoom'],
            baseZoom,
            10,
            baseZoom + 10,
            10240,
          ] as unknown as number)
        : 10,
  };

  const layerPaint = {
    'text-color': '#666666',
    'text-halo-color': '#ffffff',
    'text-halo-width': 1,
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
    <div className="space-y-3">
      <div className="flex flex-wrap items-end gap-4">
        <Tabs onValueChange={(v) => setMode(v as 'png' | 'sdf')} value={mode}>
          <TabsList>
            <TabsTrigger value="png">PNG</TabsTrigger>
            <TabsTrigger value="sdf">SDF</TabsTrigger>
          </TabsList>
        </Tabs>

        <label className="flex flex-col gap-1 text-sm">
          <span className="text-muted-foreground">Size</span>
          <div className="flex items-center gap-2">
            <input
              className="w-24 accent-purple-600"
              max="4"
              min="0.1"
              onChange={(e) => setIconSize(Number(e.target.value))}
              step="0.1"
              type="range"
              value={iconSize}
            />
            <span className="w-10 text-right font-mono">{iconSize}x</span>
          </div>
        </label>

        {mode === 'sdf' && (
          <>
            <label className="flex flex-col gap-1 text-sm">
              <span className="text-muted-foreground">Color</span>
              <input
                className="h-8 w-10 cursor-pointer rounded border"
                onChange={(e) => setIconColor(e.target.value)}
                type="color"
                value={iconColor}
              />
            </label>

            <label className="flex flex-col gap-1 text-sm">
              <span className="text-muted-foreground">Halo Color</span>
              <input
                className="h-8 w-10 cursor-pointer rounded border"
                onChange={(e) => setHaloColor(e.target.value)}
                type="color"
                value={haloColor}
              />
            </label>

            <label className="flex flex-col gap-1 text-sm">
              <span className="text-muted-foreground">Halo Width</span>
              <div className="flex items-center gap-2">
                <input
                  className="w-20 accent-purple-600"
                  max="5"
                  min="0"
                  onChange={(e) => setHaloWidth(Number(e.target.value))}
                  step="0.1"
                  type="range"
                  value={haloWidth}
                />
                <span className="w-6 text-right font-mono">{haloWidth}</span>
              </div>
            </label>
          </>
        )}
      </div>

      <MapLibreMap
        attributionControl={false}
        cursor={cursor}
        initialViewState={{ latitude: 0, longitude: 0, zoom: 15 }}
        interactiveLayerIds={[layerId]}
        mapStyle={mapStyle}
        maxBounds={[
          [-spacing, -(rows - 1) * spacing - spacing],
          [(cols - 1) * spacing + spacing, spacing],
        ]}
        onClick={handleMapClick}
        onLoad={(e) => {
          const map = e.target;
          map.fitBounds(
            [
              [0, -(rows - 1) * spacing],
              [(cols - 1) * spacing, 0],
            ],
            { animate: false, padding: { bottom: 30, left: 10, right: 10, top: 30 } },
          );
          const zoom = map.getZoom();
          map.setMinZoom(zoom);
          setBaseZoom(zoom);
        }}
        onMouseEnter={() => setCursor('pointer')}
        onMouseLeave={() => setCursor('grab')}
        pixelRatio={2}
        style={{ borderRadius: 'var(--radius)', height: '28rem', width: '100%' }}
      >
        <Source data={geojson} id={`${id}-sprites`} type="geojson">
          <Layer id={layerId} layout={layerLayout} paint={layerPaint} type="symbol" />
        </Source>
      </MapLibreMap>
    </div>
  );
}
