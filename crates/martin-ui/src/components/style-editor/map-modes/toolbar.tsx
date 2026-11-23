import {
  Check,
  ChevronDown,
  Columns2,
  Eye,
  type LucideIcon,
  Paintbrush,
  ScanEye,
  SquareMousePointer,
} from 'lucide-react';
import { useState } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { PopoverMenu } from '../layer-list/menu';

/** What the map shows: the style, the raw data, or two versions of the style side by side. */
export type MapMode = 'styled' | 'xray' | 'compare';

const VISION_MATRICES = {
  deuteranopia: '0.625 0.375 0 0 0 0.7 0.3 0 0 0 0 0.3 0.7 0 0 0 0 0 1 0',
  protanopia: '0.567 0.433 0 0 0 0.558 0.442 0 0 0 0 0.242 0.758 0 0 0 0 0 1 0',
  tritanopia: '0.95 0.05 0 0 0 0 0.433 0.567 0 0 0 0.475 0.525 0 0 0 0 0 1 0',
} as const;

/** Simulated color vision applied to the map canvas. */
export type Vision = keyof typeof VISION_MATRICES | 'normal' | 'grayscale';

const VISIONS: readonly Vision[] = [
  'normal',
  'protanopia',
  'deuteranopia',
  'tritanopia',
  'grayscale',
];

function visionLabel(vision: Vision): string {
  return vision === 'normal' ? 'Vision' : vision;
}

/** CSS filter that simulates `vision`, or `undefined` for normal vision. */
export function visionFilter(vision: Vision): string | undefined {
  if (vision === 'normal') return undefined;
  if (vision === 'grayscale') return 'grayscale(1)';
  return `url(#se-vision-${vision})`;
}

/** SVG filter definitions referenced by {@link visionFilter}; render once per page. */
export function VisionFilters() {
  return (
    <svg aria-hidden="true" className="absolute size-0">
      {Object.entries(VISION_MATRICES).map(([k, m]) => (
        <filter id={`se-vision-${k}`} key={k}>
          <feColorMatrix type="matrix" values={m} />
        </filter>
      ))}
    </svg>
  );
}

const MODES: readonly {
  mode: MapMode;
  label: string;
  icon: LucideIcon;
  hint: string;
  key: string;
}[] = [
  {
    hint: 'The map as the style draws it',
    icon: Paintbrush,
    key: 'S',
    label: 'Styled',
    mode: 'styled',
  },
  { hint: 'Every source layer, unstyled', icon: ScanEye, key: 'X', label: 'X-ray', mode: 'xray' },
  {
    hint: 'An earlier version next to the current one',
    icon: Columns2,
    key: 'C',
    label: 'Compare',
    mode: 'compare',
  },
];

/** Keyboard shortcut for each map mode, lower-case. */
export const MODE_KEYS: Readonly<Record<string, MapMode>> = Object.fromEntries(
  MODES.map((m) => [m.key.toLowerCase(), m.mode]),
);

/** Keyboard shortcut that toggles inspect, lower-case. */
export const INSPECT_KEY = 'i';

/** Props of {@link MapToolbar}. */
export interface MapToolbarProps {
  /** Current map mode. */
  mode: MapMode;
  /** Switches the map mode. */
  onMode: (mode: MapMode) => void;
  /** Whether clicking the map inspects features instead of selecting layers. */
  inspect: boolean;
  /** Turns inspect on or off; `undefined` when the mode does not allow toggling it. */
  onInspect?: (on: boolean) => void;
  /** Simulated color vision. */
  vision: Vision;
  /** Changes the simulated color vision. */
  onVision: (vision: Vision) => void;
}

const SEGMENT =
  'flex h-7 items-center gap-1.5 rounded-md px-2 text-[11px] font-medium transition-colors outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

/** Floating bar above the map that picks the map mode, inspect and color vision. */
export function MapToolbar({
  mode,
  onMode,
  inspect,
  onInspect,
  vision,
  onVision,
}: MapToolbarProps) {
  const [visionMenu, setVisionMenu] = useState<{ x: number; y: number } | null>(null);
  return (
    <div className="pointer-events-auto flex items-center gap-1 rounded-lg border bg-card/95 p-0.5 shadow-sm backdrop-blur">
      <fieldset className="flex gap-0.5">
        <legend className="sr-only">Map mode</legend>
        {MODES.map(({ mode: m, label, icon: Icon, hint, key }) => (
          <Tooltip key={m}>
            <TooltipTrigger asChild>
              <button
                aria-pressed={mode === m}
                className={cn(
                  SEGMENT,
                  mode === m
                    ? 'bg-foreground text-background'
                    : 'text-muted-foreground hover:text-foreground',
                )}
                onClick={() => onMode(m)}
                type="button"
              >
                <Icon aria-hidden="true" className="size-3.5" />
                {label}
              </button>
            </TooltipTrigger>
            <TooltipContent>
              {hint} <kbd className="ml-1 font-mono opacity-70">{key}</kbd>
            </TooltipContent>
          </Tooltip>
        ))}
      </fieldset>
      <span aria-hidden="true" className="mx-0.5 h-4 w-px bg-border" />
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            aria-disabled={!onInspect}
            aria-pressed={inspect}
            className={cn(
              SEGMENT,
              inspect
                ? 'bg-foreground text-background'
                : 'text-muted-foreground hover:text-foreground',
              !onInspect && 'cursor-default',
              !onInspect && !inspect && 'opacity-50',
            )}
            onClick={() => onInspect?.(!inspect)}
            type="button"
          >
            <SquareMousePointer aria-hidden="true" className="size-3.5" />
            Inspect
          </button>
        </TooltipTrigger>
        <TooltipContent>
          {mode === 'compare' ? (
            'Inspect works in Styled and X-ray'
          ) : mode === 'xray' ? (
            'X-ray always inspects: click a feature to see its raw properties'
          ) : (
            <>
              Click a feature to see its raw properties and which filters match it{' '}
              <kbd className="ml-1 font-mono opacity-70">I</kbd>
            </>
          )}
        </TooltipContent>
      </Tooltip>
      <span aria-hidden="true" className="mx-0.5 h-4 w-px bg-border" />
      <button
        aria-expanded={visionMenu !== null}
        aria-haspopup="menu"
        className={cn(
          SEGMENT,
          'capitalize',
          vision === 'normal'
            ? 'text-muted-foreground hover:text-foreground'
            : 'text-editor-accent',
        )}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setVisionMenu({ x: r.right, y: r.bottom + 4 });
        }}
        type="button"
      >
        <Eye aria-hidden="true" className="size-3.5" />
        {visionLabel(vision)}
        <ChevronDown aria-hidden="true" className="size-3" />
      </button>
      {visionMenu ? (
        <PopoverMenu
          items={VISIONS.map((v) => ({
            icon: <Check className={cn('size-3.5', v !== vision && 'invisible')} />,
            label: v[0].toUpperCase() + v.slice(1),
            onChoose: () => onVision(v),
          }))}
          label="Color vision simulation"
          onClose={() => setVisionMenu(null)}
          x={visionMenu.x}
          y={visionMenu.y}
        />
      ) : null}
    </div>
  );
}
