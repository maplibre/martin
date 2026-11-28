import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { StyleResourcesProvider } from '@/components/style-editor/style-resources';
import { IconPicker } from '@/components/style-editor/value-editors/pickers/icon-picker';

function stubSprite() {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      if (url.endsWith('@2x.json')) return new Response('', { status: 404 });
      return new Response(
        JSON.stringify({
          airport_11: { height: 17, pixelRatio: 1, width: 17, x: 0, y: 0 },
          bank_11: { height: 17, pixelRatio: 1, sdf: true, width: 17, x: 34, y: 0 },
          bus_11: { height: 17, pixelRatio: 1, width: 17, x: 17, y: 0 },
        }),
        { status: 200 },
      );
    }),
  );
}

function renderPicker(onChange: (name: string) => void, sprite = 'https://example.com/sprite') {
  render(
    <StyleResourcesProvider value={{ fontsInStyle: [], glyphs: undefined, sprite }}>
      <IconPicker onChange={onChange} value="bus_11" />
    </StyleResourcesProvider>,
  );
}

describe('IconPicker', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('applies the icon picked from the filtered grid and closes the popup', async () => {
    stubSprite();
    const onChange = vi.fn();
    renderPicker(onChange);

    fireEvent.click(screen.getByRole('button', { name: 'Choose an icon' }));
    await screen.findByTitle('airport_11 · 17×17 px');
    fireEvent.change(screen.getByLabelText('Search icons'), { target: { value: 'air' } });

    expect(screen.getByText('1 icon')).toBeTruthy();
    fireEvent.click(screen.getByTitle('airport_11 · 17×17 px'));

    expect(onChange.mock.calls).toEqual([['airport_11']]);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('applies a typed custom name when no icon matches', async () => {
    stubSprite();
    const onChange = vi.fn();
    renderPicker(onChange);

    fireEvent.click(screen.getByRole('button', { name: 'Choose an icon' }));
    await screen.findByTitle('bank_11 · 17×17 px · SDF');
    fireEvent.change(screen.getByLabelText('Search icons'), { target: { value: '{class}_11' } });
    fireEvent.keyDown(screen.getByLabelText('Search icons'), { key: 'Enter' });

    expect(onChange.mock.calls).toEqual([['{class}_11']]);
  });

  it('picks the highlighted icon with the arrow keys and Enter', async () => {
    stubSprite();
    const onChange = vi.fn();
    renderPicker(onChange);

    fireEvent.click(screen.getByRole('button', { name: 'Choose an icon' }));
    await waitFor(() => expect(screen.getByText('3 icons')).toBeTruthy());
    const search = screen.getByLabelText('Search icons');
    fireEvent.keyDown(search, { key: 'ArrowRight' });
    fireEvent.keyDown(search, { key: 'Enter' });

    expect(onChange.mock.calls).toEqual([['bank_11']]);
  });

  it('reports a sprite that fails to load', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('', { status: 404 })),
    );
    renderPicker(vi.fn(), 'https://broken.example.com/sprite');

    fireEvent.click(screen.getByRole('button', { name: 'Choose an icon' }));

    expect(
      (await screen.findByTitle('No sprite index at https://broken.example.com/sprite.json'))
        .textContent,
    ).toMatchInlineSnapshot(
      `"Could not load the sprite. No sprite index at https://broken.example.com/sprite.json"`,
    );
  });
});
