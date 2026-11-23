import { Search } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { buildMartinUrl } from '@/lib/api';
import type { components } from '@/lib/types.gen';
import { cn } from '@/lib/utils';
import { useStyleResources } from '../../../style-resources';
import { type FontFamily, filterFamilies, groupFamilies } from './families';
import { GlyphSample } from './glyph-sample';

const WIDTH = 380;
const MAX_HEIGHT = 440;
const MARGIN = 8;

interface Placement {
  left: number;
  top?: number;
  bottom?: number;
  maxHeight: number;
}

function place(anchor: HTMLElement): Placement {
  const r = anchor.getBoundingClientRect();
  const width = Math.min(WIDTH, window.innerWidth - 2 * MARGIN);
  const left = Math.max(MARGIN, Math.min(r.left, window.innerWidth - width - MARGIN));
  const below = window.innerHeight - r.bottom - MARGIN - 4;
  const above = r.top - MARGIN - 4;
  if (below < 260 && above > below) {
    return {
      bottom: window.innerHeight - r.top + 4,
      left,
      maxHeight: Math.min(MAX_HEIGHT, above),
    };
  }
  return { left, maxHeight: Math.min(MAX_HEIGHT, below), top: r.bottom + 4 };
}

type Catalog = components['schemas']['Catalog'];

function pointsAtMartin(glyphs: string | undefined): boolean {
  if (!glyphs) return false;
  return glyphs.startsWith(buildMartinUrl('/font/')) || glyphs.includes('/font/{fontstack}');
}

function FamilyRow({
  family,
  glyphs,
  inStack,
  onPick,
}: {
  family: FontFamily;
  glyphs: string | undefined;
  inStack: string[];
  onPick: (name: string) => void;
}) {
  const primary = family.variants[0];
  return (
    <li className="rounded-md px-1.5 py-1 hover:bg-muted/60">
      <button
        className="flex w-full min-w-0 flex-col items-start gap-0.5 text-left"
        onClick={() => onPick(primary.name)}
        title={primary.name}
        type="button"
      >
        <span className="w-full min-w-0 truncate font-mono text-xs">{family.family}</span>
        <GlyphSample glyphsUrl={glyphs} name={primary.name} />
      </button>
      <div className="mt-0.5 flex flex-wrap gap-1">
        {family.variants.map((v) => (
          <button
            className={cn(
              'max-w-full truncate rounded px-1.5 py-px text-[11px]',
              inStack.includes(v.name)
                ? 'bg-editor-accent-soft text-editor-accent'
                : 'bg-muted text-muted-foreground hover:text-foreground',
            )}
            key={v.name}
            onClick={() => onPick(v.name)}
            title={v.name}
            type="button"
          >
            {v.label}
          </button>
        ))}
      </div>
    </li>
  );
}

function Group({
  title,
  families,
  glyphs,
  inStack,
  onPick,
}: {
  title: string;
  families: FontFamily[];
  glyphs: string | undefined;
  inStack: string[];
  onPick: (name: string) => void;
}) {
  if (!families.length) return null;
  return (
    <section aria-label={title}>
      <h3 className="px-1.5 pt-2 pb-1 text-[11px] font-medium text-muted-foreground uppercase">
        {title}
      </h3>
      <ul>
        {families.map((f) => (
          <FamilyRow family={f} glyphs={glyphs} inStack={inStack} key={f.family} onPick={onPick} />
        ))}
      </ul>
    </section>
  );
}

/** Searchable popup anchored to a trigger that lists the style's fonts and the fonts of this Martin server with glyph previews. */
export function FontPopup({
  anchor,
  stack,
  onPick,
  onClose,
}: {
  /** Element the popup opens under, also exempt from outside-click closing. */
  anchor: HTMLElement;
  /** Fonts already in the stack, highlighted in the list. */
  stack: string[];
  /** Called with the chosen font name. */
  onPick: (name: string) => void;
  /** Called when the popup should close without a choice. */
  onClose: () => void;
}) {
  const { glyphs, fontsInStyle } = useStyleResources();
  const popupRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [placement, setPlacement] = useState<Placement | null>(null);
  const [query, setQuery] = useState('');
  const [server, setServer] = useState<string[] | null>(null);
  const listable = pointsAtMartin(glyphs);

  useLayoutEffect(() => {
    setPlacement(place(anchor));
  }, [anchor]);

  useEffect(() => {
    inputRef.current?.focus();
    const onPointerDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!popupRef.current?.contains(t) && !anchor.contains(t)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKey);
    window.addEventListener('resize', onClose);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', onClose);
    };
  }, [anchor, onClose]);

  useEffect(() => {
    if (!listable) return;
    let cancelled = false;
    fetch(buildMartinUrl('/catalog'))
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json() as Promise<Catalog>;
      })
      .then((c) => {
        if (!cancelled) setServer(Object.keys(c.fonts ?? {}).sort());
      })
      .catch(() => {
        if (!cancelled) setServer([]);
      });
    return () => {
      cancelled = true;
    };
  }, [listable]);

  const used = filterFamilies(groupFamilies(fontsInStyle), query);
  const available = filterFamilies(
    groupFamilies((server ?? []).filter((n) => !fontsInStyle.includes(n))),
    query,
  );
  const typed = query.trim();
  const known = new Set([...fontsInStyle, ...(server ?? [])]);

  return createPortal(
    <div
      aria-label="Choose font"
      className="fixed z-50 flex flex-col overflow-hidden rounded-lg border bg-popover text-xs text-popover-foreground shadow-lg"
      ref={popupRef}
      role="dialog"
      style={{
        bottom: placement?.bottom,
        left: placement?.left,
        maxHeight: placement?.maxHeight ?? MAX_HEIGHT,
        top: placement?.top,
        visibility: placement ? 'visible' : 'hidden',
        width: Math.min(WIDTH, window.innerWidth - 2 * MARGIN),
      }}
    >
      <p className="border-b px-2.5 py-1.5 text-[11px] text-muted-foreground">
        Fonts are tried in order; later ones fill in missing characters.
      </p>
      <div className="flex items-center gap-1.5 border-b px-2.5 py-1.5">
        <Search className="size-3.5 shrink-0 text-muted-foreground" />
        <input
          aria-label="Search fonts"
          className="min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground"
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search fonts"
          ref={inputRef}
          value={query}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-1 pb-1">
        <Group
          families={used}
          glyphs={glyphs}
          inStack={stack}
          onPick={onPick}
          title="Used in this style"
        />
        {listable ? (
          <>
            <Group
              families={available}
              glyphs={glyphs}
              inStack={stack}
              onPick={onPick}
              title="Available on this Martin server"
            />
            {server === null && (
              <p className="px-1.5 py-2 text-[11px] text-muted-foreground">
                Loading server fonts...
              </p>
            )}
          </>
        ) : (
          <p className="px-1.5 py-2 text-[11px] text-muted-foreground">
            This glyph server cannot list its fonts. Type a font name to use it.
          </p>
        )}
        {used.length === 0 && available.length === 0 && server !== null && typed === '' && (
          <p className="px-1.5 py-2 text-[11px] text-muted-foreground">No fonts to show.</p>
        )}
        {typed && !known.has(typed) && (
          <button
            className="mt-1 flex w-full min-w-0 items-center rounded-md px-1.5 py-1.5 text-left hover:bg-muted/60"
            onClick={() => onPick(typed)}
            title={typed}
            type="button"
          >
            <span className="min-w-0 truncate">
              Use custom name <span className="font-mono">"{typed}"</span>
            </span>
          </button>
        )}
      </div>
    </div>,
    document.body,
  );
}
