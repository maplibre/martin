import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AddProperty } from '@/components/style-editor/inspector/add-property';
import { layerProperties } from '@/components/style-editor/style-spec';

vi.mock('lucide-react', async (importOriginal) => importOriginal());

describe('AddProperty', () => {
  afterEach(cleanup);

  it('filters the real runtime spec properties by name while typing', () => {
    render(
      <AddProperty entries={layerProperties('line', 'paint')} layerType="line" onAdd={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole('button', { name: /add property/i }));
    fireEvent.change(screen.getByPlaceholderText('Search properties'), {
      target: { value: 'width' },
    });

    expect(
      screen
        .getAllByRole('button')
        .map((b) => b.textContent)
        .filter((t) => t?.startsWith('line-')),
    ).toMatchInlineSnapshot(`
      [
        "line-width",
        "line-gap-width",
      ]
    `);
  });

  it('shows the spec documentation once the reference has loaded', async () => {
    render(
      <AddProperty entries={layerProperties('line', 'paint')} layerType="line" onAdd={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole('button', { name: /add property/i }));
    fireEvent.change(screen.getByPlaceholderText('Search properties'), {
      target: { value: 'stroke thickness' },
    });

    expect((await screen.findByText('Stroke thickness.')).textContent).toBe('Stroke thickness.');
  });
});
