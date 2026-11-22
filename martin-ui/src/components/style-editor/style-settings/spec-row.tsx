import { latest } from '@maplibre/maplibre-gl-style-spec';
import { RotateCcw } from 'lucide-react';
import type { PropertySpec } from '../style-spec';
import { LiteralEditor } from '../value-editors/literal-editor';
import { FieldRow, Hint, IconButton } from './ui';

/** The v8 definition of one entry of a root-level table such as `$root`, `sky` or `transition`. */
export function rootSpec(
  table: string,
  name: string,
  overrides?: Partial<PropertySpec>,
): PropertySpec {
  const tables = latest as unknown as Record<string, Record<string, PropertySpec>>;
  return { ...tables[table][name], ...overrides };
}

/** Whether `value` is a plain literal the spec's editor can show, as opposed to an expression. */
export function isLiteral(spec: PropertySpec, value: unknown): boolean {
  switch (spec.type) {
    case 'number':
      return typeof value === 'number';
    case 'color':
    case 'string':
    case 'enum':
      return typeof value === 'string';
    case 'array':
      return Array.isArray(value) && value.every((v) => typeof v === 'number');
    default:
      return false;
  }
}

/** The icon button that unsets one field. */
export function ResetButton({ name, onReset }: { name: string; onReset: () => void }) {
  return (
    <IconButton label={`Reset ${name}`} onClick={onReset}>
      <RotateCcw className="size-3" />
    </IconButton>
  );
}

/** Props of {@link SpecRow}. */
export interface SpecRowProps {
  /** Text shown in the label column. */
  label: string;
  /** Plain-language explanation shown in the label tooltip. */
  hint: string;
  /** v8 definition used to pick the editor, bounds and default. */
  spec: PropertySpec;
  /** Raw value in the style, `undefined` when unset. */
  value: unknown;
  /** Writes a new literal; `coalesce` merges rapid edits. */
  onChange: (next: unknown, coalesce?: string) => void;
  /** Unsets the field; omit to hide the reset button. */
  onReset?: () => void;
  /** Accessible name for the reset button, defaults to `label`. */
  resetName?: string;
}

/** A {@link FieldRow} whose editor follows a v8 spec; numbers get no slider. */
export function SpecRow({ label, hint, spec, value, onChange, onReset, resetName }: SpecRowProps) {
  const isSet = value !== undefined;
  const shown = isSet ? value : spec.default;
  const readOnly = isSet && !isLiteral(spec, value);
  return (
    <div className="space-y-1">
      <FieldRow
        action={
          isSet && onReset ? <ResetButton name={resetName ?? label} onReset={onReset} /> : undefined
        }
        hint={hint}
        label={label}
      >
        {readOnly ? (
          <code
            className="block truncate rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-[11px]"
            title={JSON.stringify(value)}
          >
            {JSON.stringify(value)}
          </code>
        ) : (
          <LiteralEditor
            compact={spec.type === 'number'}
            isDefault={!isSet}
            onChange={onChange}
            spec={spec}
            value={shown}
          />
        )}
      </FieldRow>
      {readOnly ? (
        <Hint>Expression, read-only here. Reset to replace it with a plain value.</Hint>
      ) : null}
    </div>
  );
}
