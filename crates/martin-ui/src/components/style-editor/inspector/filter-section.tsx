import { ChevronRight, Plus, X } from 'lucide-react';
import { useId, useState } from 'react';
import { cn } from '@/lib/utils';
import type { AttributeInfo } from '../source-schema';
import { ExpressionEditor } from '../value-editors/expression-editor';
import {
  changeOperator,
  coerceValue,
  describeCondition,
  FILTER_SPEC,
  type FilterCondition,
  type FilterScalar,
  GEOMETRY_TYPE,
  GEOMETRY_TYPES,
  OPERATOR_LABEL,
  OPERATORS,
  operatorAllowed,
  parseFilter,
  serializeFilter,
  validateFilter,
} from './filter-model';

/** Props of the filter section. */
export interface FilterSectionProps {
  /** The layer's filter as stored, `undefined` when it has none. */
  filter: unknown;
  /** Attributes of the layer's source layer. */
  attributes: AttributeInfo[];
  /** Sets the layer filter; `undefined` removes it. */
  onFilter: (filter: unknown) => void;
}

const FIELD =
  'h-7 min-w-0 rounded-md border bg-background px-2 text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

const OTHER = '\u0000other';

function text(value: FilterScalar): string {
  return String(value);
}

function CommitInput({
  value,
  onCommit,
  label,
  placeholder,
  list,
  className,
}: {
  value: string;
  onCommit: (next: string) => void;
  label: string;
  placeholder?: string;
  list?: string;
  className?: string;
}) {
  const [draft, setDraft] = useState<string | undefined>();
  const commit = () => {
    if (draft !== undefined && draft !== value) onCommit(draft);
    setDraft(undefined);
  };
  return (
    <input
      aria-label={label}
      className={cn(FIELD, 'font-mono', className)}
      list={list}
      onBlur={commit}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
        if (e.key === 'Escape') setDraft(undefined);
      }}
      placeholder={placeholder}
      title={draft ?? value}
      value={draft ?? value}
    />
  );
}

function AttributeField({
  attribute,
  attributes,
  onChange,
}: {
  attribute: string;
  attributes: AttributeInfo[];
  onChange: (next: string) => void;
}) {
  const [typing, setTyping] = useState(false);
  const known = attribute === GEOMETRY_TYPE || attributes.some((a) => a.name === attribute);
  if (typing || attributes.length === 0 || (attribute !== '' && !known)) {
    return (
      <CommitInput
        className="w-full"
        label="Attribute name"
        onCommit={onChange}
        placeholder="attribute"
        value={attribute}
      />
    );
  }
  return (
    <select
      aria-label="Attribute"
      className={cn(FIELD, 'w-full font-mono')}
      onChange={(e) => {
        if (e.target.value === OTHER) setTyping(true);
        else onChange(e.target.value);
      }}
      title={attribute}
      value={attribute}
    >
      {attribute === '' && <option value="">Choose attribute</option>}
      <option value={GEOMETRY_TYPE}>geometry type</option>
      {attributes.map((a) => (
        <option key={a.name} value={a.name}>
          {a.name}
        </option>
      ))}
      <option value={OTHER}>Other attribute...</option>
    </select>
  );
}

function SuggestionList({ id, values }: { id: string; values: FilterScalar[] }) {
  return (
    <datalist id={id}>
      {values.slice(0, 100).map((v) => (
        <option key={text(v)} value={text(v)} />
      ))}
    </datalist>
  );
}

function ValueField({
  condition,
  info,
  onChange,
}: {
  condition: FilterCondition;
  info: AttributeInfo | undefined;
  onChange: (next: FilterCondition) => void;
}) {
  const listId = useId();
  const geometry = condition.attribute === GEOMETRY_TYPE;
  const suggestions: FilterScalar[] = geometry ? [...GEOMETRY_TYPES] : (info?.values ?? []);

  if ('values' in condition) {
    const add = (raw: string) => {
      const value = geometry ? raw : coerceValue(raw, info);
      if (raw === '' || condition.values.includes(value)) return;
      onChange({ ...condition, values: [...condition.values, value] });
    };
    return (
      <div className="flex min-w-0 flex-wrap items-center gap-1">
        {condition.values.map((v) => (
          <span
            className="flex max-w-full items-center gap-0.5 rounded-md border bg-muted py-0.5 pr-0.5 pl-1.5 font-mono text-[11px]"
            key={text(v)}
          >
            <span className="min-w-0 truncate" title={text(v)}>
              {text(v)}
            </span>
            <button
              aria-label={`Remove ${text(v)}`}
              className="flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-background hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
              onClick={() =>
                onChange({ ...condition, values: condition.values.filter((x) => x !== v) })
              }
              type="button"
            >
              <X className="size-3" />
            </button>
          </span>
        ))}
        <input
          aria-label="Add value"
          className={cn(FIELD, 'h-6 min-w-24 flex-1 font-mono')}
          list={listId}
          onBlur={(e) => {
            add(e.currentTarget.value.trim());
            e.currentTarget.value = '';
          }}
          onChange={(e) => {
            const picked = suggestions.find((s) => text(s) === e.target.value);
            if (picked !== undefined && !condition.values.includes(picked)) {
              onChange({ ...condition, values: [...condition.values, picked] });
              e.target.value = '';
            }
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ',') {
              e.preventDefault();
              add(e.currentTarget.value.trim());
              e.currentTarget.value = '';
            }
          }}
          placeholder={condition.values.length === 0 ? 'Add a value' : '+'}
        />
        <SuggestionList id={listId} values={suggestions} />
      </div>
    );
  }

  if ('value' in condition) {
    const ordering = ['lt', 'lte', 'gt', 'gte'].includes(condition.operator);
    return (
      <>
        <CommitInput
          className="w-full"
          label="Value"
          list={listId}
          onCommit={(raw) => {
            const value = geometry ? raw : ordering ? Number(raw) : coerceValue(raw, info);
            if (typeof value === 'number' && !Number.isFinite(value)) return;
            onChange({ ...condition, value });
          }}
          placeholder="value"
          value={text(condition.value)}
        />
        <SuggestionList id={listId} values={suggestions} />
      </>
    );
  }
  return null;
}

function ConditionRow({
  condition,
  attributes,
  onChange,
  onRemove,
}: {
  condition: FilterCondition;
  attributes: AttributeInfo[];
  onChange: (next: FilterCondition) => void;
  onRemove: () => void;
}) {
  const info = attributes.find((a) => a.name === condition.attribute);
  const operators = OPERATORS.filter((o) => operatorAllowed(condition.attribute, o));
  return (
    <li className="space-y-1.5 rounded-md border bg-background/50 p-2">
      <div className="grid grid-cols-[minmax(0,1fr)_auto_20px] items-center gap-1.5">
        <AttributeField
          attribute={condition.attribute}
          attributes={attributes}
          onChange={(attribute) => {
            const next = { ...condition, attribute };
            onChange(
              operatorAllowed(attribute, condition.operator) ? next : changeOperator(next, 'is'),
            );
          }}
        />
        <select
          aria-label="Operator"
          className={cn(FIELD, 'w-[8.5rem]')}
          onChange={(e) => {
            const next = OPERATORS.find((o) => o === e.target.value);
            if (next) onChange(changeOperator(condition, next));
          }}
          value={condition.operator}
        >
          {operators.map((o) => (
            <option key={o} value={o}>
              {OPERATOR_LABEL[o]}
            </option>
          ))}
        </select>
        <button
          aria-label="Remove condition"
          className="flex size-5 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={onRemove}
          title="Remove condition"
          type="button"
        >
          <X className="size-3.5" />
        </button>
      </div>
      <ValueField condition={condition} info={info} onChange={onChange} />
    </li>
  );
}

const SECTION_HEADING = 'text-[11px] font-semibold uppercase tracking-wider text-muted-foreground';

/** Layer filter as a list of conditions joined by "all", with a switch to the raw expression. */
export function FilterSection({ filter, attributes, onFilter }: FilterSectionProps) {
  const incoming = JSON.stringify(filter ?? null);
  const model = parseFilter(filter);
  const [conditions, setConditions] = useState<FilterCondition[]>(
    model.mode === 'structured' ? model.conditions : [],
  );
  const [seen, setSeen] = useState(incoming);
  const [raw, setRaw] = useState(false);
  const [open, setOpen] = useState(false);

  if (incoming !== seen) {
    setSeen(incoming);
    setConditions(model.mode === 'structured' ? model.conditions : []);
  }

  const commit = (next: FilterCondition[]) => {
    const serialized = serializeFilter({ conditions: next, mode: 'structured' });
    const key = JSON.stringify(serialized ?? null);
    setConditions(next);
    setSeen(key);
    if (key !== incoming) onFilter(serialized);
  };

  const expressionMode = raw || model.mode === 'expression';
  const errors = expressionMode ? [] : validateFilter(filter);

  const summary =
    filter === undefined
      ? 'None, every feature is drawn'
      : model.mode === 'expression'
        ? 'Custom expression'
        : conditions.map(describeCondition).join(' · ');

  return (
    <section className="space-y-2 px-3 py-3">
      <div className="flex items-center justify-between gap-2">
        <button
          aria-expanded={open}
          className="flex min-w-0 flex-1 items-start gap-1.5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={() => setOpen((o) => !o)}
          type="button"
        >
          <ChevronRight
            className={cn(
              'size-3.5 shrink-0 text-muted-foreground transition-transform',
              open && 'rotate-90',
            )}
          />
          <h3 className={SECTION_HEADING}>Filter</h3>
          {open ? null : (
            <span
              className="line-clamp-2 min-w-0 break-words font-mono text-[11px] text-muted-foreground"
              title={summary}
            >
              {summary}
            </span>
          )}
        </button>
        {open ? (
          <label
            className={cn(
              'flex items-center gap-1.5 text-[11px] text-muted-foreground',
              model.mode === 'expression' && 'opacity-70',
            )}
            title={
              model.mode === 'expression'
                ? 'This filter cannot be shown as conditions'
                : 'Edit the filter as an expression'
            }
          >
            Edit as expression
            <input
              checked={expressionMode}
              className="size-3.5 accent-editor-accent"
              disabled={model.mode === 'expression'}
              onChange={(e) => setRaw(e.target.checked)}
              type="checkbox"
            />
          </label>
        ) : null}
      </div>
      {!open ? null : expressionMode ? (
        <>
          <ExpressionEditor
            name="filter"
            onChange={(next) => onFilter(next)}
            spec={FILTER_SPEC}
            value={filter ?? ['all']}
          />
          {filter !== undefined && (
            <button
              className="rounded-md border px-1.5 py-0.5 text-[11px] hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
              onClick={() => {
                onFilter(undefined);
                setRaw(false);
              }}
              type="button"
            >
              Remove filter
            </button>
          )}
        </>
      ) : (
        <>
          {conditions.length > 0 && (
            <ul className="space-y-1.5">
              {conditions.map((c, i) => (
                <ConditionRow
                  attributes={attributes}
                  condition={c}
                  // biome-ignore lint/suspicious/noArrayIndexKey: rows have no identity besides position
                  key={i}
                  onChange={(next) => commit(conditions.map((x, j) => (j === i ? next : x)))}
                  onRemove={() => commit(conditions.filter((_, j) => j !== i))}
                />
              ))}
            </ul>
          )}
          <div className="flex items-center justify-between gap-2">
            <button
              className="flex h-7 items-center gap-1 rounded-md border px-2 text-xs hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
              onClick={() => commit([...conditions, { attribute: '', operator: 'is', value: '' }])}
              type="button"
            >
              <Plus className="size-3.5" />
              Add condition
            </button>
            <p className="min-w-0 truncate text-[11px] text-muted-foreground">
              {conditions.length === 0
                ? 'No filter: every feature is drawn'
                : conditions.length === 1
                  ? 'Draws features matching this condition'
                  : 'Draws features matching all conditions'}
            </p>
          </div>
          {errors.map((e) => (
            <p className="break-words text-[11px] text-destructive" key={e}>
              {e}
            </p>
          ))}
        </>
      )}
    </section>
  );
}
