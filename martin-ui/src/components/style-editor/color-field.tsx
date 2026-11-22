import { Palette } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';
import { Color, colorKey, hex6, withAlpha, withRgb } from './colors';
import { type PaletteColor, useStylePalette } from './style-palette';

/** Props of the color input. */
export interface ColorFieldProps {
  /** Current color. */
  value: Color;
  /** Called with the new color; `coalesce` merges rapid edits into one history entry. */
  onChange: (next: Color, coalesce?: string) => void;
  /** Muted styling when the value is the spec default. */
  isDefault?: boolean;
  /** Narrow layout for table cells: no opacity input. */
  compact?: boolean;
}

const CHECKERBOARD =
  'bg-[length:8px_8px] bg-[position:0_0,4px_4px] bg-[image:conic-gradient(#9996_25%,transparent_0_50%,#9996_0_75%,transparent_0)]';

const inputBase =
  'h-8 min-w-0 rounded-md border bg-background px-2 text-xs tabular-nums focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

function toCss(c: Color): string {
  const [r, g, b, a] = c.rgb;
  return `rgba(${Math.round(r * 255)},${Math.round(g * 255)},${Math.round(b * 255)},${a})`;
}

/** Swatch plus hex text plus opacity. */
export function ColorField({ value, onChange, isDefault, compact }: ColorFieldProps) {
  const [hexDraft, setHexDraft] = useState<string | null>(null);
  const [alphaDraft, setAlphaDraft] = useState<string | null>(null);
  const hexText = hexDraft ?? hex6(value);
  const invalid = hexDraft !== null && Color.parse(hexDraft.trim()) === undefined;
  const percent = Math.round(value.rgb[3] * 100);
  const palette = useStylePalette();
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const pickerOpen = anchor !== null;
  const swatchRef = useRef<HTMLButtonElement>(null);
  const nativeRef = useRef<HTMLInputElement>(null);

  const commitHex = () => {
    if (hexDraft === null) return;
    const parsed = Color.parse(hexDraft.trim());
    if (parsed) onChange(withRgb(value, parsed));
    setHexDraft(null);
  };

  return (
    <div
      className={cn(
        'relative flex h-8 w-full min-w-0 items-center gap-1.5',
        compact && 'gap-1',
        isDefault && 'opacity-60',
      )}
    >
      <button
        aria-expanded={pickerOpen}
        aria-label="Choose a color"
        className={cn(
          'relative size-6 shrink-0 cursor-pointer overflow-hidden rounded-md',
          CHECKERBOARD,
          'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
        )}
        onClick={(e) => setAnchor(pickerOpen ? null : e.currentTarget.getBoundingClientRect())}
        ref={swatchRef}
        title="Choose from this style's colors"
        type="button"
      >
        <span className="absolute inset-0" style={{ backgroundColor: toCss(value) }} />
        <span className="absolute inset-0 rounded-md ring-1 ring-inset ring-border" />
      </button>
      <input
        aria-hidden="true"
        className="pointer-events-none absolute size-0 opacity-0"
        onChange={(e) => {
          const picked = Color.parse(e.target.value);
          if (picked) onChange(withRgb(value, picked), 'color-pick');
        }}
        ref={nativeRef}
        tabIndex={-1}
        type="color"
        value={hex6(value)}
      />
      {anchor
        ? createPortal(
            <PalettePicker
              anchor={anchor}
              colors={palette}
              current={colorKey(value)}
              onClose={() => setAnchor(null)}
              onCustom={() => {
                setAnchor(null);
                nativeRef.current?.showPicker();
              }}
              onPick={(c) => {
                onChange(c);
                setAnchor(null);
                swatchRef.current?.focus();
              }}
              trigger={swatchRef.current}
            />,
            document.body,
          )
        : null}
      <input
        aria-invalid={invalid}
        aria-label="Color"
        className={cn(
          inputBase,
          'min-w-0 flex-1 font-mono',
          compact && 'h-7 px-1.5 text-[11px]',
          invalid && 'border-destructive',
        )}
        onBlur={commitHex}
        onChange={(e) => setHexDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            commitHex();
            e.currentTarget.blur();
          } else if (e.key === 'Escape') {
            setHexDraft(null);
            e.currentTarget.blur();
          }
        }}
        spellCheck={false}
        title="Any CSS color; Enter to apply, Escape to revert"
        type="text"
        value={hexText}
      />
      {compact ? null : (
        <div className="relative w-[3.25rem] shrink-0">
          <input
            aria-label="Opacity in percent"
            className={cn(
              inputBase,
              'w-full [appearance:textfield] pr-4 pl-1 text-right tabular-nums [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
            )}
            max={100}
            min={0}
            onBlur={() => setAlphaDraft(null)}
            onChange={(e) => {
              setAlphaDraft(e.target.value);
              const n = Number.parseFloat(e.target.value);
              if (Number.isFinite(n))
                onChange(withAlpha(value, Math.min(100, Math.max(0, n)) / 100), 'color-opacity');
            }}
            step={1}
            type="number"
            value={alphaDraft ?? percent}
          />
          <span className="pointer-events-none absolute inset-y-0 right-1.5 flex items-center text-[11px] text-muted-foreground">
            %
          </span>
        </div>
      )}
    </div>
  );
}

/** A small color chip on a checkerboard so transparency shows. */
export function Swatch({ color, className }: { color: Color; className?: string }) {
  return (
    <span
      className={cn(
        'relative inline-block size-5 shrink-0 overflow-hidden rounded-md',
        CHECKERBOARD,
        className,
      )}
    >
      <span className="absolute inset-0" style={{ backgroundColor: toCss(color) }} />
      <span className="absolute inset-0 rounded-md ring-1 ring-inset ring-border" />
    </span>
  );
}

const PICKER_WIDTH = 272;

function hueOrder(a: PaletteColor, b: PaletteColor): number {
  const [la, ca, ha] = a.color.oklch;
  const [lb, cb, hb] = b.color.oklch;
  const grayA = ca < 0.03;
  const grayB = cb < 0.03;
  if (grayA !== grayB) return grayA ? -1 : 1;
  if (grayA) return lb - la;
  return (Number.isFinite(ha) ? ha : 0) - (Number.isFinite(hb) ? hb : 0) || lb - la;
}

interface PalettePickerProps {
  anchor: DOMRect;
  colors: PaletteColor[];
  current: string;
  onPick: (color: Color) => void;
  onCustom: () => void;
  onClose: () => void;
  trigger: HTMLElement | null;
}

function PalettePicker({
  anchor,
  colors,
  current,
  onPick,
  onCustom,
  onClose,
  trigger,
}: PalettePickerProps) {
  const panel = useRef<HTMLDivElement>(null);
  const [hovered, setHovered] = useState<PaletteColor | null>(null);
  const sorted = [...colors].sort(hueOrder);
  const left = Math.max(8, Math.min(anchor.left, window.innerWidth - PICKER_WIDTH - 8));
  const below = anchor.bottom + 6;
  const top = below + 280 > window.innerHeight ? Math.max(8, anchor.top - 286) : below;

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (!panel.current?.contains(target) && !trigger?.contains(target)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('pointerdown', onDown, true);
    window.addEventListener('keydown', onKey);
    window.addEventListener('resize', onClose);
    return () => {
      window.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', onClose);
    };
  });

  return (
    <div
      aria-label="Colors in this style"
      className="fixed z-[60] rounded-lg border bg-popover p-2 text-popover-foreground shadow-lg"
      ref={panel}
      role="dialog"
      style={{ left, top, width: PICKER_WIDTH }}
    >
      <div className="mb-1.5 flex items-baseline justify-between gap-2 px-0.5">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          In this style
        </span>
        <span className="min-w-0 truncate font-mono text-[11px] text-muted-foreground">
          {hovered ? `${hovered.key} · ${hovered.uses}×` : `${colors.length} colors`}
        </span>
      </div>
      {sorted.length === 0 ? (
        <p className="px-0.5 py-2 text-xs text-muted-foreground">No colors yet.</p>
      ) : (
        <div className="grid max-h-48 grid-cols-10 gap-1 overflow-y-auto p-0.5">
          {sorted.map((p) => (
            <button
              aria-label={`${p.key}, used ${p.uses} times`}
              className={cn(
                'relative aspect-square w-full overflow-hidden rounded-[5px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-editor-accent',
                CHECKERBOARD,
                p.key === current && 'ring-2 ring-editor-accent ring-offset-1 ring-offset-popover',
              )}
              key={p.key}
              onBlur={() => setHovered(null)}
              onClick={() => onPick(p.color)}
              onFocus={() => setHovered(p)}
              onMouseEnter={() => setHovered(p)}
              onMouseLeave={() => setHovered(null)}
              title={`${p.key} · used ${p.uses}×`}
              type="button"
            >
              <span className="absolute inset-0" style={{ backgroundColor: toCss(p.color) }} />
              <span className="absolute inset-0 rounded-[5px] ring-1 ring-inset ring-border" />
            </button>
          ))}
        </div>
      )}
      <button
        className="mt-2 flex h-7 w-full items-center gap-1.5 rounded-md border px-2 text-xs hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
        onClick={onCustom}
        type="button"
      >
        <Palette className="size-3.5 text-muted-foreground" />
        Custom color…
      </button>
    </div>
  );
}
