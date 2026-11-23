import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { colorKey, writeColorSites } from '@/components/style-editor/colors';
import { PalettePanel } from '@/components/style-editor/palette/palette-panel';

vi.mock('lucide-react', async (importOriginal) => importOriginal());

function ControlledPalette({ initial }: { initial: StyleSpecification }) {
  const [style, setStyle] = useState(initial);
  const [selected, setSelected] = useState<string | null>('#ff0000');
  return (
    <PalettePanel
      highlight={false}
      onHighlight={vi.fn()}
      onMerge={vi.fn()}
      onRecolor={(token, next) => {
        const copy = structuredClone(style);
        writeColorSites(copy, token.sites, () => next);
        setStyle(copy);
        setSelected(colorKey(next));
      }}
      onSelect={setSelected}
      onSelectLayer={vi.fn()}
      selected={selected}
      style={style}
    />
  );
}

describe('PalettePanel', () => {
  afterEach(cleanup);

  it('keeps the opacity input focused while typing changes the color', () => {
    render(
      <ControlledPalette
        initial={{
          layers: [{ id: 'bg', paint: { 'background-color': '#ff0000' }, type: 'background' }],
          sources: {},
          version: 8,
        }}
      />,
    );
    const input = screen.getByRole('spinbutton', { name: 'Opacity in percent' });
    input.focus();

    fireEvent.change(input, { target: { value: '5' } });
    fireEvent.change(input, { target: { value: '50' } });

    expect(document.activeElement).toBe(input);
    expect(screen.getByRole('spinbutton', { name: 'Opacity in percent' })).toBe(input);
  });
});
