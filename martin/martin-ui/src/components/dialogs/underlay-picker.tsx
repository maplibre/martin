import { SegmentedControl } from '@/components/ui/segmented-control';
import { UNDERLAY_PROVIDERS, type UnderlayProviderId } from './underlay-providers';

interface UnderlayPickerProps {
  value: UnderlayProviderId | undefined;
  onChange: (value: UnderlayProviderId | undefined) => void;
}

type Choice = UnderlayProviderId | 'none';

const OPTIONS = [
  { label: 'None', value: 'none' },
  ...UNDERLAY_PROVIDERS.map((p) => ({ label: p.label, value: p.id })),
] as const satisfies readonly { label: string; value: Choice }[];

export function UnderlayPicker({ value, onChange }: UnderlayPickerProps) {
  return (
    <SegmentedControl<Choice>
      className="overflow-x-auto"
      label="Underlay basemap"
      onChange={(v) => onChange(v === 'none' ? undefined : v)}
      options={OPTIONS}
      value={value ?? 'none'}
    />
  );
}
