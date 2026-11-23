import type { SourceSpecification } from '@maplibre/maplibre-gl-style-spec';
import { RotateCcw } from 'lucide-react';
import { useState } from 'react';
import { cn } from '@/lib/utils';
import { LiteralEditor } from '../value-editors/literal-editor';
import { type SourceField, sourceRecord } from './sources-model';
import { FieldRow, IconButton, NumberInput, TextInput } from './ui';

function JsonField({
  name,
  value,
  onChange,
}: {
  name: string;
  value: unknown;
  onChange: (next: unknown) => void;
}) {
  const [draft, setDraft] = useState<string>();
  const text = JSON.stringify(value) ?? '';
  let invalid = false;
  if (draft !== undefined && draft.trim() !== '') {
    try {
      JSON.parse(draft);
    } catch {
      invalid = true;
    }
  }
  const commit = () => {
    if (draft !== undefined && !invalid && draft !== text) {
      onChange(draft.trim() === '' ? undefined : JSON.parse(draft));
    }
    setDraft(undefined);
  };
  return (
    <TextInput
      aria-invalid={invalid}
      aria-label={`${name} (JSON)`}
      className={invalid ? 'border-destructive' : undefined}
      mono
      onBlur={commit}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        else if (e.key === 'Escape') {
          setDraft(undefined);
          e.currentTarget.blur();
        }
      }}
      spellCheck={false}
      title={text}
      value={draft ?? text}
    />
  );
}

function FieldEditor({
  field,
  raw,
  onChange,
}: {
  field: SourceField;
  raw: unknown;
  onChange: (next: unknown, coalesce?: string) => void;
}) {
  const { name, spec } = field;
  const value = raw ?? spec.default;
  const scalar = spec.type === 'number' || spec.type === 'boolean' || spec.type === 'enum';
  const text =
    spec.type === 'string' ||
    spec.type === 'promoteId' ||
    (name === 'data' && (typeof value === 'string' || value === undefined));
  const numbers =
    spec.type === 'array' &&
    spec.value === 'number' &&
    Array.isArray(value) &&
    value.every((v) => typeof v === 'number');

  if (numbers) {
    return (
      <div
        className={cn(
          'grid grid-cols-2 gap-1',
          raw === undefined && '[&_input]:text-muted-foreground',
        )}
      >
        {(value as number[]).map((n, i) => (
          <NumberInput
            // biome-ignore lint/suspicious/noArrayIndexKey: positional numbers have no identity
            key={i}
            label={`${name} ${i + 1}`}
            onChange={(next) => {
              if (next !== undefined)
                onChange((value as number[]).map((v, j) => (j === i ? next : v)));
            }}
            value={n}
          />
        ))}
      </div>
    );
  }
  if (scalar && value !== undefined) {
    return (
      <LiteralEditor
        compact={spec.type === 'number'}
        isDefault={raw === undefined}
        onChange={onChange}
        spec={spec}
        value={value}
      />
    );
  }
  if (text && (typeof value === 'string' || value === undefined)) {
    return (
      <LiteralEditor
        isDefault={raw === undefined}
        onChange={(next) => onChange(next === '' ? undefined : next)}
        spec={{ type: 'string' }}
        value={value}
      />
    );
  }
  return <JsonField name={name} onChange={onChange} value={raw} />;
}

/** The typed fields of one source, each with its editor and a reset to the spec default. */
export function SourceFields({
  source,
  fields,
  onSet,
}: {
  /** The source being edited. */
  source: SourceSpecification;
  /** Fields of its type. */
  fields: SourceField[];
  /** Sets one field, or removes it when `next` is undefined. */
  onSet: (name: string, next: unknown, coalesce?: string) => void;
}) {
  const record = sourceRecord(source);
  return (
    <div className="space-y-2">
      {fields.map((field) => {
        const raw = record[field.name];
        return (
          <FieldRow
            action={
              raw === undefined ? undefined : (
                <IconButton
                  label={`Reset ${field.name}`}
                  onClick={() => onSet(field.name, undefined)}
                >
                  <RotateCcw className="size-3" />
                </IconButton>
              )
            }
            key={field.name}
            label={field.name}
          >
            <FieldEditor
              field={field}
              onChange={(next, coalesce) => onSet(field.name, next, coalesce)}
              raw={raw}
            />
          </FieldRow>
        );
      })}
    </div>
  );
}
