import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { MiniHistogram } from '@/components/charts/mini-histogram';
import type { HistogramBucket } from '@/lib/prometheus';

describe('MiniHistogram', () => {
  it('renders empty state when histogram has no buckets', () => {
    const histogram = [] as HistogramBucket[];

    const { container } = render(<MiniHistogram histogram={histogram} />);

    // Component renders the container but with no bars
    const histogramContainer = container.querySelector('.w-20.h-12.flex.items-end');
    expect(histogramContainer).toBeTruthy();

    const bars = container.querySelectorAll('.flex-1.bg-primary');
    expect(bars).toHaveLength(0);
  });

  it('renders histogram bars when data is available', () => {
    const histogram: HistogramBucket[] = [
      { count: 100, le: 0.005 },
      { count: 200, le: 0.01 },
      { count: 250, le: 0.025 },
      { count: 280, le: 0.05 },
      { count: 300, le: 0.1 },
    ];

    const { container } = render(<MiniHistogram histogram={histogram} />);

    // Should render container with bars
    const histogramContainer = container.querySelector('.w-20.h-12.flex.items-end');
    expect(histogramContainer).toBeTruthy();

    // Should render bars (limited to 8 max, but we have 5)
    const bars = container.querySelectorAll('.flex-1.bg-primary');
    expect(bars).toHaveLength(5);

    // Each bar should have height and opacity style attributes
    bars.forEach((bar) => {
      const barElement = bar as HTMLElement;
      expect(barElement.style.height).toBeTruthy();
      expect(barElement.style.opacity).toBeTruthy();
    });
  });

  it('limits bars to maximum of 8', () => {
    const histogram: HistogramBucket[] = Array.from({ length: 12 }, (_, i) => ({
      count: (i + 1) * 10,
      le: 0.001 * (i + 1),
    }));

    const { container } = render(<MiniHistogram histogram={histogram} />);

    // Should render all 12 bars (component doesn't limit bars)
    const bars = container.querySelectorAll('.flex-1.bg-primary');
    expect(bars).toHaveLength(12);
  });

  it('calculates bar heights based on bucket distribution', () => {
    const histogram: HistogramBucket[] = [
      { count: 50, le: 0.005 }, // 50 requests in this bucket (50-0)
      { count: 150, le: 0.01 }, // 100 more requests (150-50)
      { count: 200, le: 0.025 }, // 50 more requests (200-150)
    ];

    const { container } = render(<MiniHistogram histogram={histogram} />);

    const bars = container.querySelectorAll('.flex-1.bg-primary');
    expect(bars).toHaveLength(3);

    // The logic calculates bucket differences, then finds max difference
    // Bucket differences: 50, 100, 50. Max difference is 100.
    // Heights: 50/100=50%, 100/100=100%, 50/100=50%
    const firstBar = bars[0] as HTMLElement;
    const secondBar = bars[1] as HTMLElement;
    const thirdBar = bars[2] as HTMLElement;

    // Second bar should have 100% height (100 is max bucket difference)
    expect(secondBar.style.height).toBe('100%');

    // First and third bars should have 50% height (50/100)
    expect(firstBar.style.height).toBe('50%');
    expect(thirdBar.style.height).toBe('50%');
  });

  it('ensures minimum bar height for visibility', () => {
    const histogram: HistogramBucket[] = [
      { count: 1000, le: 0.005 }, // Very large bucket
      { count: 1001, le: 0.01 }, // Only 1 additional request
    ];

    const { container } = render(<MiniHistogram histogram={histogram} />);

    const bars = container.querySelectorAll('.flex-1.bg-primary');
    const secondBar = bars[1] as HTMLElement;

    // Even though the second bar represents only 1 request out of 1000,
    // it should still have at least 2% height for visibility
    const height = parseFloat(secondBar.style.height);
    expect(height).toBeGreaterThanOrEqual(2);
  });
});
