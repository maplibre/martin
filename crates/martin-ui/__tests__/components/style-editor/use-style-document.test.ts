import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { act, cleanup, fireEvent, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStyleDocument } from '@/components/style-editor/use-style-document';

describe('useStyleDocument', () => {
  beforeEach(() => {
    const style: StyleSpecification = {
      layers: [{ id: 'background', paint: { 'background-color': '#fff' }, type: 'background' }],
      sources: {},
      version: 8,
    };
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(new Response(JSON.stringify(style)))),
    );
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('merges edits sharing a key within one interaction and starts a new entry after it ends', async () => {
    const { result } = renderHook(() => useStyleDocument('https://example.com/style.json'));
    await waitFor(() => expect(result.current.style).not.toBeNull());
    const setOpacity = (value: number) =>
      act(() =>
        result.current.apply(
          `background · opacity ${value}`,
          (s) => {
            s.layers[0] = {
              id: 'background',
              paint: { 'background-opacity': value },
              type: 'background',
            };
          },
          'background:background-opacity:number-input',
        ),
      );

    setOpacity(0.5);
    setOpacity(0.4);
    fireEvent.focusOut(document.body);
    setOpacity(0.3);
    setOpacity(0.2);

    expect(result.current.edits.map((e) => e.label)).toEqual([
      'background · opacity 0.4',
      'background · opacity 0.2',
    ]);
  });
});
