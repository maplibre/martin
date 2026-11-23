import { type ReactNode, useEffect, useRef, useState } from 'react';
import { cn } from '@/lib/utils';

/** Props of the dismissible popover. */
export interface PopoverProps {
  /** Renders the trigger; call `toggle` to open or close the panel. */
  trigger: (state: { open: boolean; toggle: () => void }) => ReactNode;
  /** Renders the panel content; call `close` to dismiss it. */
  children: (close: () => void) => ReactNode;
  /** Classes for the panel, including its position relative to the wrapper. */
  panelClassName?: string;
  /** Classes for the wrapper element. */
  className?: string;
}

/** Minimal popover that closes on outside press and Escape. */
export function Popover({ trigger, children, panelClassName, className }: PopoverProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div className={cn('relative', className)} ref={ref}>
      {trigger({ open, toggle: () => setOpen((o) => !o) })}
      {open ? (
        <div
          className={cn(
            'absolute z-50 rounded-md border bg-popover text-popover-foreground shadow-md',
            panelClassName,
          )}
          ref={(panel) => panel?.scrollIntoView?.({ block: 'nearest' })}
        >
          {children(() => setOpen(false))}
        </div>
      ) : null}
    </div>
  );
}
