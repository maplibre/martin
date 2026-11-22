import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { Check, ChevronRight, CircleHelp, X } from 'lucide-react';
import { useState } from 'react';
import { cn } from '@/lib/utils';
import {
  type ClauseResult,
  filterClauses,
  type InspectedFeature,
  type LayerMatch,
  type LayerVerdict,
  layerMatches,
  type PropertyValue,
} from './inspect';

/** Props of {@link InspectPanel}. */
export interface InspectPanelProps {
  /** Features under the last click, topmost first. */
  features: readonly InspectedFeature[];
  /** Style used to decide which layers draw each feature. */
  style: StyleSpecification;
  /** Map zoom the verdicts are computed at. */
  zoom: number;
  /** Selected style layer; its filter is broken down clause by clause. */
  selectedLayer: string | null;
  /** Key of the feature outlined on the map. */
  active: string | null;
  /** Outlines a feature on the map. */
  onActive: (key: string) => void;
  /** Selects a style layer in the inspector. */
  onSelectLayer: (id: string) => void;
  /** Closes the panel. */
  onClose: () => void;
}

/** Floating card listing the source features under a click with their raw properties and the layers that match them. */
export function InspectPanel({
  features,
  style,
  zoom,
  selectedLayer,
  active,
  onActive,
  onSelectLayer,
  onClose,
}: InspectPanelProps) {
  const [layersOpen, setLayersOpen] = useState(false);
  return (
    <section
      aria-label="Inspected features"
      className="pointer-events-auto flex max-h-full w-[340px] flex-col overflow-hidden rounded-lg border bg-card/95 text-xs shadow-lg backdrop-blur"
    >
      <header className="flex items-center gap-2 border-b px-2.5 py-1.5">
        <h3 className="flex-1 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
          {features.length === 0
            ? 'Nothing here'
            : `${features.length} feature${features.length === 1 ? '' : 's'} · tile z${Math.floor(zoom)}`}
        </h3>
        <button
          aria-label="Close inspector"
          className="flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onClose}
          type="button"
        >
          <X className="size-3.5" />
        </button>
      </header>
      <div className="min-h-0 overflow-y-auto">
        {features.length === 0 ? (
          <p className="p-3 text-muted-foreground">
            No source feature under the cursor. Click on a line, area or point.
          </p>
        ) : null}
        {features.map((f) => (
          <FeatureCard
            expanded={f.key === active}
            feature={f}
            key={f.key}
            layersOpen={layersOpen}
            matches={layerMatches(style, f, zoom)}
            onExpand={() => onActive(f.key)}
            onLayersOpen={setLayersOpen}
            onSelectLayer={onSelectLayer}
            selectedLayer={selectedLayer}
            style={style}
            zoom={zoom}
          />
        ))}
      </div>
    </section>
  );
}

function FeatureCard({
  feature,
  matches,
  expanded,
  onExpand,
  style,
  zoom,
  selectedLayer,
  onSelectLayer,
  layersOpen,
  onLayersOpen,
}: {
  feature: InspectedFeature;
  matches: LayerMatch[];
  expanded: boolean;
  onExpand: () => void;
  style: StyleSpecification;
  zoom: number;
  selectedLayer: string | null;
  onSelectLayer: (id: string) => void;
  layersOpen: boolean;
  onLayersOpen: (open: boolean) => void;
}) {
  const drawn = matches.filter((m) => m.verdict.status === 'drawn');
  const initial = matches.some((m) => m.id === selectedLayer)
    ? selectedLayer
    : (drawn[0]?.id ?? null);
  const [openLayer, setOpenLayer] = useState<string | null>(initial);
  const [showAll, setShowAll] = useState(false);
  const shown = showAll
    ? matches
    : matches.filter(
        (m) => m.verdict.status === 'drawn' || m.id === selectedLayer || m.id === openLayer,
      );
  const more = matches.length - shown.length;
  const layer = style.layers.find((l) => l.id === openLayer);
  const clauses = layer ? filterClauses(layer, feature, zoom) : [];
  const watched = new Set(layersOpen ? clauses.flatMap((c) => c.keys) : []);
  const props = Object.entries(feature.properties).sort(([a], [b]) => a.localeCompare(b));

  return (
    <article className="border-b last:border-b-0">
      <button
        aria-expanded={expanded}
        className={cn(
          'flex w-full items-center gap-2 px-2.5 py-2 text-left hover:bg-muted',
          expanded && 'bg-muted/60',
        )}
        onClick={onExpand}
        type="button"
      >
        <ChevronRight
          aria-hidden="true"
          className={cn(
            'size-3.5 shrink-0 text-muted-foreground transition-transform',
            expanded && 'rotate-90',
          )}
        />
        <span className="min-w-0 flex-1 truncate font-mono font-medium">
          {feature.sourceLayer ?? feature.source}
          {feature.id === undefined ? null : (
            <span className="text-muted-foreground"> #{feature.id}</span>
          )}
        </span>
        <span className="shrink-0 text-[10px] text-muted-foreground">{feature.kind}</span>
        <span
          className={cn(
            'shrink-0 rounded px-1 text-[10px] tabular-nums',
            drawn.length ? 'bg-muted text-foreground' : 'text-muted-foreground',
          )}
          title="Style layers that draw this feature"
        >
          {drawn.length ? `${drawn.length} drawn` : 'not drawn'}
        </span>
      </button>
      {expanded ? (
        <div className="space-y-3 px-2.5 pt-1 pb-3">
          <section>
            <h4 className="mb-1 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
              Properties <span className="font-normal normal-case">({props.length})</span>
            </h4>
            <table className="w-full table-fixed font-mono text-[11px]">
              <tbody>
                {props.map(([k, v]) => (
                  <tr
                    className={cn('align-top', watched.has(k) && 'bg-editor-accent-soft')}
                    key={k}
                  >
                    <th
                      className={cn(
                        'w-[42%] truncate py-0.5 pr-2 pl-1 text-left font-normal text-muted-foreground',
                        watched.has(k) && 'text-editor-accent',
                      )}
                      scope="row"
                      title={k}
                    >
                      {k}
                    </th>
                    <td className="py-0.5 pr-1 break-words">
                      <Value value={v} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
          {matches.length ? (
            <section>
              <button
                aria-expanded={layersOpen}
                className="flex w-full items-center gap-1 text-left text-[10px] font-semibold tracking-wide text-muted-foreground uppercase hover:text-foreground"
                onClick={() => onLayersOpen(!layersOpen)}
                type="button"
              >
                <ChevronRight
                  aria-hidden="true"
                  className={cn('size-3 transition-transform', layersOpen && 'rotate-90')}
                />
                Style layers
                <span className="font-normal normal-case">
                  ({drawn.length} of {matches.length} draw it)
                </span>
              </button>
              {layersOpen ? (
                <>
                  <ul className="mt-1 space-y-px">
                    {shown.map((m) => (
                      <MatchRow
                        clauses={m.id === openLayer ? clauses : undefined}
                        feature={feature}
                        key={m.id}
                        match={m}
                        onOpen={() => setOpenLayer(m.id === openLayer ? null : m.id)}
                        onSelect={() => {
                          setOpenLayer(m.id);
                          onSelectLayer(m.id);
                        }}
                        selected={m.id === selectedLayer}
                      />
                    ))}
                  </ul>
                  {more > 0 ? (
                    <button
                      className="mt-1 ml-1 text-[11px] text-muted-foreground hover:text-foreground"
                      onClick={() => setShowAll(true)}
                      type="button"
                    >
                      + {more} more that do not draw it
                    </button>
                  ) : null}
                </>
              ) : null}
            </section>
          ) : (
            <p className="text-[11px] text-muted-foreground">
              No style layer reads {feature.sourceLayer ?? feature.source}.
            </p>
          )}
        </div>
      ) : null}
    </article>
  );
}

function verdictText(v: LayerVerdict): string {
  switch (v.status) {
    case 'drawn':
      return 'drawn';
    case 'filter':
      return v.failing ? `fails: ${v.failing}` : 'filtered out';
    case 'geometry':
      return 'wrong geometry';
    case 'hidden':
      return 'hidden';
    case 'unknown':
      return 'unknown';
    case 'zoom':
      return `z${v.minzoom}-${v.maxzoom}`;
  }
}

function MatchRow({
  match,
  feature,
  selected,
  clauses,
  onOpen,
  onSelect,
}: {
  match: LayerMatch;
  feature: InspectedFeature;
  selected: boolean;
  clauses: ClauseResult[] | undefined;
  onOpen: () => void;
  onSelect: () => void;
}) {
  const drawn = match.verdict.status === 'drawn';
  return (
    <li>
      <div
        className={cn(
          'flex items-center gap-1.5 rounded px-1 py-0.5',
          selected && 'ring-1 ring-editor-accent',
        )}
      >
        <span
          aria-hidden="true"
          className={cn(
            'size-1.5 shrink-0 rounded-full',
            drawn ? 'bg-foreground' : 'border border-muted-foreground',
          )}
        />
        <button
          className="min-w-0 flex-1 truncate text-left font-mono text-[11px] hover:underline"
          onClick={onSelect}
          title={`Select ${match.id}`}
          type="button"
        >
          {match.id}
        </button>
        <button
          aria-expanded={clauses !== undefined}
          className={cn(
            'flex max-w-[60%] items-center gap-0.5 rounded px-1 text-[10px] hover:bg-muted',
            'min-w-0 text-muted-foreground',
          )}
          onClick={onOpen}
          title={match.verdict.status === 'unknown' ? match.verdict.reason : 'Show the filter'}
          type="button"
        >
          <span className="truncate">{verdictText(match.verdict)}</span>
          <ChevronRight className={cn('size-3 transition-transform', clauses && 'rotate-90')} />
        </button>
      </div>
      {clauses ? <ClauseList clauses={clauses} feature={feature} /> : null}
    </li>
  );
}

function ClauseList({ clauses, feature }: { clauses: ClauseResult[]; feature: InspectedFeature }) {
  if (clauses.length === 0) {
    return (
      <p className="mt-0.5 mb-1.5 ml-4 text-[11px] text-muted-foreground">
        No filter: draws every feature.
      </p>
    );
  }
  return (
    <ul className="mt-0.5 mb-1.5 ml-4 space-y-0.5 border-l pl-2">
      {clauses.map((c, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: clauses are positional and may repeat
        <li className="flex items-start gap-1.5" key={i}>
          {c.pass === null ? (
            <CircleHelp
              aria-label="Not evaluated"
              className="mt-0.5 size-3 shrink-0 text-muted-foreground"
            />
          ) : c.pass ? (
            <Check aria-label="Passes" className="mt-0.5 size-3 shrink-0 text-foreground" />
          ) : (
            <X aria-label="Fails" className="mt-0.5 size-3 shrink-0 text-editor-accent" />
          )}
          <div className="min-w-0">
            <code
              className={cn(
                'block font-mono text-[10.5px] break-words',
                c.pass === false && 'text-editor-accent',
              )}
              title={JSON.stringify(c.clause)}
            >
              {c.label}
            </code>
            {c.pass === false && c.keys.length ? (
              <span className="font-mono text-[10px] text-muted-foreground">
                {c.keys.map((k) => (
                  <span className="mr-2" key={k}>
                    {k} = <Value value={feature.properties[k]} />
                  </span>
                ))}
              </span>
            ) : null}
            {c.typeMismatch ? (
              <span className="block text-[10px] text-editor-accent">
                {c.typeMismatch.key} is a {c.typeMismatch.actual}, the filter compares with a{' '}
                {c.typeMismatch.expected}
              </span>
            ) : null}
          </div>
        </li>
      ))}
    </ul>
  );
}

function Value({ value }: { value: PropertyValue | undefined }) {
  if (value === undefined) return <span className="text-muted-foreground italic">missing</span>;
  if (value === null) return <span className="text-muted-foreground">null</span>;
  if (typeof value === 'string') return <span className="text-foreground">"{value}"</span>;
  if (typeof value === 'boolean') {
    return (
      <span
        className="rounded-sm bg-violet-500/15 px-1 text-violet-700 dark:text-violet-300"
        title="boolean"
      >
        {String(value)}
      </span>
    );
  }
  return (
    <span className="text-sky-700 dark:text-sky-300" title="number">
      {value}
    </span>
  );
}
