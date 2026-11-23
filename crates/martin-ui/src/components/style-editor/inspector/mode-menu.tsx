import { Braces, ChartSpline, ChevronDown, Equal, ListTree, ZoomIn } from 'lucide-react';
import type { ComponentType } from 'react';
import { cn } from '@/lib/utils';
import type { ValueMode } from '../value-model';
import { Popover } from './popover';

const MODES: Record<
  ValueMode,
  { icon: ComponentType<{ className?: string }>; label: string; hint: string }
> = {
  constant: { hint: 'Fixed value', icon: Equal, label: 'Fixed' },
  expression: { hint: 'Custom expression', icon: Braces, label: 'Expression' },
  match: { hint: 'Style by category of an attribute', icon: ListTree, label: 'By category' },
  ramp: { hint: 'Style across a number range', icon: ChartSpline, label: 'By number' },
  zoom: { hint: 'Style across zoom levels', icon: ZoomIn, label: 'By zoom' },
};

/** Props of the mode menu. */
export interface ModeMenuProps {
  /** Current mode of the property. */
  mode: ValueMode;
  /** Modes the property's spec allows. */
  modes: ValueMode[];
  /** Property name for the accessible label. */
  name: string;
  /** Called with the chosen mode. */
  onChange: (mode: ValueMode) => void;
}

/** Labeled dropdown button that opens the plain-language list of authoring modes a property supports. */
export function ModeMenu({ mode, modes, name, onChange }: ModeMenuProps) {
  const Current = MODES[mode].icon;
  return (
    <Popover
      panelClassName="right-0 top-full mt-1 w-64 p-1"
      trigger={({ open, toggle }) => (
        <button
          aria-expanded={open}
          aria-label={`Mode of ${name}: ${MODES[mode].label}`}
          className={cn(
            'flex h-6 shrink-0 items-center gap-1 whitespace-nowrap rounded-md border px-1.5 text-[11px] hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
            mode === 'constant'
              ? 'text-muted-foreground'
              : 'border-editor-accent/40 bg-background text-foreground [&>svg:first-child]:text-editor-accent',
          )}
          onClick={toggle}
          title={
            mode === 'constant'
              ? 'Fixed value. Click to style by zoom level or by feature data.'
              : `${MODES[mode].label}: ${MODES[mode].hint}`
          }
          type="button"
        >
          <Current className="size-3.5 shrink-0" />
          {mode === 'constant' ? (
            <span className="sr-only">{MODES[mode].label}</span>
          ) : (
            MODES[mode].label
          )}
          <ChevronDown className="size-3 shrink-0 opacity-60" />
        </button>
      )}
    >
      {(close) =>
        modes.map((m) => {
          const Icon = MODES[m].icon;
          return (
            <button
              className={cn(
                'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-muted',
                m === mode && 'text-editor-accent',
              )}
              key={m}
              onClick={() => {
                close();
                if (m !== mode) onChange(m);
              }}
              type="button"
            >
              <Icon className="size-3.5 shrink-0" />
              <span className="min-w-0">
                <span className="block font-medium">{MODES[m].label}</span>
                <span className="block text-[11px] text-muted-foreground">{MODES[m].hint}</span>
              </span>
            </button>
          );
        })
      }
    </Popover>
  );
}
