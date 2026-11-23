import type { LayerSpecification } from '@maplibre/maplibre-gl-style-spec';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PropertyRow } from '@/components/style-editor/inspector/property-row';
import { layerProperties, type PropertyEntry } from '@/components/style-editor/style-spec';
import { TooltipProvider } from '@/components/ui/tooltip';

vi.mock('lucide-react', async (importOriginal) => importOriginal());

function ControlledRow({
  layer,
  entry,
  initial,
  saved,
}: {
  layer: LayerSpecification;
  entry: PropertyEntry;
  initial: unknown;
  saved: unknown[];
}) {
  const [raw, setRaw] = useState(initial);
  return (
    <TooltipProvider>
      <PropertyRow
        attributes={[]}
        entry={entry}
        label={entry.name}
        layer={layer}
        onSelectColor={vi.fn()}
        onSet={(_, value) => {
          saved.push(value);
          setRaw(value);
        }}
        raw={raw}
        zoom={14}
      />
    </TooltipProvider>
  );
}

describe('PropertyRow', () => {
  afterEach(cleanup);

  it('opens the expression editor for a fixed value and keeps it open', () => {
    const layer: LayerSpecification = {
      id: 'roads',
      paint: { 'line-color': '#ff0000' },
      source: 'omt',
      'source-layer': 'transportation',
      type: 'line',
    };
    const entry = layerProperties('line', 'paint').find((e) => e.name === 'line-color');
    if (!entry) throw new Error('line-color missing');
    const saved: unknown[] = [];
    render(<ControlledRow entry={entry} initial="#ff0000" layer={layer} saved={saved} />);

    fireEvent.click(screen.getByRole('button', { name: 'Mode of line-color: Fixed' }));
    fireEvent.click(screen.getByRole('button', { name: /^Expression/ }));

    expect((screen.getByLabelText('line-color expression') as HTMLTextAreaElement).value).toBe(
      '"#ff0000"',
    );
    expect(screen.getByRole('button', { name: 'Mode of line-color: Expression' })).toBeTruthy();
    expect(saved).toEqual(['#ff0000']);
  });

  it('opens the category editor without saving while no attribute values are known', () => {
    const layer: LayerSpecification = {
      id: 'roads',
      paint: { 'line-color': '#ff0000' },
      source: 'omt',
      'source-layer': 'transportation',
      type: 'line',
    };
    const entry = layerProperties('line', 'paint').find((e) => e.name === 'line-color');
    if (!entry) throw new Error('line-color missing');
    const saved: unknown[] = [];
    render(<ControlledRow entry={entry} initial="#ff0000" layer={layer} saved={saved} />);

    fireEvent.click(screen.getByRole('button', { name: 'Mode of line-color: Fixed' }));
    fireEvent.click(screen.getByRole('button', { name: /^By category/ }));

    expect(screen.getByRole('button', { name: 'Mode of line-color: By category' })).toBeTruthy();
    expect(screen.queryByLabelText('line-color expression')).toBeNull();
    expect(saved).toEqual([]);
  });
});
