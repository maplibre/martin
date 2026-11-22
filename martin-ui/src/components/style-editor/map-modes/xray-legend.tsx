import { Eye, EyeOff } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { XrayEntry } from './xray';

/** Props of {@link XrayLegend}. */
export interface XrayLegendProps {
  /** Every block of data in the style. */
  entries: readonly XrayEntry[];
  /** Keys of entries switched off. */
  hidden: ReadonlySet<string>;
  /** Shows or hides entries. */
  onToggle: (keys: readonly string[], visible: boolean) => void;
  /** Rendered feature count per entry key in the current view; missing until the map is idle. */
  counts: ReadonlyMap<string, number>;
  /** Entry read by the selected style layer, drawn on top in the accent color. */
  focusKey?: string;
  /** Selected style layer id, for the focus caption. */
  focusLayer?: string;
  /** Called with the entry under the pointer, `null` when it leaves the list. */
  onHover: (key: string | null) => void;
}

/** Floating list of the style's sources and source layers that turns each on or off in X-ray mode. */
export function XrayLegend({
  entries,
  hidden,
  onToggle,
  counts,
  focusKey,
  focusLayer,
  onHover,
}: XrayLegendProps) {
  const bySource = new Map<string, XrayEntry[]>();
  for (const e of entries) bySource.set(e.source, [...(bySource.get(e.source) ?? []), e]);
  const allVisible = entries.every((e) => !hidden.has(e.key));
  return (
    <section
      aria-label="Source layers"
      className="pointer-events-auto flex max-h-full w-64 flex-col overflow-hidden rounded-lg border bg-card/95 text-xs shadow-sm backdrop-blur"
    >
      <header className="flex items-center justify-between border-b px-2.5 py-1.5">
        <h3 className="text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
          Source layers
        </h3>
        <button
          className="text-[11px] text-muted-foreground hover:text-foreground"
          onClick={() =>
            onToggle(
              entries.map((e) => e.key),
              !allVisible,
            )
          }
          type="button"
        >
          {allVisible ? 'Hide all' : 'Show all'}
        </button>
      </header>
      {focusKey && focusLayer ? (
        <p className="flex items-center gap-1.5 border-b px-2.5 py-1.5 text-[11px] text-muted-foreground">
          <span className="size-2 shrink-0 rounded-full bg-editor-accent" />
          <span className="min-w-0 truncate">
            Matches <span className="font-mono text-foreground">{focusLayer}</span>
          </span>
        </p>
      ) : null}
      <div className="min-h-0 overflow-y-auto py-1">
        {[...bySource.entries()].map(([source, group]) => (
          <div key={source}>
            <p
              className="truncate px-2.5 pt-1.5 pb-0.5 font-mono text-[10px] text-muted-foreground"
              title={source}
            >
              {source}
            </p>
            <ul>
              {group.map((e) => (
                <LegendRow
                  count={counts.size ? (counts.get(e.key) ?? 0) : undefined}
                  entry={e}
                  focused={e.key === focusKey}
                  key={e.key}
                  onHover={(on) => onHover(on ? e.key : null)}
                  onSolo={() =>
                    onToggle(
                      entries.filter((x) => x.key !== e.key).map((x) => x.key),
                      false,
                    )
                  }
                  onToggle={(visible) => onToggle([e.key], visible)}
                  visible={!hidden.has(e.key)}
                />
              ))}
            </ul>
          </div>
        ))}
      </div>
      <p className="border-t px-2.5 py-1.5 text-[10px] text-muted-foreground">
        Hover a row to bring it forward · Alt-click to show only it
      </p>
    </section>
  );
}

function LegendRow({
  entry,
  visible,
  focused,
  count,
  onToggle,
  onSolo,
  onHover,
}: {
  entry: XrayEntry;
  visible: boolean;
  focused: boolean;
  count: number | undefined;
  onToggle: (visible: boolean) => void;
  onSolo: () => void;
  onHover: (on: boolean) => void;
}) {
  const unused = entry.usedBy.length === 0;
  const name = entry.sourceLayer ?? entry.source;
  return (
    <li>
      <button
        aria-pressed={visible}
        className={cn(
          'group flex h-7 w-full items-center gap-2 px-2.5 text-left hover:bg-muted',
          focused && 'bg-editor-accent-soft',
        )}
        onBlur={() => onHover(false)}
        onClick={(e) => {
          if (e.altKey) {
            onSolo();
            onToggle(true);
          } else onToggle(!visible);
        }}
        onFocus={() => onHover(true)}
        onMouseEnter={() => onHover(true)}
        onMouseLeave={() => onHover(false)}
        title={
          unused
            ? `${name}: no style layer reads it`
            : `${name}: read by ${entry.usedBy.length} style layer${entry.usedBy.length === 1 ? '' : 's'}`
        }
        type="button"
      >
        <span
          className={cn('size-2.5 shrink-0 rounded-sm', !visible && 'opacity-25')}
          style={{ backgroundColor: entry.color }}
        />
        <span
          className={cn(
            'min-w-0 flex-1 truncate font-mono',
            !visible && 'text-muted-foreground line-through',
          )}
        >
          {name}
        </span>
        {unused ? (
          <span className="shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground">
            unused
          </span>
        ) : null}
        <span className="w-9 shrink-0 text-right font-mono text-[10px] text-muted-foreground tabular-nums">
          {entry.kind === 'raster' ? '' : (count ?? '')}
        </span>
        {visible ? (
          <Eye
            aria-hidden="true"
            className="size-3.5 shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100"
          />
        ) : (
          <EyeOff aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
        )}
      </button>
    </li>
  );
}
