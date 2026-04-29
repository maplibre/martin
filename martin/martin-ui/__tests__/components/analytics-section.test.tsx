import { describe, expect, it, vi } from 'vitest';
import { AnalyticsSection } from '@/components/analytics-section';
import type { AnalyticsData } from '@/lib/types';
import { fireEvent, render, screen } from '../test-utils';

const analytics: AnalyticsData = {
  caches: {},
  fonts: {
    averageRequestDurationMs: 5.1,
    histogram: [
      { count: 0, le: 0.5 },
      { count: 500, le: 1 },
    ],
    requestCount: 500,
  },
  sprites: {
    averageRequestDurationMs: 10.5,
    histogram: [
      { count: 2, le: 0.5 },
      { count: 1000, le: 1 },
    ],
    requestCount: 1000,
  },
  styles: {
    averageRequestDurationMs: 2.3,
    histogram: [
      { count: 2, le: 0.5 },
      { count: 200, le: 1 },
    ],
    requestCount: 200,
  },
  tiles: {
    averageRequestDurationMs: 25.2,
    histogram: [
      { count: 2, le: 0.5 },
      { count: 50000, le: 1 },
    ],
    requestCount: 50000,
  },
};

const analyticsWithCaches: AnalyticsData = {
  ...analytics,
  caches: {
    font: { byZoom: [], hits: 5, misses: 5 },
    pmtiles_directory: {
      byZoom: [{ hits: 10, misses: 0, zoom: 0 }],
      hits: 10,
      misses: 0,
    },
    sprite: { byZoom: [], hits: 0, misses: 0 },
    tile: {
      byZoom: [
        { hits: 50, misses: 0, zoom: 0 },
        { hits: 30, misses: 20, zoom: 5 },
      ],
      hits: 80,
      misses: 20,
    },
  },
};

describe('AnalyticsSection', () => {
  it('renders loading state (skeletons)', () => {
    const { container } = render(<AnalyticsSection isLoading />);

    // Check for skeleton elements by their CSS classes
    const skeletons = container.querySelectorAll('.animate-pulse.rounded-md.bg-muted');
    expect(skeletons).toHaveLength(8); // 2 for metrics per card (4 cards * 2 = 8)

    // Check that all 4 cards are rendered
    expect(screen.getByText('Tiles')).toBeTruthy();
    expect(screen.getByText('Styles')).toBeTruthy();
    expect(screen.getByText('Fonts')).toBeTruthy();
    expect(screen.getByText('Sprites')).toBeTruthy();
  });

  it('renders analytics data', () => {
    render(<AnalyticsSection analytics={analytics} />);

    // Check tiles data
    expect(screen.getByText('25 ms')).toBeTruthy();
    expect(screen.getByText('50,000 requests')).toBeTruthy();

    // Check styles data
    expect(screen.getByText('2 ms')).toBeTruthy();
    expect(screen.getByText('200 requests')).toBeTruthy();

    // Check fonts data
    expect(screen.getByText('5 ms')).toBeTruthy();
    expect(screen.getAllByText('500 requests')[0]).toBeTruthy();

    // Check sprites data
    expect(screen.getByText('11 ms')).toBeTruthy();
    expect(screen.getByText('1,000 requests')).toBeTruthy();
  });

  it('renders error state and calls onRetry', () => {
    const onRetry = vi.fn();
    const error = new Error('Test error');
    render(<AnalyticsSection error={error} isRetrying={false} onRetry={onRetry} />);

    // Check error state is rendered
    expect(screen.getByText('Failed to Load Analytics')).toBeTruthy();
    expect(screen.getByText('Unable to fetch server metrics and usage data')).toBeTruthy();
    expect(screen.getByText('Test error')).toBeTruthy();

    // Check retry button and click it
    const retryButton = screen.getByText('Try Again');
    expect(retryButton).toBeTruthy();
    fireEvent.click(retryButton);
    expect(onRetry).toHaveBeenCalled();
  });

  it('renders cache hit-rates when present', () => {
    render(<AnalyticsSection analytics={analyticsWithCaches} />);

    expect(screen.getByText('Tile cache')).toBeTruthy();
    expect(screen.getByText('80.0% hit')).toBeTruthy();

    expect(screen.getByText('PMTiles dirs')).toBeTruthy();
    expect(screen.getByText('100.0% hit')).toBeTruthy();

    expect(screen.getByText('Font cache')).toBeTruthy();
    expect(screen.getByText('50.0% hit')).toBeTruthy();

    expect(screen.getByText('Sprite cache')).toBeTruthy();
    expect(screen.getByText('no requests yet')).toBeTruthy();
  });

  it('shows a zoom-breakdown info button only for caches with per-zoom data', () => {
    render(<AnalyticsSection analytics={analyticsWithCaches} />);

    // Tile + PMTiles dirs both have byZoom data; font/sprite do not.
    expect(screen.getByLabelText('Tile cache hit rate by zoom')).toBeTruthy();
    expect(screen.getByLabelText('PMTiles dirs hit rate by zoom')).toBeTruthy();
    expect(screen.queryByLabelText('Font cache hit rate by zoom')).toBeNull();
    expect(screen.queryByLabelText('Sprite cache hit rate by zoom')).toBeNull();
  });

  it('hides cache rows when cache data is absent', () => {
    render(<AnalyticsSection analytics={analytics} />);
    expect(screen.queryByText('Tile cache')).toBeNull();
    expect(screen.queryByText('Font cache')).toBeNull();
    expect(screen.queryByText('Sprite cache')).toBeNull();
  });

  it('renders retrying state (button disabled)', () => {
    const onRetry = vi.fn();
    const error = new Error('Retry error');
    render(<AnalyticsSection error={error} isRetrying={true} onRetry={onRetry} />);

    // Check that the button shows "Retrying..." and is disabled
    const retryButton = screen.getByText('Retrying...');
    expect(retryButton).toBeTruthy();
    expect((retryButton as HTMLButtonElement).disabled).toBe(true);
  });
});
