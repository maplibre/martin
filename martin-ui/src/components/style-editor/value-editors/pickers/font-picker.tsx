import { ChevronLeft, ChevronRight, Plus, X } from 'lucide-react';
import { useState } from 'react';
import { cn } from '@/lib/utils';
import { FontPopup } from './font/font-popup';

/** Props of the font stack picker. */
export interface FontStackPickerProps {
  /** Current font stack, first font preferred, later ones as fallbacks. */
  value: string[];
  /** Called with the new stack. */
  onChange: (next: string[]) => void;
  /** Muted styling when the value is the spec default. */
  isDefault?: boolean;
}

interface Open {
  anchor: HTMLElement;
  /** Index of the entry being replaced, or null when adding. */
  index: number | null;
}

function move<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length || from === to) return list;
  const next = [...list];
  next.splice(to, 0, ...next.splice(from, 1));
  return next;
}

/** Ordered font stack with a popup to browse and preview the fonts available to the style. */
export function FontStackPicker({ value, onChange, isDefault }: FontStackPickerProps) {
  const [open, setOpen] = useState<Open | null>(null);
  const [dragFrom, setDragFrom] = useState<number | null>(null);

  const toggle = (anchor: HTMLElement, index: number | null) =>
    setOpen((cur) => (cur?.anchor === anchor ? null : { anchor, index }));

  const pick = (name: string) => {
    if (open?.index != null) onChange(value.map((f, i) => (i === open.index ? name : f)));
    else if (!value.includes(name)) onChange([...value, name]);
    setOpen(null);
  };

  const iconButton =
    'inline-flex size-4 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-background hover:text-foreground disabled:opacity-30 disabled:hover:bg-transparent';

  return (
    <div className="flex w-full min-w-0 flex-wrap items-center gap-1">
      {value.map((name, i) => (
        // biome-ignore lint/a11y/noStaticElementInteractions: drag is a mouse extra, the arrow buttons cover the keyboard
        <div
          className={cn(
            'flex max-w-full min-w-0 items-center gap-0.5 rounded-md bg-muted py-0.5 pr-0.5 pl-1.5',
            isDefault && 'text-muted-foreground',
            dragFrom === i && 'opacity-50',
            open?.index === i && 'ring-1 ring-editor-accent',
          )}
          draggable
          // biome-ignore lint/suspicious/noArrayIndexKey: a stack may repeat a font name
          key={`${name}-${i}`}
          onDragEnd={() => setDragFrom(null)}
          onDragOver={(e) => e.preventDefault()}
          onDragStart={() => setDragFrom(i)}
          onDrop={() => {
            if (dragFrom !== null) onChange(move(value, dragFrom, i));
            setDragFrom(null);
          }}
        >
          {i === 0 && (
            <span className="shrink-0 rounded bg-editor-accent-soft px-1 text-[11px] text-editor-accent">
              primary
            </span>
          )}
          <button
            aria-label={`Replace ${name}`}
            className="min-w-0 truncate font-mono text-[11px]"
            onClick={(e) => toggle(e.currentTarget, i)}
            title={name}
            type="button"
          >
            {name}
          </button>
          {value.length > 1 && (
            <>
              <button
                aria-label={`Move ${name} earlier`}
                className={iconButton}
                disabled={i === 0}
                onClick={() => onChange(move(value, i, i - 1))}
                type="button"
              >
                <ChevronLeft className="size-3.5" />
              </button>
              <button
                aria-label={`Move ${name} later`}
                className={iconButton}
                disabled={i === value.length - 1}
                onClick={() => onChange(move(value, i, i + 1))}
                type="button"
              >
                <ChevronRight className="size-3.5" />
              </button>
            </>
          )}
          <button
            aria-label={`Remove ${name}`}
            className={iconButton}
            disabled={value.length === 1}
            onClick={() => onChange(value.filter((_, j) => j !== i))}
            type="button"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
      <button
        className="inline-flex items-center gap-0.5 rounded-md border border-dashed px-1.5 py-0.5 text-[11px] text-muted-foreground hover:border-editor-accent hover:text-editor-accent"
        onClick={(e) => toggle(e.currentTarget, null)}
        type="button"
      >
        <Plus className="size-3.5" />
        Add font
      </button>
      {open ? (
        <FontPopup anchor={open.anchor} onClose={() => setOpen(null)} onPick={pick} stack={value} />
      ) : null}
    </div>
  );
}
