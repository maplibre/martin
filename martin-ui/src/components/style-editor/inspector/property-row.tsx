import type { LayerSpecification } from '@maplibre/maplibre-gl-style-spec';
import { RotateCcw } from 'lucide-react';
import type { ReactNode } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { colorSites } from '../colors';
import { usePropertyDocs } from '../property-docs';
import type { AttributeInfo } from '../source-schema';
import { type PropertyEntry, type PropertyRef, validateProperty } from '../style-spec';
import { CurveEditor } from '../value-editors/curve-editor';
import { ExpressionEditor } from '../value-editors/expression-editor';
import { LiteralEditor } from '../value-editors/literal-editor';
import { MatchEditor } from '../value-editors/match-editor';
import {
  allowedModes,
  convertValue,
  parseValue,
  serializeValue,
  type ValueMode,
} from '../value-model';
import { ModeMenu } from './mode-menu';

/** Props of one property row. */
export interface PropertyRowProps {
  /** Layer the property belongs to. */
  layer: LayerSpecification;
  /** The property and its spec. */
  entry: PropertyEntry;
  /** Raw value in the layer, `undefined` when unset. */
  raw: unknown;
  /** Label shown instead of the full name. */
  label: string;
  /** Attributes of the layer's source layer. */
  attributes: AttributeInfo[];
  /** Current map zoom. */
  zoom: number;
  /** Sets or unsets the property. */
  onSet: (ref: PropertyRef, value: unknown, coalesce?: string) => void;
  /** Opens a color in the palette tab. */
  onSelectColor: (key: string) => void;
}

function formatDefault(value: unknown): string {
  return value === undefined ? 'none' : typeof value === 'string' ? value : JSON.stringify(value);
}

/** One property: label, mode menu, reset and the editor for the current mode. */
export function PropertyRow({
  layer,
  entry,
  raw,
  label,
  attributes,
  zoom,
  onSet,
  onSelectColor,
}: PropertyRowProps) {
  const { name, group, spec } = entry;
  const docOf = usePropertyDocs();
  const ref: PropertyRef = { group, layer: layer.id, name };
  const isSet = raw !== undefined;
  const value = parseValue(isSet ? raw : spec.default, spec);
  const modes = allowedModes(spec);
  const errors = validateProperty(raw, name, spec);

  const switchMode = (mode: ValueMode) => {
    const preferred = ['class', 'type', 'kind', 'subclass', 'category'];
    const categorical = attributes.filter(
      (a) => a.kind === 'string' && a.values.length > 1 && a.values.length <= 50,
    );
    const strings =
      preferred.map((n) => categorical.find((a) => a.name === n)).find((a) => a !== undefined) ??
      categorical.sort((a, b) => b.values.length - a.values.length)[0] ??
      attributes.find((a) => a.kind === 'string' && a.values.length > 0);
    const numbers = attributes.find((a) => a.kind === 'number' && a.range);
    const attribute = mode === 'match' ? strings : mode === 'ramp' ? numbers : undefined;
    const next = convertValue(value, mode, {
      attribute: attribute?.name,
      sampleLabels:
        mode === 'match' ? attribute?.values.filter((v) => typeof v !== 'boolean') : undefined,
      sampleRange: mode === 'ramp' ? attribute?.range : undefined,
      spec,
    });
    onSet(ref, serializeValue(next));
  };

  const doc = docOf(layer.type, group, name);
  const tooltip = (
    <TooltipContent className="max-w-72 space-y-1 text-xs" side="left">
      <div className="font-mono">{name}</div>
      <div className="text-muted-foreground">
        {spec.type}
        {spec.units ? `, ${spec.units}` : ''} (default {formatDefault(spec.default)})
      </div>
      {doc ? <div>{doc}</div> : null}
    </TooltipContent>
  );
  const labelNode = (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="min-w-0 truncate text-xs" title={name}>
          {label}
        </span>
      </TooltipTrigger>
      {tooltip}
    </Tooltip>
  );
  const reset = isSet ? (
    <button
      aria-label={`Reset ${name}`}
      className="flex size-5 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
      onClick={() => onSet(ref, undefined)}
      title="Reset to default"
      type="button"
    >
      <RotateCcw className="size-3" />
    </button>
  ) : (
    <span className="size-5" />
  );
  const menu = <ModeMenu mode={value.mode} modes={modes} name={name} onChange={switchMode} />;

  const swatches =
    value.mode === 'expression' && spec.type === 'color'
      ? colorSites({ layers: [layer], sources: {}, version: 8 }).filter(
          (s) => s.name === name && s.group === group,
        )
      : [];

  let editor: ReactNode = null;
  switch (value.mode) {
    case 'zoom':
    case 'ramp':
      editor = (
        <CurveEditor
          attributes={attributes}
          onChange={(next, coalesce) => onSet(ref, serializeValue(next), coalesce)}
          spec={spec}
          value={value}
          zoom={zoom}
        />
      );
      break;
    case 'match':
      editor = (
        <MatchEditor
          attributes={attributes}
          onChange={(next, coalesce) => onSet(ref, serializeValue(next), coalesce)}
          spec={spec}
          value={value}
        />
      );
      break;
    case 'expression':
      editor = (
        <ExpressionEditor
          name={name}
          onChange={(next) => onSet(ref, next)}
          spec={spec}
          value={value.value}
        />
      );
      break;
    case 'constant':
      break;
  }

  return (
    <div className="px-3 py-1">
      {value.mode === 'constant' ? (
        <div className="grid grid-cols-[104px_minmax(0,1fr)_auto] items-center gap-2">
          {labelNode}
          <div className="min-w-0">
            <LiteralEditor
              isDefault={!isSet}
              onChange={(next, coalesce) => onSet(ref, next, coalesce)}
              spec={spec}
              value={value.value}
            />
          </div>
          <div className="flex items-center justify-end gap-1">
            {menu}
            {reset}
          </div>
        </div>
      ) : (
        <div>
          <div className="flex h-7 items-center gap-1">
            <div className="flex min-w-0 flex-1">{labelNode}</div>
            {menu}
            {reset}
          </div>
          <div className="min-w-0 rounded-md border bg-background/50 p-2">
            {editor}
            {swatches.length > 0 && (
              <div className="mt-2 flex flex-wrap items-center gap-1">
                <span className="text-[11px] text-muted-foreground">Colors</span>
                {swatches.map((s) => (
                  <button
                    aria-label={`Open ${s.key} in palette`}
                    className="size-4 rounded-sm border focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
                    key={`${s.path.join('.')}-${s.key}`}
                    onClick={() => onSelectColor(s.key)}
                    style={{ background: s.key }}
                    title={s.key}
                    type="button"
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      )}
      {errors.map((e) => (
        <p className="mt-0.5 break-words text-[11px] text-destructive" key={e}>
          {e}
        </p>
      ))}
    </div>
  );
}
