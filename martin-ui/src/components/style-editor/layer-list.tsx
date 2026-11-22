import type {
  LayerSpecification,
  SourceSpecification,
  StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';
import {
  ArrowDown,
  ArrowUp,
  Box,
  ChevronDown,
  ChevronRight,
  Circle,
  Copy,
  Eye,
  EyeOff,
  Flame,
  Image,
  Layers,
  type LucideIcon,
  MoreHorizontal,
  Mountain,
  PaintBucket,
  Plus,
  Search,
  Spline,
  Square,
  Trash2,
  Type,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { cn } from '@/lib/utils';
import { AddLayerDialog } from './add-layer/dialog';
import { colorKey, colorSites, formatColor } from './colors';
import { type MenuItem, PopoverMenu } from './layer-list/menu';
import {
  type DropSide,
  dropIndex,
  groupRuns,
  isHidden,
  matches,
  type Run,
} from './layer-list/model';

/** A layer to add, plus the source to create for it when it uses a new one. */
export interface AddLayerRequest {
  /** The complete new layer. */
  layer: LayerSpecification;
  /** A source to add to the style first, e.g. one picked from Martin's catalog. */
  source?: { id: string; spec: SourceSpecification };
}

/** Props of the left-hand layer list. */
export interface LayerListProps {
  /** Style being edited. */
  style: StyleSpecification;
  /** Selected layer id. */
  selected: string | null;
  /** Layers to mark, e.g. those using the color selected in the palette. */
  highlighted: ReadonlySet<string>;
  /** Selects a layer. */
  onSelect: (id: string) => void;
  /** Shows or hides layers. */
  onVisibility: (ids: string[], visible: boolean) => void;
  /** Moves a layer to an index in `style.layers` (0 draws first). */
  onMove: (id: string, toIndex: number) => void;
  /** Duplicates a layer. */
  onDuplicate: (id: string) => void;
  /** Deletes a layer. */
  onDelete: (id: string) => void;
  /** Adds a new layer, and a new source with it when `source` is given; the shell inserts it above the selection. */
  onAddLayer: (request: AddLayerRequest) => void;
}

const TYPE_ICONS: Record<LayerSpecification['type'], LucideIcon> = {
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

interface MenuState {
  id: string;
  x: number;
  y: number;
}

interface DropState {
  id: string;
  side: DropSide;
}

const WIDTH_KEY = 'martin-style-editor-layer-width';
const WIDTH_MIN = 240;
const WIDTH_MAX = 520;
const WIDTH_DEFAULT = 320;

function clampWidth(w: number): number {
  return Math.min(WIDTH_MAX, Math.max(WIDTH_MIN, w));
}

function loadWidth(): number {
  try {
    const stored = Number(localStorage.getItem(WIDTH_KEY));
    return Number.isFinite(stored) && stored > 0 ? clampWidth(stored) : WIDTH_DEFAULT;
  } catch {
    return WIDTH_DEFAULT;
  }
}

function saveWidth(width: number | null) {
  try {
    if (width === null) localStorage.removeItem(WIDTH_KEY);
    else localStorage.setItem(WIDTH_KEY, String(width));
  } catch {
    // Storage is unavailable; the width just does not persist.
  }
}

const ICON_BUTTON =
  'flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

function zoomRange(layer: LayerSpecification): string | null {
  if (layer.minzoom === undefined && layer.maxzoom === undefined) return null;
  return `z${layer.minzoom ?? 0}-${layer.maxzoom ?? 24}`;
}

function Swatches({ colors }: { colors: string[] }) {
  if (colors.length === 0) return null;
  return (
    <span
      className="flex shrink-0 items-center"
      title={`Colors this layer draws with: ${colors.join(', ')}`}
    >
      {colors.map((css, i) => (
        <span
          className="-ml-1 size-2.5 overflow-hidden rounded-full border border-foreground/25 bg-[length:4px_4px] bg-[conic-gradient(var(--muted)_25%,var(--background)_0_50%,var(--muted)_0_75%,var(--background)_0)] first:ml-0"
          key={css}
          style={{ zIndex: colors.length - i }}
        >
          <span className="block size-full" style={{ background: css }} />
        </span>
      ))}
    </span>
  );
}

function Row({
  layer,
  colors,
  indented,
  selected,
  highlighted,
  menuOpen,
  drop,
  onSelect,
  onVisibility,
  onOpenMenu,
  onDragStart,
  onDragOver,
  onDrop,
}: {
  layer: LayerSpecification;
  colors: string[];
  indented: boolean;
  selected: boolean;
  highlighted: boolean;
  menuOpen: boolean;
  drop: DropSide | null;
  onSelect: (id: string) => void;
  onVisibility: (ids: string[], visible: boolean) => void;
  onOpenMenu: (id: string, anchor: HTMLElement) => void;
  onDragStart: (id: string) => void;
  onDragOver: (id: string, side: DropSide) => void;
  onDrop: (id: string, side: DropSide) => void;
}) {
  const Icon = TYPE_ICONS[layer.type];
  const hidden = isHidden(layer);
  const range = zoomRange(layer);
  const sideOf = (e: React.DragEvent<HTMLDivElement>): DropSide => {
    const box = e.currentTarget.getBoundingClientRect();
    return e.clientY < box.top + box.height / 2 ? 'above' : 'below';
  };

  return (
    // biome-ignore lint/a11y/useFocusableInteractive lint/a11y/useKeyWithClickEvents: the listbox holds focus and handles keys
    <div
      aria-selected={selected}
      className={cn(
        'group relative flex h-7 cursor-pointer items-center gap-1.5 pr-1.5 pl-2.5 text-xs',
        selected ? 'bg-editor-accent-soft' : 'hover:bg-muted',
        hidden && 'text-muted-foreground',
      )}
      data-layer-id={layer.id}
      draggable
      onClick={() => onSelect(layer.id)}
      onDragOver={(e) => {
        e.preventDefault();
        onDragOver(layer.id, sideOf(e));
      }}
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', layer.id);
        onDragStart(layer.id);
      }}
      onDrop={(e) => {
        e.preventDefault();
        onDrop(layer.id, sideOf(e));
      }}
      role="option"
    >
      {selected ? <span className="absolute inset-y-0 left-0 w-0.5 bg-editor-accent" /> : null}
      {drop ? (
        <span
          className={cn(
            'pointer-events-none absolute inset-x-0 z-10 h-0.5 bg-editor-accent',
            drop === 'above' ? 'top-0' : 'bottom-0',
          )}
        />
      ) : null}
      <span className={cn('flex w-6 shrink-0 items-center', hidden && 'opacity-60')}>
        <Swatches colors={colors} />
      </span>
      {indented ? <span aria-hidden="true" className="w-2 shrink-0" /> : null}
      <Icon className="size-3.5 shrink-0 text-muted-foreground" />
      <span
        className={cn('min-w-0 flex-1 truncate font-mono', hidden && 'opacity-60')}
        title={layer.id}
      >
        {layer.id}
      </span>
      {highlighted ? (
        <span
          className="size-1.5 shrink-0 rounded-full bg-editor-accent"
          title="Uses the selected color"
        />
      ) : null}
      {hidden ? (
        <EyeOff
          aria-label="Hidden"
          className={cn(
            'size-3.5 shrink-0 text-muted-foreground',
            menuOpen ? 'hidden' : 'group-hover:hidden group-focus-within:hidden',
          )}
        />
      ) : null}
      <span
        className={cn(
          'shrink-0 items-center',
          menuOpen ? 'flex' : 'hidden group-hover:flex group-focus-within:flex',
        )}
      >
        <button
          aria-label={hidden ? `Show ${layer.id}` : `Hide ${layer.id}`}
          className={ICON_BUTTON}
          onClick={(e) => {
            e.stopPropagation();
            onVisibility([layer.id], hidden);
          }}
          type="button"
        >
          {hidden ? <Eye className="size-3.5" /> : <EyeOff className="size-3.5" />}
        </button>
        <button
          aria-expanded={menuOpen}
          aria-haspopup="menu"
          aria-label={`More actions for ${layer.id}`}
          className={ICON_BUTTON}
          onClick={(e) => {
            e.stopPropagation();
            onOpenMenu(layer.id, e.currentTarget);
          }}
          type="button"
        >
          <MoreHorizontal className="size-3.5" />
        </button>
      </span>
      <span
        className="w-12 shrink-0 whitespace-nowrap text-right text-[10px] tabular-nums text-muted-foreground/70"
        title={range ? `Drawn at zoom ${range.slice(1)}` : undefined}
      >
        {range}
      </span>
    </div>
  );
}

function RunHeader({
  run,
  open,
  drop,
  onToggle,
  onVisibility,
  onDragOver,
  onDrop,
}: {
  run: Run;
  open: boolean;
  drop: boolean;
  onToggle: () => void;
  onVisibility: (ids: string[], visible: boolean) => void;
  onDragOver: (id: string, side: DropSide) => void;
  onDrop: (id: string, side: DropSide) => void;
}) {
  const allHidden = run.layers.every(isHidden);
  const first = run.layers[0].id;
  const name = run.sourceLayer ?? '';
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: mouse-only drop target
    <div
      className="group relative flex h-7 items-center gap-1 pr-1.5 pl-2.5 text-xs hover:bg-muted"
      onDragOver={(e) => {
        e.preventDefault();
        onDragOver(first, 'above');
      }}
      onDrop={(e) => {
        e.preventDefault();
        onDrop(first, 'above');
      }}
    >
      {drop ? (
        <span className="pointer-events-none absolute inset-x-0 top-0 z-10 h-0.5 bg-editor-accent" />
      ) : null}
      <button
        aria-expanded={open}
        className="flex min-w-0 flex-1 items-center gap-1.5 rounded text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
        onClick={onToggle}
        type="button"
      >
        <span className="flex w-6 shrink-0 items-center">
          {open ? (
            <ChevronDown className="size-3.5 text-muted-foreground" />
          ) : (
            <ChevronRight className="size-3.5 text-muted-foreground" />
          )}
        </span>
        <span className="min-w-0 truncate font-medium" title={name}>
          {name}
        </span>
        <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
          {run.layers.length}
        </span>
      </button>
      <button
        aria-label={allHidden ? `Show all ${name} layers` : `Hide all ${name} layers`}
        className={cn(ICON_BUTTON, 'shrink-0')}
        onClick={() =>
          onVisibility(
            run.layers.map((l) => l.id),
            allHidden,
          )
        }
        type="button"
      >
        {allHidden ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
      </button>
    </div>
  );
}

/** Layers in draw order, topmost first. */
export function LayerList({
  style,
  selected,
  highlighted,
  onSelect,
  onVisibility,
  onMove,
  onDuplicate,
  onDelete,
  onAddLayer,
}: LayerListProps) {
  const [adding, setAdding] = useState(false);
  const [query, setQuery] = useState('');
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [drop, setDrop] = useState<DropState | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(loadWidth);
  const [resizing, setResizing] = useState(false);
  const widthRef = useRef(width);
  widthRef.current = width;

  const swatches = new Map<string, string[]>();
  const seen = new Set<string>();
  for (const site of colorSites(style)) {
    const key = `${site.layer} ${colorKey(site.color)}`;
    const list = swatches.get(site.layer) ?? [];
    if (list.length < 3 && !seen.has(key)) {
      seen.add(key);
      list.push(formatColor(site.color));
    }
    swatches.set(site.layer, list);
  }

  const filtering = query.trim() !== '';
  const shown = [...style.layers].reverse().filter((l) => matches(l, query));
  const runs = groupRuns(shown);
  const isOpen = (run: Run) => run.layers.length === 1 || filtering || !collapsed.has(run.key);
  const visibleIds = runs.flatMap((run) => (isOpen(run) ? run.layers.map((l) => l.id) : []));

  useEffect(() => {
    if (!selected) return;
    const row = listRef.current?.querySelector(`[data-layer-id="${CSS.escape(selected)}"]`);
    row?.scrollIntoView({ block: 'nearest' });
  }, [selected]);

  useEffect(() => {
    if (!selected) return;
    const run = groupRuns([...style.layers].reverse()).find((r) =>
      r.layers.some((l) => l.id === selected),
    );
    if (run) {
      const key = run.key;
      setCollapsed((prev) => {
        if (!prev.has(key)) return prev;
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  }, [selected, style.layers]);

  const startResize = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const handle = e.currentTarget;
    handle.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startWidth = widthRef.current;
    setResizing(true);
    const move = (ev: PointerEvent) => setWidth(clampWidth(startWidth + ev.clientX - startX));
    const end = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', end);
      handle.removeEventListener('pointercancel', end);
      setResizing(false);
      saveWidth(widthRef.current);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
  };

  const finishDrag = () => {
    setDragging(null);
    setDrop(null);
  };
  const dropOn = (target: string, side: DropSide) => {
    if (dragging && dragging !== target)
      onMove(dragging, dropIndex(style.layers, dragging, target, side));
    finishDrag();
  };
  const dragOver = (target: string, side: DropSide) => {
    if (dragging && (drop?.id !== target || drop.side !== side)) setDrop({ id: target, side });
  };

  const menuItems = (id: string): MenuItem[] => {
    const at = style.layers.findIndex((l) => l.id === id);
    return [
      { icon: <Copy className="size-3.5" />, label: 'Duplicate', onChoose: () => onDuplicate(id) },
      {
        disabled: at >= style.layers.length - 1,
        icon: <ArrowUp className="size-3.5" />,
        label: 'Move up',
        onChoose: () => onMove(id, at + 1),
      },
      {
        disabled: at <= 0,
        icon: <ArrowDown className="size-3.5" />,
        label: 'Move down',
        onChoose: () => onMove(id, at - 1),
      },
      {
        destructive: true,
        icon: <Trash2 className="size-3.5" />,
        label: 'Delete',
        onChoose: () => onDelete(id),
      },
    ];
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const at = selected ? visibleIds.indexOf(selected) : -1;
      const next =
        e.key === 'ArrowDown' ? Math.min(at + 1, visibleIds.length - 1) : Math.max(at - 1, 0);
      if (visibleIds[next]) onSelect(visibleIds[next]);
    } else if (e.key === 'Enter' && selected) {
      onSelect(selected);
    }
  };

  const renderRow = (layer: LayerSpecification, indented: boolean) => (
    <Row
      colors={swatches.get(layer.id) ?? []}
      drop={drop?.id === layer.id ? drop.side : null}
      highlighted={highlighted.has(layer.id)}
      indented={indented}
      key={layer.id}
      layer={layer}
      menuOpen={menu?.id === layer.id}
      onDragOver={dragOver}
      onDragStart={setDragging}
      onDrop={dropOn}
      onOpenMenu={(id, anchor) => {
        const box = anchor.getBoundingClientRect();
        setMenu({ id, x: box.right, y: box.bottom + 2 });
      }}
      onSelect={onSelect}
      onVisibility={onVisibility}
      selected={layer.id === selected}
    />
  );

  return (
    <aside className="relative flex shrink-0 flex-col border-r bg-card" style={{ width }}>
      {/* biome-ignore lint/a11y/useSemanticElements: a div separator is the pointer-only resize handle */}
      <div
        aria-label="Resize layer list"
        aria-orientation="vertical"
        aria-valuemax={WIDTH_MAX}
        aria-valuemin={WIDTH_MIN}
        aria-valuenow={width}
        className="group/handle absolute inset-y-0 -right-[2px] z-20 flex w-1 cursor-col-resize touch-none justify-center"
        onDoubleClick={() => {
          setWidth(WIDTH_DEFAULT);
          saveWidth(null);
        }}
        onPointerDown={startResize}
        role="separator"
        tabIndex={-1}
      >
        <span
          className={cn(
            'h-full w-px transition-colors group-hover/handle:bg-editor-accent',
            resizing && 'bg-editor-accent',
          )}
        />
      </div>
      <div className="flex flex-col gap-2 border-b p-3">
        <div className="flex items-center justify-between">
          <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            Layers
          </h2>
          <div className="flex items-center gap-1.5">
            <span className="text-[11px] tabular-nums text-muted-foreground">
              {filtering ? `${shown.length} / ${style.layers.length}` : style.layers.length}
            </span>
            <button
              aria-label="Add layer"
              className={ICON_BUTTON}
              onClick={() => setAdding(true)}
              title="Add layer"
              type="button"
            >
              <Plus className="size-3.5" />
            </button>
          </div>
        </div>
        <div className="relative">
          <Search className="pointer-events-none absolute top-2 left-2 size-3.5 text-muted-foreground" />
          <input
            aria-label="Search layers"
            className="h-7 w-full rounded-md border bg-background pr-2 pl-7 text-xs placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search id, source layer, type"
            type="search"
            value={query}
          />
        </div>
      </div>
      <div
        aria-label="Layers"
        className="min-h-0 flex-1 scroll-py-1 overflow-y-auto pb-1 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-editor-accent"
        onDragEnd={finishDrag}
        onKeyDown={onKeyDown}
        ref={listRef}
        role="listbox"
        tabIndex={0}
      >
        {runs.length === 0 ? (
          <p className="px-3 py-6 text-center text-xs text-muted-foreground">
            No layers match "{query}"
          </p>
        ) : null}
        {runs.map((run) =>
          run.layers.length === 1 ? (
            renderRow(run.layers[0], false)
          ) : (
            <div key={run.key}>
              <RunHeader
                drop={drop?.id === run.layers[0].id && drop.side === 'above'}
                onDragOver={dragOver}
                onDrop={dropOn}
                onToggle={() =>
                  setCollapsed((prev) => {
                    const next = new Set(prev);
                    if (!next.delete(run.key)) next.add(run.key);
                    return next;
                  })
                }
                onVisibility={onVisibility}
                open={isOpen(run)}
                run={run}
              />
              {isOpen(run) ? run.layers.map((l) => renderRow(l, true)) : null}
            </div>
          ),
        )}
      </div>
      {adding ? (
        <AddLayerDialog onAdd={onAddLayer} onClose={() => setAdding(false)} style={style} />
      ) : null}
      {menu ? (
        <PopoverMenu
          items={menuItems(menu.id)}
          onClose={() => setMenu(null)}
          x={menu.x}
          y={menu.y}
        />
      ) : null}
    </aside>
  );
}
