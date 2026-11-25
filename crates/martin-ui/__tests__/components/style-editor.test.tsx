import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { StyleEditor } from '@/components/style-editor';

vi.mock('@/lib/api', () => ({
  buildMartinUrl: vi.fn((path: string) => `http://localhost:3000${path}`),
}));

vi.mock('lucide-react', async (importOriginal) => importOriginal());

vi.mock('maplibre-gl/dist/maplibre-gl.css', () => ({}));

vi.mock('maplibre-gl', () => {
  class FakeMap {
    on = vi.fn();
    off = vi.fn();
    remove = vi.fn();
    setStyle = vi.fn();
    getZoom = vi.fn(() => 3);
    getCanvas = vi.fn(() => ({ style: {} }));
    addControl = vi.fn();
    getCenter = vi.fn(() => ({ lat: 0, lng: 0 }));
    queryRenderedFeatures = vi.fn(() => []);
    querySourceFeatures = vi.fn(() => []);
    getSource = vi.fn(() => undefined);
  }
  class NavigationControl {}
  return { Map: FakeMap, NavigationControl };
});

describe('StyleEditor', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => new Promise(() => {})),
    );
  });

  it('shows the style name and file name in the header with the full path as title', () => {
    render(
      <StyleEditor
        onClose={vi.fn()}
        style={{
          colors: [],
          last_modified_at: '2023-01-01',
          layer_count: 5,
          path: '/styles/test-style.json',
          type: 'vector',
          version_hash: 'abc123',
        }}
        styleName="test-style"
      />,
    );
    expect(screen.getByRole('heading', { name: 'test-style' }).textContent).toBe('test-style');
    expect(screen.getByText('test-style.json').getAttribute('title')).toBe(
      '/styles/test-style.json',
    );
  });

  it('shows the loading state while the style is fetched', () => {
    render(
      <StyleEditor
        onClose={vi.fn()}
        style={{
          colors: [],
          last_modified_at: '2023-01-01',
          layer_count: 5,
          path: '/s.json',
          type: 'vector',
          version_hash: 'h',
        }}
        styleName="test-style"
      />,
    );
    expect(screen.getByRole('status').textContent).toBe(' Loading style…');
  });

  it('requests the style from the Martin style endpoint', () => {
    const fetchMock = vi.fn((_url: string) => new Promise(() => {}));
    vi.stubGlobal('fetch', fetchMock);
    render(
      <StyleEditor
        onClose={vi.fn()}
        style={{
          colors: [],
          last_modified_at: '2023-01-01',
          layer_count: 5,
          path: '/s.json',
          type: 'vector',
          version_hash: 'h',
        }}
        styleName="test-style"
      />,
    );
    expect(fetchMock.mock.calls.map((c) => c[0])).toEqual([
      'http://localhost:3000/style/test-style',
    ]);
  });

  it('shows the error when the fetch fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve({ ok: false, status: 404, statusText: 'Not Found' })),
    );
    render(
      <StyleEditor
        onClose={vi.fn()}
        style={{
          colors: [],
          last_modified_at: '2023-01-01',
          layer_count: 5,
          path: '/s.json',
          type: 'vector',
          version_hash: 'h',
        }}
        styleName="test-style"
      />,
    );
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Could not load test-style: 404 Not Found',
    );
  });

  it('shows the error when the network request rejects', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.reject(new Error('network down'))),
    );
    render(
      <StyleEditor
        onClose={vi.fn()}
        style={{
          colors: [],
          last_modified_at: '2023-01-01',
          layer_count: 5,
          path: '/s.json',
          type: 'vector',
          version_hash: 'h',
        }}
        styleName="test-style"
      />,
    );
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Could not load test-style: network down',
    );
  });

  it('calls onClose when the back button is clicked', async () => {
    const onClose = vi.fn();
    render(
      <StyleEditor
        onClose={onClose}
        style={{
          colors: [],
          last_modified_at: '2023-01-01',
          layer_count: 5,
          path: '/s.json',
          type: 'vector',
          version_hash: 'h',
        }}
        styleName="test-style"
      />,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Catalog' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
