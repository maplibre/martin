import type { AllowedParameter, DemoLayerEntry } from '@/types/demo';

export type FilterState = Record<string, string | number>;

interface FilterPanelProps {
  layer: DemoLayerEntry | null;
  filterState: FilterState;
  onFilterChange: (name: string, value: string | number) => void;
}

function getDefaultValue(param: AllowedParameter): string | number {
  if (param.default !== undefined) return param.default;
  if (param.type === 'number' || param.type === 'range') return param.min ?? 0;
  return '';
}

export function getInitialFilterState(layer: DemoLayerEntry | null): FilterState {
  if (!layer?.allowedParameters?.length) return {};
  const state: FilterState = {};
  for (const param of layer.allowedParameters) {
    state[param.name] = getDefaultValue(param);
  }
  return state;
}

export default function FilterPanel({ layer, filterState, onFilterChange }: FilterPanelProps) {
  if (!layer?.allowedParameters?.length) return null;

  return (
    <div className="flex flex-col gap-3">
      <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
        Filters
      </span>
      <div className="flex flex-wrap gap-3">
        {layer.allowedParameters.map((param) => {
          const value = filterState[param.name] ?? getDefaultValue(param);
          const label = param.label ?? param.name;

          if (param.type === 'number' || param.type === 'range') {
            const min = param.min ?? 0;
            const max = param.max ?? 100;
            const inputId = `filter-${param.name}`;
            return (
              <div className="flex flex-col gap-1" key={param.name}>
                <label className="text-[10px] font-mono text-muted-foreground" htmlFor={inputId}>
                  {label}
                </label>
                <input
                  className="w-24 rounded border border-border bg-background px-2 py-1 text-[11px] font-mono text-foreground"
                  id={inputId}
                  max={max}
                  min={min}
                  onChange={(e) => {
                    const raw = e.target.valueAsNumber;
                    const safe = Number.isFinite(raw) ? raw : min;
                    onFilterChange(param.name, safe);
                  }}
                  type="number"
                  value={Number(value)}
                />
              </div>
            );
          }

          if (param.type === 'string' && param.options?.length) {
            const selectId = `filter-${param.name}`;
            return (
              <div className="flex flex-col gap-1" key={param.name}>
                <label className="text-[10px] font-mono text-muted-foreground" htmlFor={selectId}>
                  {label}
                </label>
                <select
                  className="rounded border border-border bg-background px-2 py-1 text-[11px] font-mono text-foreground"
                  id={selectId}
                  onChange={(e) => onFilterChange(param.name, e.target.value)}
                  value={String(value)}
                >
                  {param.options.map((opt) => (
                    <option key={opt} value={opt}>
                      {opt || '(any)'}
                    </option>
                  ))}
                </select>
              </div>
            );
          }

          const textId = `filter-${param.name}`;
          return (
            <div className="flex flex-col gap-1" key={param.name}>
              <label className="text-[10px] font-mono text-muted-foreground" htmlFor={textId}>
                {label}
              </label>
              <input
                className="w-28 rounded border border-border bg-background px-2 py-1 text-[11px] font-mono text-foreground"
                id={textId}
                onChange={(e) => onFilterChange(param.name, e.target.value)}
                type="text"
                value={String(value)}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
