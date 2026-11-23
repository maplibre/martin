import type { LayerSpecification } from '@maplibre/maplibre-gl-style-spec';
import { Pencil } from 'lucide-react';
import { useId, useState } from 'react';
import { Switch } from '@/components/ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { AttributeInfo } from '../source-schema';
import {
  groupValues,
  type LayerType,
  layerProperties,
  type PropertyEntry,
  type PropertyGroup,
  type PropertyRef,
} from '../style-spec';
import { initialValue } from '../value-model';
import { AddProperty } from './add-property';
import { FilterSection } from './filter-section';
import { PropertyRow } from './property-row';
import { SourceFields } from './source-fields';
import { ZoomRange } from './zoom-range';

/** Props of the selected-layer inspector. */
export interface LayerInspectorProps {
  /** Selected layer. */
  layer: LayerSpecification;
  /** Attributes of the layer's source layer. */
  attributes: AttributeInfo[];
  /** Current map zoom. */
  zoom: number;
  /** Sets a property; `undefined` unsets it; `coalesce` merges rapid edits into one history entry. */
  onSet: (ref: PropertyRef, value: unknown, coalesce?: string) => void;
  /** Sets the layer's zoom range. */
  onZoomRange: (min: number | undefined, max: number | undefined) => void;
  /** Renames the layer; false when the id is taken or empty. */
  onRename: (next: string) => boolean;
  /** Opens a color in the palette tab. */
  onSelectColor: (key: string) => void;
  /** Sets the layer filter; `undefined` removes it. */
  onFilter: (filter: unknown) => void;
  /** Points the layer at another source and source layer. */
  onSource: (source: string, sourceLayer: string | undefined) => void;
}

const SECTION_HEADING = 'text-[11px] font-semibold uppercase tracking-wider text-muted-foreground';

const GROUP_HINT: Record<string, string> = {
  Layout: 'Placement and visibility, applied when tiles load',
  Paint: 'How features are drawn',
};

function Heading({ title, hint, className }: { title: string; hint?: string; className?: string }) {
  const heading = <h3 className={cn(SECTION_HEADING, className)}>{title}</h3>;
  if (!hint) return heading;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{heading}</TooltipTrigger>
      <TooltipContent className="text-xs" side="top">
        {hint}
      </TooltipContent>
    </Tooltip>
  );
}

function labelOf(type: LayerType, name: string): string {
  const prefixes = type === 'symbol' ? ['text', 'icon', 'symbol'] : [type];
  const prefix = prefixes.find((p) => name.startsWith(`${p}-`));
  return (prefix ? name.slice(prefix.length + 1) : name).replaceAll('-', ' ');
}

function subsectionOf(type: LayerType, name: string): string | undefined {
  if (type !== 'symbol') return undefined;
  return name.startsWith('text-') ? 'Text' : name.startsWith('icon-') ? 'Icon' : 'General';
}

function focusOnMount(el: HTMLInputElement | null) {
  el?.focus();
}

function Title({ id, onRename }: { id: string; onRename: (next: string) => boolean }) {
  const [draft, setDraft] = useState<string | undefined>();
  const [error, setError] = useState(false);
  const commit = () => {
    if (draft === undefined) return;
    if (draft === id || onRename(draft)) {
      setDraft(undefined);
      setError(false);
    } else {
      setError(true);
    }
  };
  if (draft === undefined) {
    return (
      <button
        className="group flex min-w-0 max-w-full items-center gap-1.5 rounded-md text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
        onClick={() => setDraft(id)}
        title={`${id} (click to rename)`}
        type="button"
      >
        <span className="min-w-0 truncate font-mono text-sm font-semibold">{id}</span>
        <Pencil className="size-3 shrink-0 text-muted-foreground opacity-60 group-hover:opacity-100" />
      </button>
    );
  }
  return (
    <div>
      <input
        aria-label="Layer id"
        className={cn(
          'h-7 w-full rounded-md border bg-background px-2 font-mono text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
          error && 'border-destructive',
        )}
        onBlur={() => {
          setDraft(undefined);
          setError(false);
        }}
        onChange={(e) => {
          setDraft(e.target.value);
          setError(false);
        }}
        onFocus={(e) => e.currentTarget.select()}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
          if (e.key === 'Escape') {
            setDraft(undefined);
            setError(false);
          }
        }}
        ref={focusOnMount}
        value={draft}
      />
      {error ? (
        <p className="mt-0.5 text-[11px] text-destructive">Id is empty or already used</p>
      ) : null}
    </div>
  );
}

function Group({
  title,
  entries,
  showAll,
  props,
}: {
  title: string;
  entries: PropertyEntry[];
  showAll: boolean;
  props: LayerInspectorProps;
}) {
  const { layer } = props;
  const visible = entries.filter(
    (e) => showAll || groupValues(layer, e.group)[e.name] !== undefined,
  );
  if (visible.length === 0) return null;
  const present = new Set(visible.map((e) => subsectionOf(layer.type, e.name)));
  const subsections =
    layer.type === 'symbol'
      ? ['Text', 'Icon', 'General'].filter((x) => present.has(x))
      : [undefined];
  return (
    <section className="py-2">
      <Heading className="px-3 pb-1" hint={GROUP_HINT[title]} title={title} />
      {subsections.map((sub) => (
        <div key={sub ?? 'all'}>
          {sub ? (
            <h4 className="px-3 pt-2 pb-0.5 text-[11px] font-medium text-muted-foreground">
              {sub}
            </h4>
          ) : null}
          {visible
            .filter((e) => subsectionOf(layer.type, e.name) === sub)
            .map((e) => (
              <PropertyRow
                attributes={props.attributes}
                entry={e}
                key={`${e.group}-${e.name}`}
                label={labelOf(layer.type, e.name)}
                layer={layer}
                onSelectColor={props.onSelectColor}
                onSet={props.onSet}
                raw={groupValues(layer, e.group)[e.name]}
                zoom={props.zoom}
              />
            ))}
        </div>
      ))}
    </section>
  );
}

/** Layer header plus every paint and layout property from the v8 spec. */
export function LayerInspector(props: LayerInspectorProps) {
  const { layer, onRename, onZoomRange, zoom } = props;
  const [showAll, setShowAll] = useState(false);
  const switchId = useId();
  const source = 'source' in layer ? layer.source : undefined;
  const sourceLayer = 'source-layer' in layer ? layer['source-layer'] : undefined;
  const groups: PropertyGroup[] = ['paint', 'layout'];
  const all = groups.flatMap((g) => layerProperties(layer.type, g));
  const unset = all.filter((e) => groupValues(layer, e.group)[e.name] === undefined);

  return (
    <div className="pb-4">
      <header className="space-y-0.5 border-b px-3 py-3">
        <Title id={layer.id} key={layer.id} onRename={onRename} />
        <p className="text-[11px] text-muted-foreground">{layer.type}</p>
        <div className="pt-1.5">
          <SourceFields layer={layer} onSource={props.onSource} />
        </div>
      </header>
      <section className="space-y-2 border-b px-3 py-3">
        <Heading hint="Zooms at which the layer is drawn" title="Zoom range" />
        <ZoomRange max={layer.maxzoom} min={layer.minzoom} onZoomRange={onZoomRange} zoom={zoom} />
      </section>
      {(source !== undefined || sourceLayer !== undefined) && (
        <div className="border-b">
          <FilterSection
            attributes={props.attributes}
            filter={'filter' in layer ? layer.filter : undefined}
            key={layer.id}
            onFilter={props.onFilter}
          />
        </div>
      )}
      <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
        <label className="text-xs text-muted-foreground" htmlFor={switchId}>
          Show all properties
        </label>
        <Switch checked={showAll} id={switchId} onCheckedChange={setShowAll} />
      </div>
      <Group
        entries={layerProperties(layer.type, 'paint')}
        props={props}
        showAll={showAll}
        title="Paint"
      />
      <Group
        entries={layerProperties(layer.type, 'layout')}
        props={props}
        showAll={showAll}
        title="Layout"
      />
      {unset.length > 0 && !showAll && (
        <div className="px-3 pt-2">
          <AddProperty
            entries={unset}
            layerType={layer.type}
            onAdd={(e) =>
              props.onSet({ group: e.group, layer: layer.id, name: e.name }, initialValue(e.spec))
            }
          />
        </div>
      )}
    </div>
  );
}
