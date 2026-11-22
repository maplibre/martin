import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import {
  ArrowLeft,
  Copy,
  Download,
  ExternalLink,
  History,
  Monitor,
  Redo2,
  RotateCcw,
  Undo2,
} from 'lucide-react';
import type { Map as MapLibre } from 'maplibre-gl';
import { type ComponentProps, useEffect, useState } from 'react';
import { LoadingSpinner } from '@/components/loading/loading-spinner';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { buildMartinUrl } from '@/lib/api';
import type { Catalog } from '@/lib/types.gen';
import { cn } from '@/lib/utils';
import { Color, colorKey, writeColorSites } from './colors';
import { LayerInspector } from './inspector/layer-inspector';
import { LayerList } from './layer-list';
import { MapCanvas } from './map-canvas';
import {
  addLayer,
  deleteLayer,
  duplicateLayer,
  moveLayer,
  renameLayer,
  setFilter,
  setLayerSource,
  setProperty,
  setSource,
  setVisibility,
  setZoomRange,
} from './operations';
import { ChecksPanel } from './palette/checks-panel';
import {
  type ContrastFix,
  type LabelIssue,
  labelIssues,
  paletteTokens,
  type Token,
} from './palette/model';
import { PalettePanel } from './palette/palette-panel';
import { useSourceSchema } from './source-schema';
import { StylePaletteProvider } from './style-palette';
import { collectFonts, StyleResourcesProvider, useMartinCatalog } from './style-resources';
import { StyleSettings } from './style-settings/style-settings';
import { validateStyle } from './style-spec';
import { type StyleDocument, useStyleDocument } from './use-style-document';

interface StyleEditorProps {
  /** Catalog id of the style. */
  styleName: string;
  /** Catalog entry of the style. */
  style: Catalog['styles'][string];
  /** Leaves the editor. */
  onClose: () => void;
}

const VISION = {
  deuteranopia: '0.625 0.375 0 0 0 0.7 0.3 0 0 0 0 0.3 0.7 0 0 0 0 0 1 0',
  protanopia: '0.567 0.433 0 0 0 0.558 0.442 0 0 0 0 0.242 0.758 0 0 0 0 0 1 0',
  tritanopia: '0.95 0.05 0 0 0 0 0.433 0.567 0 0 0 0.475 0.525 0 0 0 0 0 1 0',
} as const;
type Vision = keyof typeof VISION | 'normal' | 'grayscale';
const VISIONS: Vision[] = ['normal', 'protanopia', 'deuteranopia', 'tritanopia', 'grayscale'];

type Tab = 'style' | 'layer' | 'palette' | 'checks';

/** Full-screen style editor: layers left, map centre, inspector right; edits stay in memory until downloaded. */
export function StyleEditor({ styleName, style: entry, onClose }: StyleEditorProps) {
  const styleUrl = buildMartinUrl(`/style/${styleName}`);
  const doc = useStyleDocument(styleUrl);
  const [smallScreen] = useState(() => window.matchMedia?.('(max-width: 767px)').matches ?? false);
  const [proceed, setProceed] = useState(false);

  const maputnikUrl = new URL('https://maplibre.org/maputnik/');
  maputnikUrl.searchParams.set('style', styleUrl);

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-background text-foreground">
      <Header
        doc={doc}
        maputnikUrl={maputnikUrl.toString()}
        onClose={onClose}
        path={entry.path}
        styleName={styleName}
      />
      <div className="relative flex min-h-0 flex-1">
        {smallScreen && !proceed ? (
          <SmallScreenGate onClose={onClose} onProceed={() => setProceed(true)} />
        ) : doc.error ? (
          <div
            className="flex flex-1 items-center justify-center p-6 text-sm text-destructive"
            role="alert"
          >
            Could not load {styleName}: {doc.error}
          </div>
        ) : doc.style ? (
          <Workspace doc={doc} style={doc.style} />
        ) : (
          <div
            className="flex flex-1 items-center justify-center gap-3 text-sm text-muted-foreground"
            role="status"
          >
            <LoadingSpinner /> Loading style…
          </div>
        )}
      </div>
    </div>
  );
}

function Workspace({ doc, style }: { doc: StyleDocument; style: StyleSpecification }) {
  const [map, setMap] = useState<MapLibre | null>(null);
  const [zoom, setZoom] = useState(0);
  const [tab, setTab] = useState<Tab>('style');
  const [layerId, setLayerId] = useState<string | null>(null);
  const [tokenKey, setTokenKey] = useState<string | null>(null);
  const [highlight, setHighlight] = useState(false);
  const [vision, setVision] = useState<Vision>('normal');
  const [pick, setPick] = useState<{ x: number; y: number; layers: string[] } | null>(null);
  const schema = useSourceSchema(style, map);
  const martin = useMartinCatalog();

  const tokens = paletteTokens(style);
  const token = tokens.find((t) => t.key === tokenKey);
  const checks = labelIssues(style).length + validateStyle(style).length;
  const layer = style.layers.find((l) => l.id === layerId);
  const attributes =
    layer && 'source' in layer
      ? schema.attributes(layer.source, 'source-layer' in layer ? layer['source-layer'] : undefined)
      : [];

  const mapStyle = (() => {
    if (!highlight || !token || tab !== 'palette') return style;
    const accent =
      Color.parse(
        getComputedStyle(document.documentElement).getPropertyValue('--editor-accent').trim(),
      ) ?? Color.red;
    const preview = structuredClone(style);
    writeColorSites(preview, token.sites, () => accent);
    return preview;
  })();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target?.closest('input, textarea, select, [contenteditable]')) return;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        if (e.shiftKey) doc.redo();
        else doc.undo();
      } else if (mod && e.key.toLowerCase() === 'y') {
        e.preventDefault();
        doc.redo();
      } else if (e.key === 'Escape') setPick(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const selectLayer = (id: string) => {
    setLayerId(id);
    setTab('layer');
    setPick(null);
  };
  const selectToken = (key: string | null) => {
    setTokenKey(key);
    if (key) setTab('palette');
  };

  const onReady = (m: MapLibre) => {
    setMap(m);
    setZoom(m.getZoom());
    m.on('zoom', () => setZoom(m.getZoom()));
    m.on('movestart', () => setPick(null));
    m.on('click', (e) => {
      const { x, y } = e.point;
      const ids = [
        ...new Set(
          m
            .queryRenderedFeatures([
              [x - 4, y - 4],
              [x + 4, y + 4],
            ])
            .map((f) => f.layer.id),
        ),
      ];
      if (ids.length === 1) selectLayer(ids[0]);
      else setPick(ids.length ? { layers: ids, x, y } : null);
    });
    m.on('mousemove', (e) => {
      m.getCanvas().style.cursor = m.queryRenderedFeatures(e.point).length ? 'pointer' : '';
    });
  };

  const recolor = (t: Token, next: Color, coalesce?: string) => {
    const sites = t.sites.map((x) => `${x.layer}/${x.name}/${x.path.join('.')}`).join('|');
    doc.apply(
      `Recolor ${t.sites.length} uses of ${t.key}`,
      (s) => writeColorSites(s, t.sites, () => next),
      `token:${sites}:${coalesce ?? 'edit'}`,
    );
    setTokenKey(colorKey(next));
  };

  const fixContrast = (issue: LabelIssue, fix: ContrastFix) =>
    doc.apply(`${fix.label} on ${issue.layer}`, (s) => {
      for (const change of fix.changes) {
        setProperty(s, { group: 'paint', layer: issue.layer, name: change.name }, change.value);
      }
    });

  const filter =
    vision === 'normal'
      ? undefined
      : vision === 'grayscale'
        ? 'grayscale(1)'
        : `url(#se-vision-${vision})`;

  return (
    <StylePaletteProvider
      colors={tokens.map((t) => ({ color: t.color, key: t.key, uses: t.sites.length }))}
    >
      <StyleResourcesProvider
        value={{
          fontsInStyle: collectFonts(style),
          glyphs: style.glyphs,
          martinFonts: martin.fonts,
          martinSources: martin.tiles,
          martinSprites: martin.sprites,
          sourceLayers: schema.sourceLayers,
          sources: style.sources,
          sprite: style.sprite,
        }}
      >
        <svg aria-hidden="true" className="absolute size-0">
          {Object.entries(VISION).map(([k, m]) => (
            <filter id={`se-vision-${k}`} key={k}>
              <feColorMatrix type="matrix" values={m} />
            </filter>
          ))}
        </svg>

        <LayerList
          highlighted={token && tab === 'palette' ? token.layers : new Set()}
          onAddLayer={({ layer: added, source }) => {
            const at = layerId
              ? style.layers.findIndex((l) => l.id === layerId) + 1
              : style.layers.length;
            doc.apply(`Add layer ${added.id}`, (s) => {
              if (source) setSource(s, source.id, source.spec);
              addLayer(s, added, at);
            });
            selectLayer(added.id);
          }}
          onDelete={(id) => {
            doc.apply(`Delete ${id}`, (s) => deleteLayer(s, id));
            if (id === layerId) setLayerId(null);
          }}
          onDuplicate={(id) => {
            let copy: string | undefined;
            doc.apply(`Duplicate ${id}`, (s) => {
              copy = duplicateLayer(s, id);
            });
            if (copy) selectLayer(copy);
          }}
          onMove={(id, to) => doc.apply(`Move ${id}`, (s) => moveLayer(s, id, to))}
          onSelect={selectLayer}
          onVisibility={(ids, visible) =>
            doc.apply(
              `${visible ? 'Show' : 'Hide'} ${ids.length === 1 ? ids[0] : `${ids.length} layers`}`,
              (s) => setVisibility(s, ids, visible),
            )
          }
          selected={layerId}
          style={style}
        />

        <main className="relative min-w-0 flex-1 overflow-hidden">
          <MapCanvas filter={filter} onReady={onReady} style={mapStyle} />
          <div className="pointer-events-none absolute inset-x-3 top-3 flex items-start justify-between gap-3">
            <fieldset className="pointer-events-auto flex flex-wrap items-center gap-0.5 rounded-lg border bg-card/95 p-0.5 text-[11px] shadow-sm backdrop-blur">
              <legend className="sr-only">Color vision simulation</legend>
              {VISIONS.map((v) => (
                <button
                  aria-pressed={vision === v}
                  className={cn(
                    'h-6 rounded-md px-2 capitalize transition-colors',
                    vision === v
                      ? 'bg-editor-accent text-white'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                  key={v}
                  onClick={() => setVision(v)}
                  type="button"
                >
                  {v}
                </button>
              ))}
            </fieldset>
            <span className="rounded-md border bg-card/95 px-2 py-1 font-mono text-[11px] text-muted-foreground tabular-nums shadow-sm">
              z{zoom.toFixed(1)}
            </span>
          </div>
          {highlight && token && tab === 'palette' && (
            <div className="absolute bottom-3 left-1/2 -translate-x-1/2 rounded-full bg-editor-accent px-3 py-1 text-[11px] font-medium text-white shadow">
              Showing {token.sites.length} uses of {token.key}
            </div>
          )}
          {pick ? (
            <PickMenu layers={pick.layers} onSelect={selectLayer} x={pick.x} y={pick.y} />
          ) : null}
        </main>

        <aside className="flex w-[380px] shrink-0 flex-col border-l bg-card">
          <Tabs
            className="flex min-h-0 flex-1 flex-col"
            onValueChange={(v) => setTab(v as Tab)}
            value={tab}
          >
            <div className="border-b p-2">
              <TabsList className="grid h-9 w-full grid-cols-4 sm:h-9">
                <TabsTrigger
                  className="text-xs outline-none focus-visible:ring-1 focus-visible:ring-editor-accent sm:px-2"
                  value="style"
                >
                  Style
                </TabsTrigger>
                <TabsTrigger
                  className="text-xs outline-none focus-visible:ring-1 focus-visible:ring-editor-accent sm:px-2"
                  value="layer"
                >
                  Layer
                </TabsTrigger>
                <TabsTrigger
                  className="gap-1.5 text-xs outline-none focus-visible:ring-1 focus-visible:ring-editor-accent sm:px-2"
                  value="palette"
                >
                  Palette <Badge n={tokens.length} />
                </TabsTrigger>
                <TabsTrigger
                  className="gap-1.5 text-xs outline-none focus-visible:ring-1 focus-visible:ring-editor-accent sm:px-2"
                  value="checks"
                >
                  Checks <Badge alert={checks > 0} n={checks} />
                </TabsTrigger>
              </TabsList>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden">
              <TabsContent className="mt-0" value="layer">
                {layer ? (
                  <LayerInspector
                    attributes={attributes}
                    key={layer.id}
                    layer={layer}
                    onFilter={(filter) =>
                      doc.apply(
                        `${layer.id} · filter`,
                        (s) => setFilter(s, layer.id, filter),
                        `${layer.id}:filter`,
                      )
                    }
                    onRename={(next) => {
                      if (!next || style.layers.some((l) => l.id === next)) return false;
                      doc.apply(`Rename ${layer.id} to ${next}`, (s) =>
                        renameLayer(s, layer.id, next),
                      );
                      setLayerId(next);
                      return true;
                    }}
                    onSelectColor={selectToken}
                    onSet={(ref, value, coalesce) =>
                      doc.apply(
                        `${ref.layer} · ${ref.name}${value === undefined ? ' reset' : ''}`,
                        (s) => setProperty(s, ref, value),
                        coalesce && `${ref.layer}:${ref.name}:${coalesce}`,
                      )
                    }
                    onSource={(source, sourceLayer) =>
                      doc.apply(
                        `${layer.id} · source ${source}${sourceLayer ? ` / ${sourceLayer}` : ''}`,
                        (s) => setLayerSource(s, layer.id, source, sourceLayer),
                      )
                    }
                    onZoomRange={(min, max) =>
                      doc.apply(
                        `${layer.id} · zoom range`,
                        (s) => setZoomRange(s, layer.id, min, max),
                        `${layer.id}:zoom-range`,
                      )
                    }
                    zoom={zoom}
                  />
                ) : (
                  <p className="p-4 text-xs text-muted-foreground">
                    Select a layer in the list, or click a feature on the map.
                  </p>
                )}
              </TabsContent>
              <TabsContent className="mt-0" value="style">
                <StyleSettings apply={doc.apply} map={map} style={style} />
              </TabsContent>
              <TabsContent className="mt-0" value="palette">
                <PalettePanel
                  highlight={highlight}
                  onHighlight={setHighlight}
                  onMerge={(group) => {
                    const [target, ...rest] = group;
                    doc.apply(`Merge ${rest.length} colors into ${target.key}`, (s) =>
                      writeColorSites(
                        s,
                        rest.flatMap((t) => t.sites),
                        () => target.color,
                      ),
                    );
                  }}
                  onRecolor={recolor}
                  onSelect={selectToken}
                  onSelectLayer={selectLayer}
                  selected={tokenKey}
                  style={style}
                />
              </TabsContent>
              <TabsContent className="mt-0" value="checks">
                <ChecksPanel onFix={fixContrast} onSelectLayer={selectLayer} style={style} />
              </TabsContent>
            </div>
          </Tabs>
        </aside>
      </StyleResourcesProvider>
    </StylePaletteProvider>
  );
}

function Badge({ n, alert }: { n: number; alert?: boolean }) {
  return (
    <span
      className={cn(
        'rounded-full px-1.5 text-[10px] leading-4 tabular-nums',
        alert ? 'bg-editor-accent text-white' : 'bg-muted-foreground/15 text-muted-foreground',
      )}
    >
      {n}
    </span>
  );
}

function PickMenu({
  layers,
  x,
  y,
  onSelect,
}: {
  layers: string[];
  x: number;
  y: number;
  onSelect: (id: string) => void;
}) {
  return (
    <div
      className="absolute z-10 w-60 rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg"
      style={{ left: x + 8, top: y + 8 }}
    >
      <p className="px-2 py-1 text-[11px] text-muted-foreground">
        {layers.length} layers here, topmost first
      </p>
      {layers.slice(0, 10).map((id) => (
        <button
          className="block w-full truncate rounded px-2 py-1 text-left font-mono text-[11px] hover:bg-muted"
          key={id}
          onClick={() => onSelect(id)}
          title={id}
          type="button"
        >
          {id}
        </button>
      ))}
    </div>
  );
}

interface HeaderProps {
  doc: StyleDocument;
  styleName: string;
  path: string;
  maputnikUrl: string;
  onClose: () => void;
}

function Header({ doc, styleName, path, maputnikUrl, onClose }: HeaderProps) {
  const [historyOpen, setHistoryOpen] = useState(false);
  const { copy } = useCopyToClipboard({ successMessage: 'Style JSON copied' });
  const json = () => JSON.stringify(doc.style, null, 2);
  const download = () => {
    const url = URL.createObjectURL(new Blob([json()], { type: 'application/json' }));
    const a = Object.assign(document.createElement('a'), {
      download: `${styleName}.json`,
      href: url,
    });
    a.click();
    URL.revokeObjectURL(url);
  };
  const icon =
    'flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40';

  return (
    <header className="relative flex h-12 shrink-0 items-center gap-2 border-b bg-card px-2 sm:px-3">
      <Button className="shrink-0" onClick={onClose} size="sm" variant="ghost">
        <ArrowLeft aria-hidden="true" />
        <span className="sr-only sm:not-sr-only">Catalog</span>
      </Button>
      <span className="h-5 w-px shrink-0 bg-border" />
      <div className="flex min-w-0 flex-1 items-baseline gap-2">
        <h2 className="truncate font-mono text-sm font-semibold" title={styleName}>
          {styleName}
        </h2>
        <span
          className="hidden truncate font-mono text-[11px] text-muted-foreground lg:inline"
          title={path}
        >
          {path.split(/[\\/]/).pop()}
        </span>
      </div>
      <button
        aria-expanded={historyOpen}
        className="flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
        onClick={() => setHistoryOpen((o) => !o)}
        type="button"
      >
        <History className="size-3.5" />
        <span className="tabular-nums">{doc.edits.length}</span>
        <span className="hidden sm:inline">{doc.edits.length === 1 ? 'edit' : 'edits'}</span>
      </button>
      <IconButton className={icon} disabled={!doc.canUndo} label="Undo (Ctrl+Z)" onClick={doc.undo}>
        <Undo2 className="size-4" />
      </IconButton>
      <IconButton
        className={icon}
        disabled={!doc.canRedo}
        label="Redo (Ctrl+Shift+Z)"
        onClick={doc.redo}
      >
        <Redo2 className="size-4" />
      </IconButton>
      <IconButton
        className={icon}
        disabled={!doc.canUndo}
        label="Discard all edits"
        onClick={doc.reset}
      >
        <RotateCcw className="size-4" />
      </IconButton>
      <span className="h-5 w-px shrink-0 bg-border" />
      <IconButton
        className={icon}
        label="Open in Maputnik"
        onClick={() => window.open(maputnikUrl, '_blank', 'noreferrer')}
      >
        <ExternalLink className="size-4" />
      </IconButton>
      <IconButton
        className={icon}
        disabled={!doc.style}
        label="Copy style JSON"
        onClick={() => copy(json())}
      >
        <Copy className="size-4" />
      </IconButton>
      <button
        className="flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-editor-accent px-3 text-xs font-medium text-white hover:opacity-90 disabled:opacity-40"
        disabled={!doc.style}
        onClick={download}
        type="button"
      >
        <Download className="size-3.5" />
        <span className="hidden sm:inline">Download</span>
      </button>
      {historyOpen ? (
        <ol className="absolute top-full right-3 z-20 mt-1 max-h-80 w-96 max-w-[calc(100vw-1.5rem)] overflow-auto rounded-lg border bg-popover p-1 text-xs shadow-lg">
          {doc.edits.length === 0 ? (
            <li className="p-2 text-muted-foreground">No edits yet</li>
          ) : null}
          {doc.edits.map((e) => (
            <li
              className="truncate rounded px-2 py-1.5 font-mono text-[11px] odd:bg-muted/50"
              key={e.id}
              title={e.label}
            >
              {e.label}
            </li>
          ))}
        </ol>
      ) : null}
    </header>
  );
}

function IconButton({ label, children, ...props }: ComponentProps<'button'> & { label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button aria-label={label} type="button" {...props}>
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function SmallScreenGate({ onProceed, onClose }: { onProceed: () => void; onClose: () => void }) {
  return (
    <div className="flex flex-1 items-center justify-center p-4">
      <div className="w-full max-w-sm rounded-xl border bg-card p-6 text-center shadow-card">
        <Monitor aria-hidden="true" className="mx-auto mb-4 size-10 text-editor-accent" />
        <h3 className="mb-2 text-lg font-semibold">Editing works best on a larger screen</h3>
        <p className="mb-5 text-sm text-muted-foreground">
          The style editor shows a layer list, the map and an inspector side by side. Open this page
          on a tablet or computer.
        </p>
        <div className="flex flex-col gap-2">
          <Button onClick={onProceed}>Continue anyway</Button>
          <Button onClick={onClose} variant="outline">
            Back to Catalog
          </Button>
        </div>
      </div>
    </div>
  );
}
