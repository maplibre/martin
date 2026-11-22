import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { ArrowDownWideNarrow, ChevronRight, Crosshair, Merge, Palette } from 'lucide-react';
import { useState } from 'react';
import { cn } from '@/lib/utils';
import { ColorField, Swatch } from '../color-field';
import { type Color, deltaE } from '../colors';
import { nearDuplicates, paletteTokens, type Token } from './model';

/** Props of the palette tab. */
export interface PalettePanelProps {
  /** Style being edited. */
  style: StyleSpecification;
  /** Selected token key. */
  selected: string | null;
  /** Whether the selected token is shown in the accent color on the map. */
  highlight: boolean;
  /** Toggles the map highlight. */
  onHighlight: (on: boolean) => void;
  /** Selects or clears the token. */
  onSelect: (key: string | null) => void;
  /** Recolors every use of a token; `coalesce` merges rapid edits into one history entry. */
  onRecolor: (token: Token, next: Color, coalesce?: string) => void;
  /** Rewrites every token of a group to the first one. */
  onMerge: (group: Token[]) => void;
  /** Opens a layer in the inspector. */
  onSelectLayer: (id: string) => void;
}

type Sort = 'usage' | 'hue';

const GRAY_CHROMA = 0.02;

function byHue(a: Token, b: Token): number {
  const [, ca, ha] = a.color.oklch;
  const [, cb, hb] = b.color.oklch;
  const grayA = ca < GRAY_CHROMA;
  const grayB = cb < GRAY_CHROMA;
  if (grayA !== grayB) return grayA ? 1 : -1;
  if (grayA) return b.color.oklch[0] - a.color.oklch[0];
  return ha - hb;
}

/** Every distinct color, near-duplicate suggestions, and the uses of the selected one. */
export function PalettePanel(props: PalettePanelProps) {
  const { style, selected, onSelect } = props;
  const [role, setRole] = useState<string | null>(null);
  const [sort, setSort] = useState<Sort>('usage');
  const [showDuplicates, setShowDuplicates] = useState(false);

  const tokens = paletteTokens(style);
  const duplicates = nearDuplicates(tokens);
  const roles = [...new Set(tokens.flatMap((t) => [...t.roles]))].sort();
  const filtered = role ? tokens.filter((t) => t.roles.has(role)) : tokens;
  const shown = sort === 'hue' ? [...filtered].sort(byHue) : filtered;
  const places = tokens.reduce((n, t) => n + t.sites.length, 0);

  return (
    <div className="flex flex-col gap-3 p-3">
      <p className="text-xs text-muted-foreground">
        <span className="font-semibold text-foreground tabular-nums">{tokens.length}</span> colors
        in <span className="font-semibold text-foreground tabular-nums">{places}</span> places.
        Change a color here and every layer using it follows.
      </p>

      <div className="flex items-start gap-2">
        <div className="flex min-w-0 flex-1 flex-wrap gap-1">
          {[null, ...roles].map((r) => (
            <button
              className={cn(
                'h-6 rounded-full border px-2.5 text-[11px] capitalize focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
                role === r
                  ? 'border-editor-accent bg-editor-accent-soft text-editor-accent'
                  : 'text-muted-foreground hover:text-foreground',
              )}
              key={r ?? 'all'}
              onClick={() => setRole(r)}
              type="button"
            >
              {r ?? 'all'}
            </button>
          ))}
        </div>
        <button
          className="flex h-6 shrink-0 items-center gap-1 rounded-md border px-2 text-[11px] text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={() => setSort(sort === 'usage' ? 'hue' : 'usage')}
          title="Toggle sort order"
          type="button"
        >
          {sort === 'usage' ? (
            <ArrowDownWideNarrow className="size-3.5" />
          ) : (
            <Palette className="size-3.5" />
          )}
          {sort === 'usage' ? 'Usage' : 'Hue'}
        </button>
      </div>

      {duplicates.length > 0 && (
        <div className="rounded-md border border-dashed">
          <button
            className="flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
            onClick={() => setShowDuplicates((s) => !s)}
            type="button"
          >
            <Merge className="size-3.5 shrink-0 text-editor-accent" />
            <span
              className="min-w-0 flex-1 truncate font-medium"
              title="Colors so close that people cannot tell them apart; merging them simplifies the palette"
            >
              {duplicates.length} sets of nearly identical colors
            </span>
            <ChevronRight
              className={cn(
                'size-3.5 shrink-0 text-muted-foreground transition-transform',
                showDuplicates && 'rotate-90',
              )}
            />
          </button>
          {showDuplicates ? (
            <ul className="border-t">
              {duplicates.map((group) => {
                const spread = Math.max(...group.map((t) => deltaE(group[0].color, t.color)));
                return (
                  <li className="flex h-9 items-center gap-2 px-2.5" key={group[0].key}>
                    <span className="flex shrink-0 -space-x-1">
                      {group.map((t) => (
                        <Swatch
                          className="size-4 rounded-full ring-2 ring-card"
                          color={t.color}
                          key={t.key}
                        />
                      ))}
                    </span>
                    <span className="shrink-0 font-mono text-[11px] text-muted-foreground tabular-nums">
                      ΔE {spread.toFixed(1)}
                    </span>
                    <button
                      className="ml-auto min-w-0 truncate rounded text-[11px] font-medium text-editor-accent hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
                      onClick={() => props.onMerge(group)}
                      type="button"
                    >
                      Merge into {group[0].key}
                    </button>
                  </li>
                );
              })}
            </ul>
          ) : null}
        </div>
      )}

      <ul className="flex flex-col">
        {shown.map((token) =>
          token.key === selected ? (
            <TokenCard key={token.key} {...props} token={token} />
          ) : (
            <li key={token.key}>
              <TokenHeader onClick={() => onSelect(token.key)} token={token} />
            </li>
          ),
        )}
      </ul>
    </div>
  );
}

function TokenHeader({ token, onClick }: { token: Token; onClick: () => void }) {
  const roles = [...token.roles].join(' · ');
  return (
    <button
      className="flex h-9 w-full items-center gap-2.5 rounded-md px-1 text-left hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
      onClick={onClick}
      type="button"
    >
      <Swatch className="size-6" color={token.color} />
      <span className="shrink-0 font-mono text-xs">{token.key}</span>
      <span className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground" title={roles}>
        {roles}
      </span>
      <span className="w-8 shrink-0 text-right text-[11px] text-muted-foreground tabular-nums">
        {token.sites.length}
      </span>
    </button>
  );
}

function TokenCard({
  token,
  highlight,
  onHighlight,
  onSelect,
  onRecolor,
  onSelectLayer,
}: PalettePanelProps & { token: Token }) {
  const byLayer = new Map<string, string[]>();
  for (const site of token.sites) {
    const names = byLayer.get(site.layer) ?? [];
    if (!names.includes(site.name)) names.push(site.name);
    byLayer.set(site.layer, names);
  }

  return (
    <li className="my-1 rounded-lg border border-editor-accent bg-card p-1 shadow-sm">
      <TokenHeader onClick={() => onSelect(null)} token={token} />
      <div className="flex flex-col gap-2 p-1.5">
        <ColorField
          onChange={(next, coalesce) => onRecolor(token, next, coalesce)}
          value={token.color}
        />
        <button
          aria-pressed={highlight}
          className={cn(
            'flex h-7 items-center gap-1.5 self-start rounded-md border px-2 text-[11px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
            highlight
              ? 'border-editor-accent bg-editor-accent text-white'
              : 'text-muted-foreground hover:text-foreground',
          )}
          onClick={() => onHighlight(!highlight)}
          type="button"
        >
          <Crosshair className="size-3.5" /> Show on map
        </button>
        <ul className="flex max-h-56 flex-col overflow-y-auto">
          {[...byLayer].map(([layer, names]) => {
            const props = names.join(', ');
            return (
              <li key={layer}>
                <button
                  className="grid h-7 w-full grid-cols-[minmax(0,1fr)_minmax(0,auto)] items-center gap-2 rounded px-1 text-left hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
                  onClick={() => onSelectLayer(layer)}
                  type="button"
                >
                  <span className="truncate font-mono text-[11px]" title={layer}>
                    {layer}
                  </span>
                  <span
                    className="max-w-40 truncate font-mono text-[11px] text-muted-foreground"
                    title={props}
                  >
                    {props}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    </li>
  );
}
