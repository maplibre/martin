import { render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { acquireThumbSlot, MAX_CONCURRENT_THUMBS, TilesCatalog } from '@/components/catalogs/tiles';
import type { Catalog } from '@/lib/types.gen';

describe('acquireThumbSlot', () => {
  it('limits concurrency, skips cancelled waiters and starts the next job on release', async () => {
    const held = await Promise.all(
      Array.from({ length: MAX_CONCURRENT_THUMBS }, () =>
        acquireThumbSlot(new AbortController().signal),
      ),
    );
    expect(held.every((release) => release !== null)).toBe(true);

    const cancelled = new AbortController();
    const cancelledSlot = acquireThumbSlot(cancelled.signal);
    const nextSlot = acquireThumbSlot(new AbortController().signal);
    let nextGranted = false;
    nextSlot.then(() => {
      nextGranted = true;
    });

    cancelled.abort();
    expect(await cancelledSlot).toBeNull();
    await Promise.resolve();
    expect(nextGranted).toBe(false);

    held[0]?.();
    const nextRelease = await nextSlot;
    expect(nextRelease).not.toBeNull();

    nextRelease?.();
    for (const release of held.slice(1)) release?.();
  });
});

describe('TilesCatalog thumbnails', () => {
  const tileSources: Catalog['tiles'] = {
    source: { content_type: 'application/x-protobuf' },
  };
  const original = globalThis.IntersectionObserver;
  let trigger: () => void;
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    globalThis.IntersectionObserver = class {
      constructor(callback: (entries: { isIntersecting: boolean }[]) => void) {
        trigger = () => callback([{ isIntersecting: true }]);
      }
      disconnect() {}
      observe() {}
      unobserve() {}
    } as unknown as typeof IntersectionObserver;
    fetchMock = vi.fn(() => new Promise(() => {}));
    vi.stubGlobal('fetch', fetchMock);
  });

  afterEach(() => {
    globalThis.IntersectionObserver = original;
    vi.unstubAllGlobals();
  });

  it('fetches nothing until the card becomes visible', async () => {
    render(<TilesCatalog searchQuery="" tileSources={tileSources} />);
    await Promise.resolve();
    expect(fetchMock).not.toHaveBeenCalled();

    trigger();
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
  });
});
