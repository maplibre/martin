import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { ChevronDown } from 'lucide-react';
import type { Map as MapLibre } from 'maplibre-gl';
import { type ReactNode, useEffect, useState } from 'react';
import { Popover } from '../inspector/popover';
import { MapCanvas } from '../map-canvas';
import type { Version } from '../use-style-document';
import type { StyleDiff } from './style-diff';

/** Which earlier state the compare view shows: the loaded style or the state after one edit. */
export type CompareBase = { kind: 'original' } | { kind: 'version'; editId: number };

/** Resolves `base` against the history, falling back to the original once that edit is undone. */
export function baseStyle(
  base: CompareBase,
  original: StyleSpecification,
  versions: readonly Version[],
): { style: StyleSpecification; base: CompareBase } {
  if (base.kind === 'version') {
    const v = versions.find((x) => x.edit.id === base.editId);
    if (v) return { base, style: v.style };
  }
  return { base: { kind: 'original' }, style: original };
}

/** Props of {@link CompareMap}. */
export interface CompareMapProps {
  /** Earlier style to draw. */
  style: StyleSpecification;
  /** The main map, whose camera this map follows and drives. */
  main: MapLibre | null;
  /** CSS filter for color-vision simulation, same as the main map. */
  filter?: string;
  /** Picks which earlier state to show. */
  picker: ReactNode;
}

/** Second map on the left of the compare view, camera-locked to the main map. */
export function CompareMap({ style, main, filter, picker }: CompareMapProps) {
  const [map, setMap] = useState<MapLibre | null>(null);

  useEffect(() => {
    if (!map || !main) return;
    let syncing = false;
    const follow = (from: MapLibre, to: MapLibre) => () => {
      if (syncing) return;
      syncing = true;
      to.jumpTo({
        bearing: from.getBearing(),
        center: from.getCenter(),
        pitch: from.getPitch(),
        zoom: from.getZoom(),
      });
      syncing = false;
    };
    const fromMain = follow(main, map);
    const fromBase = follow(map, main);
    fromMain();
    main.on('move', fromMain);
    map.on('move', fromBase);
    return () => {
      main.off('move', fromMain);
      map.off('move', fromBase);
    };
  }, [map, main]);

  return (
    <>
      <MapCanvas filter={filter} onReady={setMap} style={style} />
      <div className="pointer-events-none absolute inset-x-3 top-14 flex">{picker}</div>
    </>
  );
}

/** Props of {@link VersionPicker}. */
export interface VersionPickerProps {
  /** Shown state. */
  base: CompareBase;
  /** Every applied edit, oldest first. */
  versions: readonly Version[];
  /** Picks a state. */
  onBase: (base: CompareBase) => void;
  /** What differs between the shown state and the current style. */
  diff: StyleDiff;
  /** Selects a changed layer in the inspector. */
  onSelectLayer: (id: string) => void;
}

/** Chip on the earlier map that picks the state to compare against and sums up what changed since. */
export function VersionPicker({ base, versions, onBase, diff, onSelectLayer }: VersionPickerProps) {
  const changes = [
    diff.layers.length
      ? `${diff.layers.length} layer${diff.layers.length === 1 ? '' : 's'} changed`
      : null,
    diff.removed.length ? `${diff.removed.length} removed` : null,
    diff.root ? 'style settings changed' : null,
  ].filter((x) => x !== null);
  const summary = changes.length ? changes.join(' · ') : 'Same as current';
  return (
    <div className="pointer-events-auto flex max-w-full items-center gap-2 rounded-lg border bg-card/95 py-1 pr-2.5 pl-1 text-xs shadow-sm backdrop-blur">
      <select
        aria-label="Compare with"
        className="h-7 max-w-56 min-w-0 truncate rounded-md border-0 bg-muted px-2 font-mono text-[11px] outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
        onChange={(e) =>
          onBase(
            e.target.value === 'original'
              ? { kind: 'original' }
              : { editId: Number(e.target.value), kind: 'version' },
          )
        }
        value={base.kind === 'original' ? 'original' : String(base.editId)}
      >
        <option value="original">Original (as loaded)</option>
        {versions.map((v, i) => (
          <option key={v.edit.id} value={v.edit.id}>
            After edit {i + 1}: {v.edit.label}
          </option>
        ))}
      </select>
      {diff.layers.length || diff.removed.length ? (
        <Popover
          className="min-w-0"
          panelClassName="left-0 top-full mt-2 max-h-80 w-80 overflow-y-auto p-1"
          trigger={({ open, toggle }) => (
            <button
              aria-expanded={open}
              className="flex min-w-0 items-center gap-1 rounded px-1 text-[11px] text-foreground underline decoration-muted-foreground/50 underline-offset-2 hover:decoration-foreground"
              onClick={toggle}
              type="button"
            >
              <span className="truncate">{summary}</span>
              <ChevronDown aria-hidden="true" className="size-3 shrink-0 opacity-60" />
            </button>
          )}
        >
          {(close) => (
            <ul className="text-xs">
              {diff.layers.map((change) => (
                <li key={change.id}>
                  <button
                    className="flex w-full flex-col items-start rounded-sm px-2 py-1.5 text-left hover:bg-muted"
                    onClick={() => {
                      close();
                      onSelectLayer(change.id);
                    }}
                    type="button"
                  >
                    <span className="font-mono text-[11px]">{change.id}</span>
                    <span className="text-[11px] text-muted-foreground">
                      {change.kind === 'added' ? 'added' : change.properties.join(', ')}
                    </span>
                  </button>
                </li>
              ))}
              {diff.removed.map((id) => (
                <li className="px-2 py-1.5" key={id}>
                  <span className="font-mono text-[11px] text-muted-foreground line-through">
                    {id}
                  </span>
                  <span className="ml-2 text-[11px] text-muted-foreground">removed</span>
                </li>
              ))}
            </ul>
          )}
        </Popover>
      ) : (
        <span className="truncate text-[11px] text-muted-foreground">{summary}</span>
      )}
    </div>
  );
}

/** Chip on the current map in the compare view. */
export function CurrentLabel({ edits }: { edits: number }) {
  return (
    <div className="pointer-events-none absolute top-14 left-3 rounded-lg border bg-card/95 px-2.5 py-1.5 text-xs shadow-sm backdrop-blur">
      <span className="font-medium">Current</span>
      <span className="ml-2 text-[11px] text-muted-foreground">
        {edits} edit{edits === 1 ? '' : 's'}
      </span>
    </div>
  );
}
