import type { LayerSpecification } from '@maplibre/maplibre-gl-style-spec';
import { useStyleResources } from '../style-resources';
import { layerTypesFor, usesSourceLayer } from '../style-spec';
import { Dropdown, type DropdownOption } from './dropdown';

/** Props of the source and source-layer fields. */
export interface SourceFieldsProps {
  /** Selected layer. */
  layer: LayerSpecification;
  /** Points the layer at another source and source layer. */
  onSource: (source: string, sourceLayer: string | undefined) => void;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[80px_minmax(0,1fr)] items-center gap-2">
      <span className="text-[11px] text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

/** Source and source-layer dropdowns limited to what the layer type can draw; background layers have none. */
export function SourceFields({ layer, onSource }: SourceFieldsProps) {
  const { sources, sourceLayers } = useStyleResources();
  const source = 'source' in layer ? layer.source : undefined;
  const sourceLayer = 'source-layer' in layer ? layer['source-layer'] : undefined;
  if (layer.type === 'background') {
    return <p className="text-[11px] text-muted-foreground">No source</p>;
  }

  const sourceType = source === undefined ? undefined : sources[source]?.type;
  const options: DropdownOption[] = Object.entries(sources)
    .filter(([, s]) => layerTypesFor(s.type).includes(layer.type))
    .map(([value, s]) => ({ badge: s.type, value }));
  if (source !== undefined && !options.some((o) => o.value === source)) {
    options.unshift({ badge: sourceType, value: source });
  }

  const listed = source === undefined ? [] : sourceLayers(source);
  const layerOptions = listed.map((value) => ({ value }));
  if (sourceLayer !== undefined && !listed.includes(sourceLayer)) {
    layerOptions.unshift({ value: sourceLayer });
  }

  const pickSource = (next: string) => {
    if (next === source) return;
    if (!usesSourceLayer(sources[next]?.type)) {
      onSource(next, undefined);
      return;
    }
    const available = sourceLayers(next);
    const keep = sourceLayer !== undefined && available.includes(sourceLayer);
    onSource(next, keep ? sourceLayer : (available[0] ?? sourceLayer));
  };

  const unloaded =
    usesSourceLayer(sourceType) && sourceLayer !== undefined && !listed.includes(sourceLayer);

  return (
    <div className="space-y-1.5">
      <Field label="Source">
        <Dropdown
          label="Source"
          onChange={pickSource}
          options={options}
          placeholder="Choose a source"
          value={source}
        />
      </Field>
      {usesSourceLayer(sourceType) ? (
        <Field label="Source layer">
          <Dropdown
            allowCustom
            label="Source layer"
            onChange={(next) => source !== undefined && onSource(source, next)}
            options={layerOptions}
            placeholder="Choose a source layer"
            value={sourceLayer}
          />
        </Field>
      ) : null}
      {unloaded ? (
        <p
          className="truncate text-[11px] text-muted-foreground"
          title={`${sourceLayer} has no features loaded yet`}
        >
          <span className="font-mono">{sourceLayer}</span> has no features loaded yet
        </p>
      ) : null}
    </div>
  );
}
