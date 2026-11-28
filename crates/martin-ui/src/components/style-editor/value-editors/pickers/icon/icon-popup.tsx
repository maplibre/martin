import { Plus } from 'lucide-react';
import { type KeyboardEvent, useEffect, useRef, useState } from 'react';
import { cn } from '@/lib/utils';
import { IconThumb } from './icon-thumb';
import type { SpriteIcon, SpriteIcons } from './sprite-sheets';

const COLUMNS = 4;

/** Props of the icon popup body. */
export interface IconPopupProps {
  /** Loaded sprite icons and their loading state. */
  sprite: SpriteIcons;
  /** Name currently applied, highlighted as the current icon. */
  current: string | undefined;
  /** Called with the icon name or custom text the user picked. */
  onPick: (name: string) => void;
}

function iconTitle(icon: SpriteIcon): string {
  const w = Math.round(icon.width / icon.pixelRatio);
  const h = Math.round(icon.height / icon.pixelRatio);
  return `${icon.name} · ${w}×${h} px${icon.sdf ? ' · SDF' : ''}`;
}

/** Search field, current-icon chip and keyboard navigable grid of sprite icons with a custom-name fallback. */
export function IconPopup({ sprite, current, onPick }: IconPopupProps) {
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const activeRef = useRef<HTMLElement | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const setActiveEl = (el: HTMLElement | null) => {
    activeRef.current = el;
  };

  const needle = query.trim().toLowerCase();
  const matches = sprite.icons.filter((i) => i.name.toLowerCase().includes(needle));
  const typed = query.trim();
  const offerCustom = typed !== '' && !sprite.icons.some((i) => i.name === typed);
  const total = matches.length + (offerCustom ? 1 : 0);
  const cursor = Math.min(active, Math.max(total - 1, 0));
  const currentIcon = sprite.icons.find((i) => i.name === current);
  const grouped = sprite.sheetIds.length > 1;

  useEffect(() => {
    searchRef.current?.focus();
  }, []);

  useEffect(() => {
    activeRef.current?.scrollIntoView?.({ block: 'nearest' });
  }, [cursor, query]);

  const move = (delta: number) => setActive(Math.min(Math.max(cursor + delta, 0), total - 1));

  const onKeyDown = (e: KeyboardEvent) => {
    const step: Record<string, number> = {
      ArrowDown: COLUMNS,
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -COLUMNS,
    };
    if (e.key in step && total > 0) {
      e.preventDefault();
      move(step[e.key] ?? 0);
    } else if (e.key === 'Enter' && total > 0) {
      e.preventDefault();
      const icon = matches[cursor];
      onPick(icon ? icon.name : typed);
    }
  };

  let lastSheet: string | undefined;
  const tiles = matches.map((icon, i) => {
    const header = grouped && icon.sheetId !== lastSheet;
    lastSheet = icon.sheetId;
    return (
      <div className="contents" key={icon.name}>
        {header ? (
          <div className="col-span-4 truncate px-1 pt-1 font-mono text-[10px] text-muted-foreground uppercase">
            {icon.sheetId}
          </div>
        ) : null}
        <button
          className={cn(
            'relative flex min-w-0 flex-col items-center gap-1 rounded-md px-1 py-1.5 outline-none hover:bg-muted',
            i === cursor && 'bg-muted ring-1 ring-editor-accent',
            icon.name === current && 'bg-editor-accent-soft',
          )}
          onClick={() => onPick(icon.name)}
          onMouseMove={() => setActive(i)}
          ref={i === cursor ? setActiveEl : undefined}
          tabIndex={-1}
          title={iconTitle(icon)}
          type="button"
        >
          <IconThumb box={28} icon={icon} />
          {icon.sdf ? (
            <span className="absolute top-0.5 right-0.5 rounded bg-muted px-0.5 text-[9px] text-muted-foreground leading-3">
              SDF
            </span>
          ) : null}
          <span className="w-full min-w-0 truncate text-center font-mono text-[10px]">
            {icon.name}
          </span>
        </button>
      </div>
    );
  });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 border-b p-2">
        <input
          aria-label="Search icons"
          className="h-7 min-w-0 flex-1 rounded-md border bg-background px-2 text-xs outline-none focus:ring-1 focus:ring-editor-accent"
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
          placeholder="Search icons"
          ref={searchRef}
          value={query}
        />
        {current ? (
          <button
            className="flex h-7 max-w-[45%] shrink-0 items-center gap-1 rounded-md bg-editor-accent-soft px-1.5 text-[11px] text-editor-accent"
            onClick={() => setQuery(current)}
            title={`Current: ${current}`}
            type="button"
          >
            {currentIcon && <IconThumb box={16} icon={currentIcon} />}
            <span className="min-w-0 truncate font-mono">{current}</span>
          </button>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {sprite.status === 'loading' && (
          <p className="p-2 text-muted-foreground text-xs">Loading sprite…</p>
        )}
        {sprite.status === 'idle' && (
          <p className="p-2 text-muted-foreground text-xs">This style has no sprite.</p>
        )}
        {sprite.status === 'error' && (
          <p className="p-2 text-destructive text-xs" title={sprite.error}>
            Could not load the sprite. {sprite.error}
          </p>
        )}
        {sprite.status === 'ready' && matches.length === 0 && (
          <p className="p-2 text-muted-foreground text-xs">No icon matches.</p>
        )}
        <div className="grid grid-cols-4 gap-1">{tiles}</div>
        {offerCustom ? (
          <button
            className={cn(
              'mt-2 flex w-full min-w-0 items-center gap-2 rounded-md border border-dashed px-2 py-1.5 text-left text-xs outline-none hover:bg-muted',
              cursor === matches.length && 'bg-muted ring-1 ring-editor-accent',
            )}
            onClick={() => onPick(typed)}
            ref={cursor === matches.length ? setActiveEl : undefined}
            title={`Use custom name ${typed}`}
            type="button"
          >
            <Plus className="size-3.5 shrink-0" />
            <span className="shrink-0">Use custom name</span>
            <span className="min-w-0 truncate font-mono text-muted-foreground">{typed}</span>
          </button>
        ) : null}
      </div>
      <div className="border-t px-2 py-1 text-[11px] text-muted-foreground">
        {matches.length} {matches.length === 1 ? 'icon' : 'icons'}
      </div>
    </div>
  );
}
