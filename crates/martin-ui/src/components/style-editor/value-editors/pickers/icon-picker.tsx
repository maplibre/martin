import { ChevronDown } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@/lib/utils';
import { useStyleResources } from '../../style-resources';
import { IconPopup } from './icon/icon-popup';
import { IconThumb } from './icon/icon-thumb';
import { useSpriteIcons } from './icon/sprite-sheets';

/** Props of the sprite icon picker. */
export interface IconPickerProps {
  /** Current icon name, or undefined when unset. */
  value: string | undefined;
  /** Called with the chosen icon name. */
  onChange: (name: string) => void;
  /** Muted styling when the value is the spec default. */
  isDefault?: boolean;
  /** Narrow layout for table cells. */
  compact?: boolean;
}

const WIDTH = 360;
const MAX_HEIGHT = 420;
const MARGIN = 8;

interface Placement {
  top: number;
  left: number;
  width: number;
  height: number;
}

function place(trigger: DOMRect): Placement {
  const width = Math.min(WIDTH, window.innerWidth - 2 * MARGIN);
  const below = window.innerHeight - trigger.bottom - MARGIN;
  const above = trigger.top - MARGIN;
  const height = Math.min(MAX_HEIGHT, Math.max(below, above));
  const left = Math.max(MARGIN, Math.min(trigger.left, window.innerWidth - width - MARGIN));
  const top = below >= height ? trigger.bottom + 4 : trigger.top - height - 4;
  return { height, left, top, width };
}

/** Field showing the current icon that opens a searchable grid of the style's sprite icons. */
export function IconPicker({ value, onChange, isDefault, compact }: IconPickerProps) {
  const { sprite } = useStyleResources();
  const icons = useSpriteIcons(sprite);
  const [open, setOpen] = useState(false);
  const [placement, setPlacement] = useState<Placement | undefined>();
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const current = icons.icons.find((i) => i.name === value);

  useLayoutEffect(() => {
    if (open && trigger.current) setPlacement(place(trigger.current.getBoundingClientRect()));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (!popup.current?.contains(target) && !trigger.current?.contains(target)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        close();
        trigger.current?.focus();
      }
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKey);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', close);
    };
  }, [open]);

  const label = value || 'none';
  return (
    <>
      <button
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label="Choose an icon"
        className={cn(
          'flex w-full min-w-0 items-center gap-2 rounded-md border bg-background px-1.5 text-xs outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-editor-accent',
          compact ? 'h-7' : 'h-8',
          isDefault && 'text-muted-foreground',
        )}
        onClick={() => setOpen((o) => !o)}
        ref={trigger}
        title={label}
        type="button"
      >
        {current ? (
          <IconThumb box={20} icon={current} />
        ) : (
          <span aria-hidden className="size-5 shrink-0 rounded border border-dashed" />
        )}
        <span className={cn('min-w-0 flex-1 truncate text-left font-mono', !value && 'italic')}>
          {label}
        </span>
        <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
      </button>
      {open && placement
        ? createPortal(
            <div
              aria-label="Icons in this style"
              className="fixed z-[100] flex flex-col overflow-hidden rounded-lg border bg-popover text-popover-foreground shadow-lg"
              ref={popup}
              role="dialog"
              style={placement}
            >
              <IconPopup
                current={value}
                onPick={(name) => {
                  onChange(name);
                  setOpen(false);
                  trigger.current?.focus();
                }}
                sprite={icons}
              />
            </div>,
            document.body,
          )
        : null}
    </>
  );
}
