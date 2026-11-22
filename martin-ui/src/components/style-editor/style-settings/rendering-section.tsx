import type {
  LightSpecification,
  SkySpecification,
  StyleSpecification,
  TransitionSpecification,
} from '@maplibre/maplibre-gl-style-spec';
import { latest } from '@maplibre/maplibre-gl-style-spec';
import { cn } from '@/lib/utils';
import { setRoot } from '../operations';
import type { PropertySpec } from '../style-spec';
import { RenderingGroup } from './rendering-group';
import { isLiteral, ResetButton, rootSpec, SpecRow } from './spec-row';
import type { StyleSectionProps } from './types';
import { FieldRow, Hint, NumberInput, SettingsSection } from './ui';

const SKY_HINTS: Record<string, string> = {
  'atmosphere-blend': 'How strongly the atmosphere glows around the globe, 0 to 1.',
  'fog-color': 'Color of the fog that fades distant ground.',
  'fog-ground-blend': 'How far fog reaches down onto the ground, 0 to 1.',
  'horizon-color': 'Color of the band at the horizon.',
  'horizon-fog-blend': 'How much fog and horizon color mix, 0 to 1.',
  'sky-color': 'Color of the sky above the horizon.',
  'sky-horizon-blend': 'How smoothly the sky fades into the horizon, 0 to 1.',
};

const LIGHT_HINTS: Record<string, string> = {
  anchor: 'Whether 3D extrusions are lit relative to the map or to the screen.',
  color: 'Color of the light.',
  intensity: 'Strength of the light, 0 to 1.',
};

const POSITION: { label: string; hint: string; spec: PropertySpec }[] = [
  {
    hint: 'Distance of the light from the center.',
    label: 'Radial',
    spec: { default: 1.15, minimum: 0, type: 'number' },
  },
  {
    hint: 'Direction around the map in degrees, 0 is north.',
    label: 'Azimuthal',
    spec: { default: 210, maximum: 360, minimum: 0, type: 'number' },
  },
  {
    hint: 'Height of the light in degrees, 0 is straight above.',
    label: 'Polar',
    spec: { default: 30, maximum: 180, minimum: 0, type: 'number' },
  },
];

const TRANSITION_HINTS: Record<string, string> = {
  delay: 'Wait before a style change starts animating.',
  duration: 'How long a style change takes to animate.',
};

const PROJECTIONS = ['mercator', 'globe'] as const;

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function countState(value: object | undefined): string {
  if (value === undefined) return 'Not set';
  const n = Object.keys(value).length;
  return n === 0 ? 'Defaults' : `${n} ${n === 1 ? 'value' : 'values'}`;
}

function PositionField({
  value,
  onChange,
  onReset,
}: {
  value: unknown;
  onChange: (next: number[]) => void;
  onReset: () => void;
}) {
  const spec = rootSpec('light', 'position');
  const isSet = value !== undefined;
  const computed = isSet && !isLiteral(spec, value);
  const current = isLiteral(spec, value) ? (value as number[]) : (spec.default as number[]);
  return (
    <div className="space-y-1">
      <FieldRow
        action={isSet ? <ResetButton name="light position" onReset={onReset} /> : undefined}
        alignTop
        hint="Where the light sits: distance, direction and height."
        label="Position"
      >
        {computed ? (
          <code className="block truncate rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-[11px]">
            {JSON.stringify(value)}
          </code>
        ) : (
          <div className="grid min-w-0 grid-cols-3 gap-1">
            {POSITION.map((p, i) => (
              <div className="flex min-w-0 flex-col gap-0.5" key={p.label}>
                <span className="truncate text-[11px] text-muted-foreground" title={p.hint}>
                  {p.label}
                </span>
                <NumberInput
                  label={p.label}
                  max={p.spec.maximum}
                  min={p.spec.minimum}
                  onChange={(n) => {
                    if (n !== undefined) onChange(current.map((v, j) => (j === i ? n : v)));
                  }}
                  step={i === 0 ? 0.05 : 1}
                  value={current[i]}
                />
              </div>
            ))}
          </div>
        )}
      </FieldRow>
      {computed ? (
        <Hint>Expression, read-only here. Reset to replace it with a plain value.</Hint>
      ) : null}
    </div>
  );
}

function Projection({ style, apply }: StyleSectionProps) {
  const type = style.projection?.type;
  const isSet = style.projection !== undefined;
  const readOnly = type !== undefined && typeof type !== 'string';
  const write = (next: string | undefined) =>
    apply('Set projection', (d) =>
      setRoot(d, 'projection', next === undefined ? undefined : { ...d.projection, type: next }),
    );
  return (
    <RenderingGroup
      hint="How the world is flattened: Mercator is a flat map, Globe a sphere that flattens when you zoom in."
      isSet={isSet}
      onAdd={() => write('mercator')}
      onRemove={() => apply('Remove projection', (d) => setRoot(d, 'projection', undefined))}
      state={!isSet ? 'Not set' : typeof type === 'string' ? capitalize(type) : 'Expression'}
      title="Projection"
    >
      <FieldRow hint="The projection type." label="Type">
        {readOnly ? (
          <code className="block truncate rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-[11px]">
            {JSON.stringify(type)}
          </code>
        ) : (
          <fieldset
            aria-label="Projection type"
            className="m-0 flex h-8 min-w-0 rounded-md border p-0.5"
          >
            {PROJECTIONS.map((p) => {
              const on = (type ?? 'mercator') === p;
              return (
                <button
                  aria-pressed={on}
                  className={cn(
                    'flex-1 rounded-sm text-xs capitalize focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent',
                    on ? 'bg-editor-accent text-white' : 'text-muted-foreground hover:bg-muted',
                    !isSet && on && 'opacity-60',
                  )}
                  key={p}
                  onClick={() => write(p)}
                  type="button"
                >
                  {p}
                </button>
              );
            })}
          </fieldset>
        )}
      </FieldRow>
      {readOnly ? (
        <Hint>
          This projection is an expression, read-only here. Remove the group to start over.
        </Hint>
      ) : null}
    </RenderingGroup>
  );
}

function Light({ style, apply }: StyleSectionProps) {
  const light = style.light;
  const set = (name: keyof LightSpecification, value: unknown, coalesce?: string) =>
    apply(
      `Set light ${name}`,
      (d) => {
        const next: Record<string, unknown> = { ...d.light };
        if (value === undefined) delete next[name];
        else next[name] = value;
        setRoot(d, 'light', next as LightSpecification);
      },
      coalesce,
    );
  return (
    <RenderingGroup
      hint="Light that shades 3D buildings and terrain."
      isSet={light !== undefined}
      onAdd={() => apply('Add light', (d) => setRoot(d, 'light', {}))}
      onRemove={() => apply('Remove light', (d) => setRoot(d, 'light', undefined))}
      state={countState(light)}
      title="Light"
    >
      {(['anchor'] as const).map((name) => (
        <SpecRow
          hint={LIGHT_HINTS[name]}
          key={name}
          label={name}
          onChange={(next) => set(name, next)}
          onReset={() => set(name, undefined)}
          resetName={`light ${name}`}
          spec={rootSpec('light', name)}
          value={light?.[name]}
        />
      ))}
      <PositionField
        onChange={(next) => set('position', next, 'light-position')}
        onReset={() => set('position', undefined)}
        value={light?.position}
      />
      {(['color', 'intensity'] as const).map((name) => (
        <SpecRow
          hint={LIGHT_HINTS[name]}
          key={name}
          label={name}
          onChange={(next, coalesce) => set(name, next, coalesce)}
          onReset={() => set(name, undefined)}
          resetName={`light ${name}`}
          spec={rootSpec('light', name)}
          value={light?.[name]}
        />
      ))}
    </RenderingGroup>
  );
}

function Sky({ style, apply }: StyleSectionProps) {
  const sky = style.sky;
  const set = (name: string, value: unknown, coalesce?: string) =>
    apply(
      `Set sky ${name}`,
      (d) => {
        const next: Record<string, unknown> = { ...d.sky };
        if (value === undefined) delete next[name];
        else next[name] = value;
        setRoot(d, 'sky', next as SkySpecification);
      },
      coalesce,
    );
  return (
    <RenderingGroup
      hint="Sky and fog beyond the horizon; visible when the map is pitched or in globe view."
      isSet={sky !== undefined}
      onAdd={() => apply('Add sky', (d) => setRoot(d, 'sky', {}))}
      onRemove={() => apply('Remove sky', (d) => setRoot(d, 'sky', undefined))}
      state={countState(sky)}
      title="Sky"
    >
      {Object.keys(latest.sky).map((name) => (
        <SpecRow
          hint={SKY_HINTS[name] ?? name}
          key={name}
          label={name}
          onChange={(next, coalesce) => set(name, next, coalesce)}
          onReset={() => set(name, undefined)}
          resetName={`sky ${name}`}
          spec={rootSpec('sky', name)}
          value={(sky as Record<string, unknown> | undefined)?.[name]}
        />
      ))}
    </RenderingGroup>
  );
}

function demSources(style: StyleSpecification): string[] {
  return Object.entries(style.sources)
    .filter(([, s]) => s.type === 'raster-dem')
    .map(([id]) => id);
}

function Terrain({ style, apply }: StyleSectionProps) {
  const terrain = style.terrain;
  const dems = demSources(style);
  const options = terrain && !dems.includes(terrain.source) ? [terrain.source, ...dems] : dems;
  return (
    <RenderingGroup
      addBlocked={dems.length === 0 ? 'Add a raster-dem source in Sources first' : undefined}
      hint="Lifts the map into 3D using elevation tiles from a raster-dem source."
      isSet={terrain !== undefined}
      onAdd={
        dems.length > 0
          ? () => apply('Add terrain', (d) => setRoot(d, 'terrain', { source: dems[0] }))
          : undefined
      }
      onRemove={() => apply('Remove terrain', (d) => setRoot(d, 'terrain', undefined))}
      state={terrain ? terrain.source : 'Not set'}
      title="Terrain"
    >
      {terrain ? (
        <>
          <FieldRow hint="The raster-dem source that provides elevation." label="Source">
            <select
              aria-label="Terrain source"
              className="h-8 w-full min-w-0 rounded-md border bg-background px-2 font-mono text-[11px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-editor-accent"
              onChange={(e) =>
                apply('Set terrain source', (d) =>
                  setRoot(d, 'terrain', { ...d.terrain, source: e.target.value }),
                )
              }
              value={terrain.source}
            >
              {options.map((id) => (
                <option key={id} value={id}>
                  {dems.includes(id) ? id : `${id} (not a raster-dem source)`}
                </option>
              ))}
            </select>
          </FieldRow>
          <SpecRow
            hint="Multiplies elevations; 1 is true height, 0 is flat."
            label="exaggeration"
            onChange={(next, coalesce) =>
              apply(
                'Set terrain exaggeration',
                (d) =>
                  setRoot(d, 'terrain', {
                    ...d.terrain,
                    exaggeration: next as number,
                    source: terrain.source,
                  }),
                coalesce,
              )
            }
            onReset={() =>
              apply('Reset terrain exaggeration', (d) =>
                setRoot(d, 'terrain', { source: terrain.source }),
              )
            }
            resetName="terrain exaggeration"
            spec={rootSpec('terrain', 'exaggeration')}
            value={terrain.exaggeration}
          />
        </>
      ) : null}
    </RenderingGroup>
  );
}

function Transition({ style, apply }: StyleSectionProps) {
  const transition = style.transition;
  const set = (name: keyof TransitionSpecification, value: number | undefined, coalesce?: string) =>
    apply(
      `Set transition ${name}`,
      (d) => {
        const next = { ...d.transition };
        if (value === undefined) delete next[name];
        else next[name] = value;
        setRoot(d, 'transition', next);
      },
      coalesce,
    );
  return (
    <RenderingGroup
      hint="How property changes animate when the style is updated."
      isSet={transition !== undefined}
      onAdd={() => apply('Add transition', (d) => setRoot(d, 'transition', {}))}
      onRemove={() => apply('Remove transition', (d) => setRoot(d, 'transition', undefined))}
      state={countState(transition)}
      title="Transition"
    >
      {(['duration', 'delay'] as const).map((name) => (
        <SpecRow
          hint={TRANSITION_HINTS[name]}
          key={name}
          label={name}
          onChange={(next, coalesce) => set(name, next as number, coalesce)}
          onReset={() => set(name, undefined)}
          resetName={`transition ${name}`}
          spec={rootSpec('transition', name)}
          value={transition?.[name]}
        />
      ))}
    </RenderingGroup>
  );
}

/** Projection, light, sky, terrain and the default transition. */
export function RenderingSection(props: StyleSectionProps) {
  return (
    <SettingsSection title="Rendering">
      <div className="flex flex-col gap-2">
        <Projection {...props} />
        <Light {...props} />
        <Sky {...props} />
        <Terrain {...props} />
        <Transition {...props} />
      </div>
    </SettingsSection>
  );
}
