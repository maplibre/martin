import type { DemoLayerEntry } from '@/types/demo';
import type { FilterState } from './FilterPanel';

interface TimeSliderProps {
  layer: DemoLayerEntry | null;
  filterState: FilterState;
  onFilterChange: (name: string, value: string | number) => void;
}

const START_PARAM = 'start_time';
const END_PARAM = 'end_time';

function hasTimeParams(layer: DemoLayerEntry | null): boolean {
  if (!layer?.allowedParameters?.length) return false;
  const names = new Set(layer.allowedParameters.map((p) => p.name));
  return names.has(START_PARAM) && names.has(END_PARAM);
}

export default function TimeSlider({ layer, filterState, onFilterChange }: TimeSliderProps) {
  if (!layer || !hasTimeParams(layer)) return null;

  const startParam = layer.allowedParameters?.find((p) => p.name === START_PARAM);
  const endParam = layer.allowedParameters?.find((p) => p.name === END_PARAM);
  if (!startParam || !endParam) return null;

  const min = (startParam.min ?? endParam.min ?? 1900) as number;
  const max = (endParam.max ?? startParam.max ?? 2025) as number;
  const start = Number(filterState[START_PARAM] ?? startParam.default ?? min);
  const end = Number(filterState[END_PARAM] ?? endParam.default ?? max);

  return (
    <div className="flex flex-col gap-2">
      <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
        Time range
      </span>
      <div className="flex items-center gap-2">
        <input
          className="w-16 rounded border border-border bg-background px-2 py-1 text-[11px] font-mono text-foreground"
          max={max}
          min={min}
          onChange={(e) => {
            const raw = e.target.valueAsNumber;
            onFilterChange(START_PARAM, Number.isFinite(raw) ? raw : min);
          }}
          type="number"
          value={start}
        />
        <span className="text-[10px] text-muted-foreground">–</span>
        <input
          className="w-16 rounded border border-border bg-background px-2 py-1 text-[11px] font-mono text-foreground"
          max={max}
          min={min}
          onChange={(e) => {
            const raw = e.target.valueAsNumber;
            onFilterChange(END_PARAM, Number.isFinite(raw) ? raw : max);
          }}
          type="number"
          value={end}
        />
      </div>
    </div>
  );
}
