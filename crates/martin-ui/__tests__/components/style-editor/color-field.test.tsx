import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ColorField } from '@/components/style-editor/color-field';
import { Color, colorKey } from '@/components/style-editor/colors';
import { StylePaletteProvider } from '@/components/style-editor/style-palette';

vi.mock('lucide-react', async (importOriginal) => importOriginal());

describe('ColorField', () => {
  afterEach(cleanup);

  it('applies a color picked from the style palette and closes the picker', () => {
    const onChange = vi.fn();
    render(
      <StylePaletteProvider
        colors={[
          { color: Color.parse('#e9ac77') as Color, key: '#e9ac77', uses: 15 },
          { color: Color.parse('#a0c8f0') as Color, key: '#a0c8f0', uses: 3 },
        ]}
      >
        <ColorField onChange={onChange} value={Color.parse('#e9ac77') as Color} />
      </StylePaletteProvider>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Choose a color' }));
    fireEvent.click(screen.getByRole('button', { name: '#a0c8f0, used 3 times' }));

    expect(onChange.mock.calls.map(([c]) => colorKey(c))).toEqual(['#a0c8f0']);
    expect(screen.queryByRole('dialog', { name: 'Colors in this style' })).toBeNull();
  });

  it('shows an empty state when the style has no colors yet', () => {
    render(<ColorField onChange={vi.fn()} value={Color.parse('#000000') as Color} />);

    fireEvent.click(screen.getByRole('button', { name: 'Choose a color' }));

    expect(screen.getByRole('dialog', { name: 'Colors in this style' }).textContent).toContain(
      'No colors yet.',
    );
  });
});
