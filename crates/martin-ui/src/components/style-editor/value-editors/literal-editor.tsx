import { Plus, X } from 'lucide-react';
import { useState } from 'react';
import { Switch } from '@/components/ui/switch';
import { cn } from '@/lib/utils';
import { ColorField } from '../color-field';
import { Color, formatColor } from '../colors';
import type { PropertySpec } from '../style-spec';
import { FontStackPicker } from './pickers/font-picker';
import { IconPicker } from './pickers/icon-picker';

/** Props of the editor for one literal value of any v8 type. */
export interface LiteralEditorProps {
  /** Definition of the property. */
  spec: PropertySpec;
  /** Current literal, or the spec default when unset. */
  value: unknown;
  /** Muted styling when `value` is the spec default. */
  isDefault?: boolean;
  /** Narrow layout for table cells: colors drop the opacity input, numbers drop the slider. */
  compact?: boolean;
  /** Called with the new literal; `coalesce` merges rapid edits into one history entry. */
  onChange: (next: unknown, coalesce?: string) => void;
}

const control =
  'h-8 min-w-0 rounded-md border bg-background px-2 text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

const FALLBACK_COLOR = Color.parse('#000000') as Color;

function unitSuffix(units: string | undefined): string | undefined {
  if (!units) return undefined;
  const short: Record<string, string> = {
    degrees: '°',
    ems: 'em',
    'factor of the original icon size': '×',
    milliseconds: 'ms',
    pixels: 'px',
  };
  return short[units] ?? (units.length <= 4 ? units : undefined);
}

function stepFor(spec: PropertySpec): number {
  if (spec.maximum !== undefined && spec.maximum <= 1) return 0.05;
  return spec.type === 'number' && spec.units === 'pixels' ? 0.5 : 1;
}

function clamp(n: number, spec: PropertySpec): number {
  return Math.min(
    spec.maximum ?? Number.POSITIVE_INFINITY,
    Math.max(spec.minimum ?? Number.NEGATIVE_INFINITY, n),
  );
}

function toJson(value: unknown): string {
  return JSON.stringify(value) ?? '';
}

/** Picks the control from `spec.type`: color, number, enum, boolean, string, array, padding, formatted. */
export function LiteralEditor(props: LiteralEditorProps) {
  const { spec, value, isDefault, compact, onChange } = props;
  const muted = isDefault ? 'opacity-60' : undefined;

  switch (spec.type) {
    case 'color': {
      const parsed = typeof value === 'string' ? Color.parse(value) : undefined;
      return (
        <ColorField
          compact={compact}
          isDefault={isDefault}
          onChange={(next, coalesce) => onChange(formatColor(next), coalesce)}
          value={parsed ?? FALLBACK_COLOR}
        />
      );
    }
    case 'number':
      if (typeof value === 'number') return <NumberControl {...props} value={value} />;
      break;
    case 'enum':
      if (spec.values && (typeof value === 'string' || value === undefined)) {
        return (
          <select
            className={cn(control, 'w-full', muted)}
            onChange={(e) => onChange(e.target.value)}
            value={typeof value === 'string' ? value : ''}
          >
            {typeof value !== 'string' && <option value="" />}
            {Object.entries(spec.values).map(([key, { doc }]) => (
              <option key={key} title={doc} value={key}>
                {key}
              </option>
            ))}
          </select>
        );
      }
      break;
    case 'boolean':
      return (
        <div className={cn('flex h-8 items-center', muted)}>
          <Switch
            checked={value === true}
            className="h-5 w-9"
            onCheckedChange={(next) => onChange(next)}
          />
        </div>
      );
    case 'resolvedImage':
      if (typeof value === 'string' || value === undefined) {
        return (
          <IconPicker
            compact={compact}
            isDefault={isDefault}
            onChange={(name) => onChange(name)}
            value={value}
          />
        );
      }
      break;
    case 'string':
    case 'formatted':
      if (typeof value === 'string' || value === undefined) {
        return <TextControl {...props} value={value ?? ''} />;
      }
      break;
    case 'array':
      if (Array.isArray(value)) {
        if (spec.value === 'number' && value.every((v) => typeof v === 'number')) {
          return <NumberArrayControl {...props} value={value as number[]} />;
        }
        if (spec.value === 'string' && value.every((v) => typeof v === 'string')) {
          return (
            <FontStackPicker
              isDefault={isDefault}
              onChange={(next) => onChange(next)}
              value={value as string[]}
            />
          );
        }
        if (spec.value === 'enum' && spec.values && value.every((v) => typeof v === 'string')) {
          return (
            <ToggleChips {...props} allowed={Object.keys(spec.values)} value={value as string[]} />
          );
        }
      }
      break;
    default:
  }
  return <JsonControl {...props} />;
}

function TextControl({ value, isDefault, onChange }: LiteralEditorProps & { value: string }) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft !== null && draft !== value) onChange(draft);
    setDraft(null);
  };
  return (
    <input
      className={cn(control, 'w-full', isDefault && 'opacity-60')}
      onBlur={commit}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        else if (e.key === 'Escape') {
          setDraft(null);
          e.currentTarget.blur();
        }
      }}
      title={value}
      type="text"
      value={draft ?? value}
    />
  );
}

function NumberInput({
  value,
  spec,
  onChange,
  className,
  suffix,
}: {
  value: number;
  spec: PropertySpec;
  onChange: (n: number, coalesce?: string) => void;
  className?: string;
  suffix?: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <div className={cn('relative min-w-0', className)}>
      <input
        className={cn(control, 'w-full tabular-nums', suffix && 'pr-6')}
        max={spec.maximum}
        min={spec.minimum}
        onBlur={() => setDraft(null)}
        onChange={(e) => {
          setDraft(e.target.value);
          const n = Number.parseFloat(e.target.value);
          if (Number.isFinite(n)) onChange(clamp(n, spec), 'number-input');
        }}
        step={stepFor(spec)}
        type="number"
        value={draft ?? value}
      />
      {suffix ? (
        <span className="pointer-events-none absolute inset-y-0 right-2 flex items-center text-[11px] text-muted-foreground">
          {suffix}
        </span>
      ) : null}
    </div>
  );
}

function NumberControl({
  spec,
  value,
  isDefault,
  compact,
  onChange,
}: LiteralEditorProps & { value: number }) {
  const bounded = !compact && spec.minimum !== undefined && spec.maximum !== undefined;
  return (
    <div className={cn('flex h-8 w-full min-w-0 items-center gap-2', isDefault && 'opacity-60')}>
      {bounded ? (
        <input
          aria-label="Slider"
          className="h-1 min-w-8 flex-1 cursor-pointer accent-editor-accent"
          max={spec.maximum}
          min={spec.minimum}
          onChange={(e) => onChange(Number.parseFloat(e.target.value), 'number-slider')}
          step={stepFor(spec) / 5}
          type="range"
          value={value}
        />
      ) : null}
      <NumberInput
        className={bounded ? 'w-24 shrink-0' : 'w-full'}
        onChange={onChange}
        spec={spec}
        suffix={unitSuffix(spec.units)}
        value={value}
      />
    </div>
  );
}

function NumberArrayControl({
  spec,
  value,
  isDefault,
  onChange,
}: LiteralEditorProps & { value: number[] }) {
  const fixed = spec.length !== undefined;
  const set = (i: number, n: number, coalesce?: string) =>
    onChange(
      value.map((v, j) => (j === i ? n : v)),
      coalesce,
    );
  return (
    <div
      className={cn('flex w-full min-w-0 flex-wrap items-center gap-1', isDefault && 'opacity-60')}
    >
      {value.map((n, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: positional numbers have no identity
        <div className="flex min-w-0 flex-1 basis-12 items-center gap-0.5" key={i}>
          <NumberInput
            className="min-w-0 flex-1"
            onChange={(next, coalesce) => set(i, next, `${coalesce}-${i}`)}
            spec={{ ...spec, maximum: undefined, minimum: undefined, type: 'number' }}
            value={n}
          />
          {!fixed && (
            <button
              aria-label="Remove element"
              className="shrink-0 rounded p-0.5 text-muted-foreground hover:text-foreground"
              onClick={() => onChange(value.filter((_, j) => j !== i))}
              type="button"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>
      ))}
      {!fixed && (
        <button
          aria-label="Add element"
          className="flex size-8 shrink-0 items-center justify-center rounded-md border text-muted-foreground hover:text-foreground"
          onClick={() => onChange([...value, value.at(-1) ?? 1])}
          type="button"
        >
          <Plus className="size-3.5" />
        </button>
      )}
    </div>
  );
}

function ToggleChips({
  allowed,
  value,
  isDefault,
  onChange,
}: LiteralEditorProps & { allowed: string[]; value: string[] }) {
  return (
    <div className={cn('flex w-full min-w-0 flex-wrap gap-1', isDefault && 'opacity-60')}>
      {allowed.map((name) => {
        const on = value.includes(name);
        return (
          <button
            aria-pressed={on}
            className={cn(
              'rounded-md border px-1.5 py-0.5 font-mono text-[11px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
              on
                ? 'border-editor-accent bg-editor-accent text-white'
                : 'bg-background text-muted-foreground',
            )}
            key={name}
            onClick={() => onChange(on ? value.filter((v) => v !== name) : [...value, name])}
            type="button"
          >
            {name}
          </button>
        );
      })}
    </div>
  );
}

function JsonControl({ value, isDefault, onChange }: LiteralEditorProps) {
  const [draft, setDraft] = useState<string | null>(null);
  const text = toJson(value);
  const invalid = (() => {
    if (draft === null) return false;
    try {
      JSON.parse(draft);
      return false;
    } catch {
      return true;
    }
  })();
  return (
    <input
      aria-invalid={invalid}
      className={cn(
        control,
        'w-full font-mono',
        invalid && 'border-destructive',
        isDefault && 'opacity-60',
      )}
      onBlur={() => {
        if (draft !== null && !invalid) {
          if (draft !== text) onChange(JSON.parse(draft));
        }
        setDraft(null);
      }}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        else if (e.key === 'Escape') {
          setDraft(null);
          e.currentTarget.blur();
        }
      }}
      spellCheck={false}
      title={text}
      type="text"
      value={draft ?? text}
    />
  );
}
