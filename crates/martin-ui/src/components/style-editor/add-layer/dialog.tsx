import type { SourceSpecification, StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import {
  Box,
  Circle,
  Flame,
  Image,
  Layers,
  type LucideIcon,
  Mountain,
  PaintBucket,
  Spline,
  Square,
  Type,
  X,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';
import type { AddLayerRequest } from '../layer-list';
import { uniqueLayerId } from '../operations';
import { useStyleResources } from '../style-resources';
import { type LayerType, layerTypesFor, usesSourceLayer } from '../style-spec';
import {
  buildLayer,
  parseVectorLayers,
  type SourceLayerInfo,
  TYPE_INFO,
  uniqueSourceId,
  validateNewLayer,
} from './model';

const TYPE_ICONS: Record<LayerType, LucideIcon> = {
  background: PaintBucket,
  circle: Circle,
  'color-relief': Layers,
  fill: Square,
  'fill-extrusion': Box,
  heatmap: Flame,
  hillshade: Mountain,
  line: Spline,
  raster: Image,
  symbol: Type,
};

type Pick = { kind: 'none' } | { kind: 'style'; id: string } | { kind: 'martin'; id: string };

const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), [tabindex="0"]';

function sameSource(a: Pick, b: Pick): boolean {
  return a.kind === b.kind && (a.kind === 'none' || (b.kind !== 'none' && a.id === b.id));
}

function describeSource(spec: SourceSpecification): string {
  if ('url' in spec && spec.url) return spec.url;
  if ('tiles' in spec && spec.tiles?.[0]) return spec.tiles[0];
  return 'inline';
}

function useTileJsonLayers(url: string | undefined): SourceLayerInfo[] {
  const [loaded, setLoaded] = useState<{ url: string; layers: SourceLayerInfo[] } | null>(null);
  useEffect(() => {
    if (!url) return;
    const controller = new AbortController();
    fetch(url, { signal: controller.signal })
      .then((r) => (r.ok ? r.json() : undefined))
      .then((json) => setLoaded({ layers: parseVectorLayers(json), url }))
      .catch(() => undefined);
    return () => controller.abort();
  }, [url]);
  return loaded && loaded.url === url ? loaded.layers : [];
}

const SECTION_LABEL =
  'px-1 pb-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground';

function Option({
  selected,
  onClick,
  children,
  title,
}: {
  selected: boolean;
  onClick: () => void;
  children: React.ReactNode;
  title?: string;
}) {
  return (
    <button
      aria-pressed={selected}
      className={cn(
        'flex h-7 w-full items-center gap-1.5 rounded px-2 text-left text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
        selected ? 'bg-editor-accent-soft' : 'hover:bg-muted',
      )}
      onClick={onClick}
      title={title}
      type="button"
    >
      {children}
    </button>
  );
}

function Badge({ children }: { children: React.ReactNode }) {
  return (
    <span className="shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground">
      {children}
    </span>
  );
}

/** Modal that picks a source, source layer and type, then hands the complete new layer to `onAdd`. */
export function AddLayerDialog({
  style,
  onAdd,
  onClose,
}: {
  /** Style the layer is added to. */
  style: StyleSpecification;
  /** Receives the new layer and, for a Martin source not in the style, the source to add. */
  onAdd: (request: AddLayerRequest) => void;
  /** Closes the dialog without adding. */
  onClose: () => void;
}) {
  const resources = useStyleResources();
  const styleSources = Object.entries(style.sources);
  const inStyle = new Set(styleSources.map(([id]) => id));
  const martinOnly = resources.martinSources.filter((s) => !inStyle.has(s.id));

  const [pick, setPick] = useState<Pick>(() => {
    const vector = styleSources.find(([, s]) => s.type === 'vector') ?? styleSources[0];
    return vector ? { id: vector[0], kind: 'style' } : { kind: 'none' };
  });
  const [chosenLayer, setChosenLayer] = useState<{ source: Pick; name: string } | null>(null);
  const [chosenType, setChosenType] = useState<LayerType | null>(null);
  const [editedId, setEditedId] = useState<string | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const dialogRef = useRef<HTMLDivElement>(null);

  const martin = pick.kind === 'martin' ? martinOnly.find((s) => s.id === pick.id) : undefined;
  const spec: SourceSpecification | undefined =
    pick.kind === 'style' ? style.sources[pick.id] : martin?.spec;
  const sourceId = pick.kind === 'none' ? undefined : pick.id;
  const vector = usesSourceLayer(spec?.type);

  const tileJsonUrl =
    vector && spec && 'url' in spec && typeof spec.url === 'string' ? spec.url : undefined;
  const infos = useTileJsonLayers(tileJsonUrl);
  const layerNames = [
    ...new Set([
      ...(pick.kind === 'style' && vector ? resources.sourceLayers(pick.id) : []),
      ...infos.map((i) => i.id),
    ]),
  ];

  const sourceLayer = !vector
    ? undefined
    : chosenLayer && sameSource(chosenLayer.source, pick)
      ? chosenLayer.name
      : (layerNames[0] ?? '');
  const typedName = sourceLayer && !layerNames.includes(sourceLayer) ? sourceLayer : '';
  const shownLayerNames = typedName
    ? layerNames.filter((n) => n.toLowerCase().includes(typedName.toLowerCase()))
    : layerNames;

  const types = layerTypesFor(spec?.type);
  const type = chosenType && types.includes(chosenType) ? chosenType : types[0];

  const fields = infos.find((i) => i.id === sourceLayer)?.fields ?? {};
  const suggestedId = uniqueLayerId(
    style,
    `${sourceLayer || sourceId || 'layer'}-${type ?? 'layer'}`,
  );
  const id = editedId ?? suggestedId;
  const idError =
    id.trim() === ''
      ? 'Id is empty'
      : style.layers.some((l) => l.id === id)
        ? `A layer "${id}" already exists`
        : null;
  const needsLayer = vector && !sourceLayer;
  const canCreate =
    type !== undefined && !idError && !needsLayer && (pick.kind === 'none' || spec !== undefined);

  useEffect(() => {
    dialogRef.current?.focus();
  }, []);

  const create = () => {
    if (!type || !canCreate) return;
    const newSource =
      martin && sourceId ? { id: uniqueSourceId(style, sourceId), spec: martin.spec } : undefined;
    const layer = buildLayer(style, {
      fields,
      font: resources.fontsInStyle[0],
      id,
      source: newSource?.id ?? sourceId,
      sourceLayer: sourceLayer || undefined,
      type,
    });
    const problems = validateNewLayer(style, layer, newSource);
    if (problems.length > 0) {
      setErrors(problems);
      return;
    }
    onAdd(newSource ? { layer, source: newSource } : { layer });
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onClose();
    } else if (
      e.key === 'Enter' &&
      e.target instanceof HTMLInputElement &&
      e.target.name === 'layer-id'
    ) {
      e.preventDefault();
      create();
    } else if (e.key === 'Tab' && dialogRef.current) {
      const items = [...dialogRef.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === first || active === dialogRef.current)) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first?.focus();
      }
    }
  };

  const select = (next: Pick) => {
    setPick(next);
    setErrors([]);
  };

  const summary = !type
    ? 'No layer type draws this source'
    : `Adds ${/^[aeiou]/.test(type) ? 'an' : 'a'} ${type} layer${sourceLayer ? ` drawing ${sourceLayer} from ${sourceId}` : sourceId ? ` drawing ${sourceId}` : ''}`;

  return createPortal(
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop only closes on a click outside the dialog
    // biome-ignore lint/a11y/useKeyWithClickEvents: Escape is handled on the dialog
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onClick={onClose}
    >
      <div
        aria-label="Add layer"
        aria-modal="true"
        className="flex h-[min(640px,calc(100vh-4rem))] w-full max-w-[820px] flex-col rounded-lg border bg-popover text-xs text-popover-foreground shadow-xl focus-visible:outline-none"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
        ref={dialogRef}
        role="dialog"
        tabIndex={-1}
      >
        <div className="flex h-9 shrink-0 items-center justify-between border-b px-3">
          <h2 className="text-xs font-semibold">Add layer</h2>
          <button
            aria-label="Close"
            className="flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
            onClick={onClose}
            type="button"
          >
            <X className="size-3.5" />
          </button>
        </div>
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,5fr)_minmax(0,6fr)] overflow-hidden">
          <section aria-label="Source" className="min-h-0 overflow-y-auto border-r p-2">
            <h3 className={SECTION_LABEL}>Source</h3>
            {styleSources.map(([sid, s]) => (
              <Option
                key={sid}
                onClick={() => select({ id: sid, kind: 'style' })}
                selected={pick.kind === 'style' && pick.id === sid}
                title={`${sid}: ${describeSource(s)}`}
              >
                <span className="min-w-0 max-w-[45%] shrink-0 truncate font-mono">{sid}</span>
                <Badge>{s.type}</Badge>
                <span className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">
                  {describeSource(s)}
                </span>
              </Option>
            ))}
            <Option
              onClick={() => select({ kind: 'none' })}
              selected={pick.kind === 'none'}
              title="A background layer needs no source"
            >
              <span className="min-w-0 truncate">No source (background)</span>
            </Option>
            <h3 className={cn(SECTION_LABEL, 'mt-2')}>From this Martin server</h3>
            {martinOnly.map((s) => (
              <Option
                key={s.id}
                onClick={() => select({ id: s.id, kind: 'martin' })}
                selected={pick.kind === 'martin' && pick.id === s.id}
                title={`${s.name} (${s.id})`}
              >
                <span className="min-w-0 flex-1 truncate">{s.name}</span>
                {s.name !== s.id ? (
                  <span className="min-w-0 max-w-[45%] truncate font-mono text-[11px] text-muted-foreground">
                    {s.id}
                  </span>
                ) : null}
                <Badge>{s.spec.type}</Badge>
              </Option>
            ))}
            {martinOnly.length === 0 ? (
              <p className="px-2 py-1 text-[11px] text-muted-foreground">
                {resources.martinSources.length === 0
                  ? 'This Martin server serves no tile sources.'
                  : 'All tile sources of this Martin server are already in the style.'}
              </p>
            ) : null}
          </section>
          <div className="flex min-h-0 flex-col gap-3 overflow-y-auto p-2">
            {vector ? (
              <section aria-label="Source layer">
                <h3 className={SECTION_LABEL}>Source layer</h3>
                <input
                  aria-label="Source layer name"
                  className="mb-1 h-7 w-full rounded-md border bg-background px-2 font-mono text-xs placeholder:font-sans placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
                  onChange={(e) => setChosenLayer({ name: e.target.value, source: pick })}
                  placeholder={`Filter ${layerNames.length} source layers or type a name`}
                  value={typedName}
                />
                <div className="max-h-48 overflow-y-auto rounded-md border p-0.5">
                  {shownLayerNames.length === 0 ? (
                    <p className="px-2 py-1.5 text-[11px] text-muted-foreground">
                      {typedName
                        ? `No match; "${typedName}" is used as typed`
                        : 'No source layers known yet'}
                    </p>
                  ) : null}
                  {shownLayerNames.map((name) => (
                    <Option
                      key={name}
                      onClick={() => setChosenLayer({ name, source: pick })}
                      selected={sourceLayer === name}
                      title={name}
                    >
                      <span className="min-w-0 flex-1 truncate font-mono">{name}</span>
                    </Option>
                  ))}
                </div>
              </section>
            ) : null}
            <section aria-label="Layer type">
              <h3 className={SECTION_LABEL}>Layer type</h3>
              {types.length === 0 ? (
                <p className="px-1 text-[11px] text-muted-foreground">
                  No layer type draws this source.
                </p>
              ) : (
                <div className="grid grid-cols-2 gap-1">
                  {types.map((t) => {
                    const Icon = TYPE_ICONS[t];
                    return (
                      <button
                        aria-pressed={type === t}
                        className={cn(
                          'flex min-w-0 items-start gap-1.5 rounded border p-1.5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
                          type === t
                            ? 'border-editor-accent bg-editor-accent-soft'
                            : 'hover:bg-muted',
                        )}
                        key={t}
                        onClick={() => setChosenType(t)}
                        title={`${TYPE_INFO[t].label}: ${TYPE_INFO[t].description}`}
                        type="button"
                      >
                        <Icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                        <span className="min-w-0">
                          <span className="block truncate font-medium">{TYPE_INFO[t].label}</span>
                          <span className="block text-[11px] leading-tight text-muted-foreground">
                            {TYPE_INFO[t].description}
                          </span>
                        </span>
                      </button>
                    );
                  })}
                </div>
              )}
            </section>
            <section aria-label="Layer id">
              <h3 className={SECTION_LABEL}>Id</h3>
              <input
                aria-invalid={idError !== null}
                aria-label="Layer id"
                className={cn(
                  'h-7 w-full rounded-md border bg-background px-2 font-mono text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
                  idError && 'border-destructive',
                )}
                name="layer-id"
                onChange={(e) => {
                  setEditedId(e.target.value);
                  setErrors([]);
                }}
                value={id}
              />
              {idError ? <p className="px-1 pt-1 text-[11px] text-destructive">{idError}</p> : null}
            </section>
          </div>
        </div>
        {errors.length > 0 ? (
          <ul
            className="max-h-20 shrink-0 overflow-y-auto border-t bg-destructive/10 px-3 py-1.5 text-[11px] text-destructive"
            role="alert"
          >
            {errors.map((m) => (
              <li key={m}>{m}</li>
            ))}
          </ul>
        ) : null}
        <div className="flex h-11 shrink-0 items-center gap-2 border-t px-3">
          <p className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground" title={summary}>
            {type ? (
              <>
                Adds {/^[aeiou]/.test(type) ? 'an' : 'a'} {type} layer
                {sourceLayer ? (
                  <>
                    {' drawing '}
                    <code className="font-mono text-foreground">{sourceLayer}</code>
                    {' from '}
                    <code className="font-mono text-foreground">{sourceId}</code>
                  </>
                ) : sourceId ? (
                  <>
                    {' drawing '}
                    <code className="font-mono text-foreground">{sourceId}</code>
                  </>
                ) : null}
              </>
            ) : (
              'No layer type draws this source'
            )}
          </p>
          <button
            className="h-7 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
            onClick={onClose}
            type="button"
          >
            Cancel
          </button>
          <button
            className="h-7 rounded-md bg-editor-accent px-3 text-xs font-medium text-white hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-editor-accent focus-visible:ring-offset-1 disabled:opacity-40"
            disabled={!canCreate}
            onClick={create}
            type="button"
          >
            Create
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
