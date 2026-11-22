import { type ReactNode, useEffect, useRef } from 'react';
import { cn } from '@/lib/utils';

/** One entry of a row menu. */
export interface MenuItem {
  /** Visible label. */
  label: string;
  /** Icon shown before the label. */
  icon: ReactNode;
  /** Runs when chosen. */
  onChoose: () => void;
  /** Greys the item out. */
  disabled?: boolean;
  /** Styles the item as destructive. */
  destructive?: boolean;
}

/** A small popover menu at a fixed screen position that closes on outside click, scroll and Escape. */
export function PopoverMenu({
  x,
  y,
  items,
  onClose,
}: {
  /** Right edge of the menu in viewport pixels. */
  x: number;
  /** Top edge of the menu in viewport pixels. */
  y: number;
  /** Entries to list. */
  items: MenuItem[];
  /** Called when the menu should close. */
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    const onPointer = (e: PointerEvent) => {
      if (e.target instanceof Node && !ref.current?.contains(e.target)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    document.addEventListener('pointerdown', onPointer, true);
    document.addEventListener('keydown', onKey, true);
    window.addEventListener('scroll', onClose, true);
    return () => {
      document.removeEventListener('pointerdown', onPointer, true);
      document.removeEventListener('keydown', onKey, true);
      window.removeEventListener('scroll', onClose, true);
    };
  }, [onClose]);

  const move = (dir: 1 | -1) => {
    const buttons = [
      ...(ref.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []),
    ];
    const active = document.activeElement;
    const at = active instanceof HTMLButtonElement ? buttons.indexOf(active) : -1;
    buttons[(at + dir + buttons.length) % buttons.length]?.focus();
  };

  return (
    <div
      aria-label="Layer actions"
      className="fixed z-50 w-36 -translate-x-full rounded-md border bg-popover p-1 text-popover-foreground shadow-md"
      onKeyDown={(e) => {
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault();
          move(e.key === 'ArrowDown' ? 1 : -1);
        }
      }}
      ref={ref}
      role="menu"
      style={{ left: x, top: y }}
    >
      {items.map((item) => (
        <button
          className={cn(
            'flex h-7 w-full items-center gap-2 rounded-sm px-2 text-left text-xs hover:bg-muted focus-visible:bg-muted focus-visible:outline-none disabled:pointer-events-none disabled:opacity-40',
            item.destructive && 'text-destructive',
          )}
          disabled={item.disabled}
          key={item.label}
          onClick={() => {
            onClose();
            item.onChoose();
          }}
          role="menuitem"
          type="button"
        >
          {item.icon}
          {item.label}
        </button>
      ))}
    </div>
  );
}
