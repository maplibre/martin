import type { SourceSpecification } from '@maplibre/maplibre-gl-style-spec';
import { ChevronDown, ChevronRight, Plus, Trash2 } from 'lucide-react';
import { useState } from 'react';
import { cn } from '@/lib/utils';
import { layersUsingSource, removeSource, renameSource, setSource } from '../operations';
import { useStyleResources } from '../style-resources';
import { fetchJson, useProbe } from './resources-probe';
import { SourceFields } from './sources-fields';
import {
  CUSTOM_SOURCE_TYPES,
  type CustomSourceType,
  customSource,
  sourceFields,
  sourceRecord,
  sourceSummary,
} from './sources-model';
import type { StyleSectionProps } from './types';
import {
  Badge,
  Chip,
  FieldRow,
  Hint,
  PrimaryButton,
  SecondaryButton,
  SettingsSection,
  StatusLine,
  TextInput,
  WarningBox,
} from './ui';

/** Every data source of the style: add, edit, rename and remove. */
export function SourcesSection(props: StyleSectionProps) {
  const { style, apply } = props;
  const entries = Object.entries(style.sources);
  const [expanded, setExpanded] = useState<string>();
  const add = (id: string, source: SourceSpecification) => {
    apply(`Add source ${id}`, (draft) => setSource(draft, id, source));
    setExpanded(id);
  };
  return (
    <SettingsSection title={`Sources (${entries.length})`}>
      {entries.length === 0 ? <Hint>This style has no sources.</Hint> : null}
      <ul className="space-y-2">
        {entries.map(([id, source]) => (
          <SourceRow
            {...props}
            id={id}
            isOpen={expanded === id}
            key={id}
            onRenamed={setExpanded}
            onToggle={() => setExpanded(expanded === id ? undefined : id)}
            source={source}
          />
        ))}
      </ul>
      <AddSource onAdd={add} style={style} />
    </SettingsSection>
  );
}

function SourceRow({
  id,
  source,
  isOpen,
  onToggle,
  onRenamed,
  style,
  apply,
}: StyleSectionProps & {
  id: string;
  source: SourceSpecification;
  isOpen: boolean;
  onToggle: () => void;
  onRenamed: (id: string) => void;
}) {
  const [showLayers, setShowLayers] = useState(false);
  const layers = layersUsingSource(style, id);
  const summary = sourceSummary(source);
  const Chevron = isOpen ? ChevronDown : ChevronRight;
  return (
    <li className="rounded-md border">
      <div className="flex items-start gap-1 px-2 py-1.5">
        <button
          aria-expanded={isOpen}
          aria-label={`${isOpen ? 'Collapse' : 'Expand'} source ${id}`}
          className="flex min-w-0 flex-1 items-start gap-1.5 rounded-md text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={onToggle}
          type="button"
        >
          <Chevron className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-1.5">
              <span className="min-w-0 truncate font-mono text-xs" title={id}>
                {id}
              </span>
              <Badge>{source.type}</Badge>
            </span>
            <span
              className={cn(
                'block truncate text-[11px] text-muted-foreground',
                summary.includes('/') && 'font-mono text-[11px]',
              )}
              title={summary}
            >
              {summary}
            </span>
          </span>
        </button>
        <button
          aria-expanded={showLayers}
          className="shrink-0 rounded-md px-1 text-[11px] text-muted-foreground hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
          onClick={() => setShowLayers(!showLayers)}
          type="button"
        >
          used by {layers.length} {layers.length === 1 ? 'layer' : 'layers'}
        </button>
      </div>
      {showLayers ? (
        <ul className="mx-2 mb-1.5 max-h-28 overflow-y-auto rounded-md bg-muted/50 px-2 py-1">
          {layers.length === 0 ? (
            <li className="text-[11px] text-muted-foreground">No layers</li>
          ) : null}
          {layers.map((layer) => (
            <li className="truncate font-mono text-[11px]" key={layer} title={layer}>
              {layer}
            </li>
          ))}
        </ul>
      ) : null}
      {isOpen ? (
        <SourceDetails apply={apply} id={id} onRenamed={onRenamed} source={source} style={style} />
      ) : null}
    </li>
  );
}

function TileJsonLine({ url }: { url: string }) {
  const probe = useProbe(`tilejson ${url}`, async (signal) => {
    const tilejson = (await fetchJson(url, signal)) as {
      name?: string;
      minzoom?: number;
      maxzoom?: number;
      vector_layers?: unknown[];
    };
    const zooms = `z${tilejson.minzoom ?? 0}-${tilejson.maxzoom ?? 22}`;
    const parts = [tilejson.name ?? 'Unnamed', zooms];
    if (tilejson.vector_layers) {
      parts.push(`${tilejson.vector_layers.length} vector layers`);
    }
    return parts.join(', ');
  });
  if (probe.status === 'idle') return null;
  if (probe.status === 'loading') return <StatusLine tone="pending">Checking</StatusLine>;
  return <StatusLine tone={probe.status}>{probe.text}</StatusLine>;
}

function SourceDetails({
  id,
  source,
  style,
  apply,
  onRenamed,
}: Pick<StyleSectionProps, 'style' | 'apply'> & {
  id: string;
  source: SourceSpecification;
  onRenamed: (id: string) => void;
}) {
  const [name, setName] = useState(id);
  const [error, setError] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const url = sourceRecord(source).url;
  const doomed = layersUsingSource(style, id);

  const rename = () => {
    const next = name.trim();
    if (next === id) return;
    if (next === '' || next in style.sources) {
      setError(true);
      return;
    }
    apply(`Rename source ${id} to ${next}`, (draft) => {
      renameSource(draft, id, next);
    });
    onRenamed(next);
  };

  return (
    <div className="space-y-2 border-t px-2 py-2">
      <FieldRow
        action={
          <SecondaryButton disabled={name.trim() === id} onClick={rename}>
            Rename
          </SecondaryButton>
        }
        label="id"
      >
        <TextInput
          aria-invalid={error}
          aria-label="id"
          className={error ? 'border-destructive' : undefined}
          id={`source-id-${id}`}
          mono
          onChange={(e) => {
            setName(e.target.value);
            setError(false);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') rename();
          }}
          spellCheck={false}
          value={name}
        />
      </FieldRow>
      {error ? <StatusLine tone="error">Id is empty or already used</StatusLine> : null}
      <SourceFields
        fields={sourceFields(source.type)}
        onSet={(field, next, coalesce) =>
          apply(
            `Set ${field} of source ${id}`,
            (draft) => setSource(draft, id, withField(draft.sources[id], field, next)),
            coalesce ? `source-${id}-${field}-${coalesce}` : undefined,
          )
        }
        source={source}
      />
      {typeof url === 'string' ? <TileJsonLine url={url} /> : null}
      {confirming ? (
        <div className="space-y-2 rounded-md border p-2">
          <p className="text-xs text-foreground">Remove this source?</p>
          <Chip>{id}</Chip>
          {doomed.length > 0 ? (
            <WarningBox>
              <p>
                {doomed.length} {doomed.length === 1 ? 'layer is' : 'layers are'} removed with it:
              </p>
              <ul className="max-h-24 space-y-0.5 overflow-y-auto">
                {doomed.map((layer) => (
                  <li
                    className="truncate font-mono text-[11px] text-foreground"
                    key={layer}
                    title={layer}
                  >
                    {layer}
                  </li>
                ))}
              </ul>
            </WarningBox>
          ) : null}
          <div className="flex gap-2">
            <SecondaryButton
              className="border-destructive text-destructive hover:bg-destructive/10"
              onClick={() => apply(`Remove source ${id}`, (draft) => removeSource(draft, id))}
            >
              Remove source
            </SecondaryButton>
            <SecondaryButton onClick={() => setConfirming(false)}>Cancel</SecondaryButton>
          </div>
        </div>
      ) : (
        <div>
          <SecondaryButton className="text-destructive" onClick={() => setConfirming(true)}>
            <Trash2 className="size-3" /> Remove
          </SecondaryButton>
        </div>
      )}
    </div>
  );
}

function withField(
  source: SourceSpecification,
  field: string,
  value: unknown,
): SourceSpecification {
  const next = { ...sourceRecord(source) };
  if (value === undefined) delete next[field];
  else next[field] = value;
  return next as unknown as SourceSpecification;
}

function AddSource({
  style,
  onAdd,
}: {
  style: StyleSectionProps['style'];
  onAdd: (id: string, source: SourceSpecification) => void;
}) {
  const { martinSources } = useStyleResources();
  const [open, setOpen] = useState(false);
  const [type, setType] = useState<CustomSourceType>('vector');
  const [url, setUrl] = useState('');
  const [id, setId] = useState('');
  if (!open) {
    return (
      <div>
        <SecondaryButton onClick={() => setOpen(true)}>
          <Plus className="size-3" /> Add source
        </SecondaryButton>
      </div>
    );
  }
  const used = new Set(Object.values(style.sources).map((s) => sourceRecord(s).url));
  const available = martinSources.filter(
    (m) => !(m.id in style.sources) && !used.has(sourceRecord(m.spec).url),
  );
  const taken = id.trim() in style.sources;
  const valid = id.trim() !== '' && !taken && url.trim() !== '';
  const close = () => {
    setOpen(false);
    setUrl('');
    setId('');
  };
  return (
    <div className="space-y-3 rounded-md border p-2">
      {available.length > 0 ? (
        <div className="space-y-2">
          <p className="text-xs font-medium text-foreground">From this Martin</p>
          {available.map((m) => (
            <div className="flex items-center gap-2" key={m.id}>
              <span className="min-w-0 flex-1 truncate font-mono text-xs" title={m.id}>
                {m.id}
              </span>
              <Badge>{m.spec.type}</Badge>
              <SecondaryButton
                aria-label={`Add ${m.id} from Martin`}
                onClick={() => {
                  onAdd(m.id, m.spec);
                  close();
                }}
              >
                Add
              </SecondaryButton>
            </div>
          ))}
        </div>
      ) : null}
      <div className="space-y-2">
        <p className="text-xs font-medium text-foreground">Custom</p>
        <FieldRow label="Type">
          <select
            aria-label="Source type"
            className="h-8 w-full min-w-0 rounded-md border bg-background px-2 text-xs focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
            onChange={(e) => setType(e.target.value as CustomSourceType)}
            value={type}
          >
            {CUSTOM_SOURCE_TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </FieldRow>
        <FieldRow label="Id">
          <TextInput
            aria-label="Source id"
            className={taken ? 'border-destructive' : undefined}
            mono
            onChange={(e) => setId(e.target.value)}
            placeholder="my-source"
            spellCheck={false}
            value={id}
          />
        </FieldRow>
        {taken ? <StatusLine tone="error">Id is already used</StatusLine> : null}
        <FieldRow label="URL">
          <TextInput
            aria-label="Source URL"
            mono
            onChange={(e) => setUrl(e.target.value)}
            placeholder={
              type === 'geojson'
                ? 'https://example.com/data.geojson'
                : 'https://example.com/tiles.json'
            }
            spellCheck={false}
            value={url}
          />
        </FieldRow>
        <div className="flex gap-2">
          <PrimaryButton
            disabled={!valid}
            onClick={() => {
              onAdd(id.trim(), customSource(type, url.trim()));
              close();
            }}
          >
            Add
          </PrimaryButton>
          <SecondaryButton onClick={close}>Cancel</SecondaryButton>
        </div>
      </div>
    </div>
  );
}
