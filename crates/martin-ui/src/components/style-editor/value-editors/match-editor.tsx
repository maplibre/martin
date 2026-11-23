import { Plus, X } from 'lucide-react';
import { useId, useState } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { Color, categoricalColors, formatColor } from '../colors';
import { Popover } from '../inspector/popover';
import type { AttributeInfo } from '../source-schema';
import type { PropertySpec } from '../style-spec';
import type { MatchCase, MatchValue } from '../value-model';
import { LiteralEditor } from './literal-editor';

/** Props of the categorical `match` editor. */
export interface MatchEditorProps {
  /** Definition of the property. */
  spec: PropertySpec;
  /** The match. */
  value: MatchValue;
  /** Attributes of the layer's source layer, with observed values to suggest. */
  attributes: AttributeInfo[];
  /** Called with the new match; `coalesce` merges rapid edits into one history entry. */
  onChange: (next: MatchValue, coalesce?: string) => void;
}

type Label = string | number;

const MAX_UNUSED = 20;
const ROW = 'grid h-8 grid-cols-[minmax(0,1fr)_128px_20px] items-center gap-1';
const PICKABLE: AttributeInfo['kind'][] = ['string', 'number', 'mixed', 'unknown'];

const control =
  'h-7 rounded-md border bg-background px-2 text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';
const iconButton =
  'inline-flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent';

function usedLabels(cases: MatchCase[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const c of cases)
    for (const l of c.labels) counts.set(String(l), (counts.get(String(l)) ?? 0) + 1);
  return counts;
}

function toLabel(text: string, kind: AttributeInfo['kind']): Label | undefined {
  if (kind !== 'number') return text;
  const n = Number(text);
  return text.trim() !== '' && Number.isFinite(n) ? n : undefined;
}

interface LabelAdderProps {
  kind: AttributeInfo['kind'];
  suggestions: Label[];
  isTaken: (label: Label) => boolean;
  onAdd: (label: Label) => void;
}

function LabelAdder({ kind, suggestions, isTaken, onAdd }: LabelAdderProps) {
  const listId = useId();
  const [draft, setDraft] = useState('');
  const [problem, setProblem] = useState<string>();

  const submit = () => {
    const text = draft.trim();
    if (text === '') return;
    const label = toLabel(text, kind);
    if (label === undefined) {
      setProblem('Not a number');
    } else if (isTaken(label)) {
      setProblem('Already used by a case');
    } else {
      onAdd(label);
      setDraft('');
      setProblem(undefined);
    }
  };

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <input
        aria-label="Add label"
        className={cn(control, 'w-full font-mono text-[11px]')}
        inputMode={kind === 'number' ? 'decimal' : undefined}
        list={listId}
        onBlur={submit}
        onChange={(e) => {
          setDraft(e.target.value);
          setProblem(undefined);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ',') {
            e.preventDefault();
            submit();
          }
        }}
        placeholder="add value"
        value={draft}
      />
      <datalist id={listId}>
        {suggestions.slice(0, 100).map((s) => (
          <option key={String(s)} value={String(s)} />
        ))}
      </datalist>
      {problem !== undefined ? (
        <span className="text-[11px] text-muted-foreground">{problem}</span>
      ) : null}
      {suggestions.length > 0 && (
        <div className="flex max-h-20 flex-wrap gap-1 overflow-y-auto">
          {suggestions.slice(0, 12).map((s) => (
            <button
              className="max-w-full truncate rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground hover:text-foreground"
              key={String(s)}
              onClick={() => onAdd(s)}
              title={String(s)}
              type="button"
            >
              {String(s)}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

interface LabelsCellProps {
  labels: Label[];
  used: Map<string, number>;
  kind: AttributeInfo['kind'];
  suggestions: Label[];
  onChange: (labels: Label[]) => void;
}

function LabelsCell({ labels, used, kind, suggestions, onChange }: LabelsCellProps) {
  const shown = labels.slice(0, 2);
  const hidden = labels.length - shown.length;
  const all = labels.map(String).join(', ');
  return (
    <Popover
      className="min-w-0"
      panelClassName="left-0 top-full z-50 mt-1 w-60 p-2"
      trigger={({ toggle }) => (
        <button
          aria-label={`Edit values: ${all || 'none'}`}
          className="flex h-7 w-full min-w-0 items-center gap-1 rounded-md px-1 text-left hover:bg-muted focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={toggle}
          title={all}
          type="button"
        >
          {labels.length === 0 && (
            <span className="text-[11px] italic text-muted-foreground">no values</span>
          )}
          {shown.map((l) => (
            <span
              className={cn(
                'min-w-0 truncate rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] tabular-nums',
                (used.get(String(l)) ?? 0) > 1 && 'ring-1 ring-destructive',
              )}
              key={String(l)}
            >
              {String(l)}
            </span>
          ))}
          {hidden > 0 && (
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
                  +{hidden}
                </span>
              </TooltipTrigger>
              <TooltipContent className="max-w-64 break-words font-mono text-[11px]">
                {all}
              </TooltipContent>
            </Tooltip>
          )}
        </button>
      )}
    >
      {() => (
        <div className="flex flex-col gap-2">
          {labels.length > 0 && (
            <div className="flex max-h-28 min-w-0 flex-wrap gap-1 overflow-y-auto">
              {labels.map((l) => (
                <span
                  className={cn(
                    'inline-flex max-w-full items-center gap-0.5 rounded bg-muted py-0.5 pr-0.5 pl-1.5 font-mono text-[11px] tabular-nums',
                    (used.get(String(l)) ?? 0) > 1 && 'ring-1 ring-destructive',
                  )}
                  key={String(l)}
                >
                  <span className="min-w-0 truncate" title={String(l)}>
                    {String(l)}
                  </span>
                  <button
                    aria-label={`Remove ${l}`}
                    className="inline-flex size-4 shrink-0 items-center justify-center rounded text-muted-foreground hover:text-foreground"
                    onClick={() => onChange(labels.filter((x) => x !== l))}
                    type="button"
                  >
                    <X className="size-3" />
                  </button>
                </span>
              ))}
            </div>
          )}
          <LabelAdder
            isTaken={(l) => used.has(String(l))}
            kind={kind}
            onAdd={(l) => onChange([...labels, l])}
            suggestions={suggestions}
          />
        </div>
      )}
    </Popover>
  );
}

/** Attribute picker plus value to output rows plus the fallback. */
export function MatchEditor({ spec, value, attributes, onChange }: MatchEditorProps) {
  const [custom, setCustom] = useState(false);
  const pickable = attributes.filter((a) => PICKABLE.includes(a.kind));
  const info = attributes.find((a) => a.name === value.attribute);
  const kind = info?.kind ?? 'unknown';
  const used = usedLabels(value.cases);
  const duplicates = [...used.entries()].filter(([, n]) => n > 1).map(([l]) => l);

  const unused = (info?.values ?? []).filter(
    (v): v is Label =>
      (typeof v === 'number' || typeof v === 'string') &&
      !used.has(String(v)) &&
      (kind !== 'number' || typeof v === 'number'),
  );

  const setCase = (index: number, patch: Partial<MatchCase>, coalesce?: string) =>
    onChange(
      { ...value, cases: value.cases.map((c, i) => (i === index ? { ...c, ...patch } : c)) },
      coalesce,
    );

  const addCase = () =>
    onChange({ ...value, cases: [...value.cases, { labels: [], output: value.fallback }] });

  const addUnused = () =>
    onChange({ ...value, cases: [...value.cases, ...seedCases(unused.slice(0, MAX_UNUSED))] });

  const seedCases = (labels: Label[]): MatchCase[] => {
    const base = typeof value.fallback === 'string' ? Color.parse(value.fallback) : undefined;
    if (spec.type !== 'color' || base === undefined) {
      return labels.map((l) => ({ labels: [l], output: value.fallback }));
    }
    const colors = categoricalColors(base, labels.length + 1).slice(1);
    return labels.map((l, i) => ({ labels: [l], output: formatColor(colors[i] ?? base) }));
  };

  const attributeKnown = pickable.some((a) => a.name === value.attribute);

  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex min-w-0 items-center gap-1">
        {custom ? (
          <input
            aria-label="Attribute name"
            className={cn(control, 'min-w-0 flex-1 font-mono')}
            onChange={(e) => onChange({ ...value, attribute: e.target.value }, 'match-attribute')}
            placeholder="attribute name"
            value={value.attribute}
          />
        ) : (
          <select
            aria-label="Attribute"
            className={cn(control, 'min-w-0 flex-1 font-mono')}
            onChange={(e) => onChange({ ...value, attribute: e.target.value })}
            title={info?.description ?? value.attribute}
            value={value.attribute}
          >
            {!attributeKnown && (
              <option value={value.attribute}>{value.attribute || 'Pick an attribute'}</option>
            )}
            {pickable.map((a) => (
              <option key={a.name} title={a.description} value={a.name}>
                {a.name} ({a.kind}, {a.values.length})
              </option>
            ))}
          </select>
        )}
        <button
          aria-pressed={custom}
          className={cn(iconButton, custom && 'bg-editor-accent-soft text-editor-accent')}
          onClick={() => setCustom(!custom)}
          title="Type an attribute name"
          type="button"
        >
          <Plus className="size-3.5" />
        </button>
      </div>
      {info !== undefined && (
        <div className="text-[11px] text-muted-foreground">
          {info.values.length} values observed
        </div>
      )}

      <div className="flex min-w-0 flex-col">
        <div className={cn(ROW, 'mb-1 border-b pb-1')}>
          <span
            className="truncate px-1 text-[11px] italic text-muted-foreground"
            title="Used for every value not listed below"
          >
            Everything else
          </span>
          <LiteralEditor
            compact
            onChange={(fallback, key) => onChange({ ...value, fallback }, key ?? 'match-fallback')}
            spec={spec}
            value={value.fallback}
          />
          <span />
        </div>
        {value.cases.map((c, i) => (
          <div
            className={cn(ROW, 'border-b border-border/50')}
            // biome-ignore lint/suspicious/noArrayIndexKey: cases have no identity besides position
            key={i}
          >
            <LabelsCell
              kind={kind}
              labels={c.labels}
              onChange={(labels) => setCase(i, { labels })}
              suggestions={unused}
              used={used}
            />
            <LiteralEditor
              compact
              onChange={(output, key) => setCase(i, { output }, key ?? `match-case-${i}`)}
              spec={spec}
              value={c.output}
            />
            <button
              aria-label="Remove case"
              className="inline-flex size-5 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
              onClick={() => onChange({ ...value, cases: value.cases.filter((_, j) => j !== i) })}
              type="button"
            >
              <X className="size-3" />
            </button>
          </div>
        ))}
      </div>

      {duplicates.length > 0 && (
        <div
          className="break-words text-[11px] text-muted-foreground"
          title={duplicates.join(', ')}
        >
          Duplicate labels are rejected by MapLibre: {duplicates.join(', ')}
        </div>
      )}

      <div className="flex flex-wrap gap-1">
        <button
          className={cn(control, 'inline-flex items-center gap-1 hover:bg-muted')}
          onClick={addCase}
          type="button"
        >
          <Plus className="size-3.5" />
          Add case
        </button>
        {unused.length > 0 && (
          <button className={cn(control, 'hover:bg-muted')} onClick={addUnused} type="button">
            Add {Math.min(unused.length, MAX_UNUSED)} unused values
          </button>
        )}
      </div>
    </div>
  );
}
