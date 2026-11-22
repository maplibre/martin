import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { StyleResourcesProvider } from '@/components/style-editor/style-resources';
import { FontStackPicker } from '@/components/style-editor/value-editors/pickers/font-picker';

vi.mock('lucide-react', async (importOriginal) => importOriginal());

function renderPicker(value: string[], onChange = vi.fn()) {
  render(
    <StyleResourcesProvider
      value={{
        fontsInStyle: ['Noto Sans Regular', 'Noto Sans Bold', 'Noto Sans Italic', 'Roboto Medium'],
        glyphs: 'https://glyphs.example.com/fonts/{fontstack}/{range}.pbf',
        sprite: undefined,
      }}
    >
      <FontStackPicker onChange={onChange} value={value} />
    </StyleResourcesProvider>,
  );
  return onChange;
}

describe('FontStackPicker', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('missing', { status: 404 })),
    );
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('adds the variant picked from a searched family to the end of the stack', () => {
    const onChange = renderPicker(['Noto Sans Regular']);

    fireEvent.click(screen.getByRole('button', { name: 'Add font' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Search fonts' }), {
      target: { value: 'noto' },
    });
    const dialog = screen.getByRole('dialog', { name: 'Choose font' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Italic' }));

    expect(onChange.mock.calls).toEqual([[['Noto Sans Regular', 'Noto Sans Italic']]]);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('groups the style fonts by family with their variants', () => {
    renderPicker(['Noto Sans Regular']);

    fireEvent.click(screen.getByRole('button', { name: 'Add font' }));
    const section = screen.getByRole('region', { name: 'Used in this style' });

    expect(
      within(section)
        .getAllByRole('listitem')
        .map((li) => li.textContent),
    ).toMatchInlineSnapshot(`
      [
        "Noto SansRegularBoldItalic",
        "RobotoMedium",
      ]
    `);
  });

  it('replaces the clicked entry instead of appending', () => {
    const onChange = renderPicker(['Noto Sans Regular', 'Roboto Medium']);

    fireEvent.click(screen.getByRole('button', { name: 'Replace Roboto Medium' }));
    fireEvent.click(screen.getByRole('button', { name: 'Bold' }));

    expect(onChange.mock.calls).toEqual([[['Noto Sans Regular', 'Noto Sans Bold']]]);
  });

  it('offers a custom name when nothing matches and a glyph server cannot list fonts', () => {
    const onChange = renderPicker(['Noto Sans Regular']);

    fireEvent.click(screen.getByRole('button', { name: 'Add font' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Search fonts' }), {
      target: { value: 'Comic Neue' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Use custom name/ }));

    expect(onChange.mock.calls).toEqual([[['Noto Sans Regular', 'Comic Neue']]]);
  });

  it('shows the muted note that this glyph server cannot list its fonts', () => {
    renderPicker(['Noto Sans Regular']);

    fireEvent.click(screen.getByRole('button', { name: 'Add font' }));

    expect(
      screen.getByText('This glyph server cannot list its fonts. Type a font name to use it.'),
    ).toBeTruthy();
  });

  it('removes a font from the stack', () => {
    const onChange = renderPicker(['Noto Sans Regular', 'Roboto Medium']);

    fireEvent.click(screen.getByRole('button', { name: 'Remove Noto Sans Regular' }));

    expect(onChange.mock.calls).toEqual([[['Roboto Medium']]]);
  });

  it('keeps the last font because a stack cannot be empty', () => {
    renderPicker(['Noto Sans Regular']);

    expect(
      (screen.getByRole('button', { name: 'Remove Noto Sans Regular' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it('moves a font earlier with the arrow button', () => {
    const onChange = renderPicker(['Noto Sans Regular', 'Roboto Medium', 'Noto Sans Bold']);

    fireEvent.click(screen.getByRole('button', { name: 'Move Roboto Medium earlier' }));

    expect(onChange.mock.calls).toEqual([
      [['Roboto Medium', 'Noto Sans Regular', 'Noto Sans Bold']],
    ]);
  });

  it('moves a font later with the arrow button', () => {
    const onChange = renderPicker(['Noto Sans Regular', 'Roboto Medium']);

    fireEvent.click(screen.getByRole('button', { name: 'Move Noto Sans Regular later' }));

    expect(onChange.mock.calls).toEqual([[['Roboto Medium', 'Noto Sans Regular']]]);
  });

  it('marks only the first font as primary', () => {
    renderPicker(['Noto Sans Regular', 'Roboto Medium']);

    expect(screen.getAllByText('primary')).toHaveLength(1);
  });

  it('closes the popup on Escape', () => {
    renderPicker(['Noto Sans Regular']);

    fireEvent.click(screen.getByRole('button', { name: 'Add font' }));
    fireEvent.keyDown(document, { key: 'Escape' });

    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
