import { Button } from '@/components/ui/button';
import { UNDERLAY_PROVIDERS, type UnderlayProviderId } from './underlay-providers';

interface UnderlayPickerProps {
  value: UnderlayProviderId | undefined;
  onChange: (value: UnderlayProviderId | undefined) => void;
}

export function UnderlayPicker({ value, onChange }: UnderlayPickerProps) {
  return (
    <fieldset className="absolute top-2 left-2 z-10 flex flex-wrap gap-1 rounded-md border-0 bg-background/90 p-1 shadow-md backdrop-blur">
      <legend className="sr-only">Underlay basemap</legend>
      <Button
        onClick={() => onChange(undefined)}
        size="sm"
        variant={value === undefined ? 'default' : 'outline'}
      >
        None
      </Button>
      {UNDERLAY_PROVIDERS.map((provider) => (
        <Button
          key={provider.id}
          onClick={() => onChange(provider.id)}
          size="sm"
          variant={value === provider.id ? 'default' : 'outline'}
        >
          {provider.label}
        </Button>
      ))}
    </fieldset>
  );
}
