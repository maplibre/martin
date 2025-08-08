import { AnalyticsSection } from '@/components/analytics-section';
import type { AnalyticsData } from '@/lib/types';
import { fireEvent, render, screen } from '../test-utils';

const analytics: AnalyticsData = {
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

describe('AnalyticsSection', () => {
  it('renders loading state (skeletons)', () => {
    const { container } = render(<AnalyticsSection isLoading />);

    // Check for skeleton elements by their CSS classes
    const skeletons = container.querySelectorAll('.animate-pulse.rounded-md.bg-muted');
    expect(skeletons).toHaveLength(8); // 2 for metrics per card (4 cards * 2 = 8)

    // Check that all 4 cards are rendered
    expect(screen.getByText('Tiles')).toBeInTheDocument();
    expect(screen.getByText('Styles')).toBeInTheDocument();
    expect(screen.getByText('Fonts')).toBeInTheDocument();
    expect(screen.getByText('Sprites')).toBeInTheDocument();
  });

  it('renders analytics data', () => {
    render(<AnalyticsSection analytics={analytics} />);

    // Check tiles data
    expect(screen.getByText('25 ms')).toBeInTheDocument();
    expect(screen.getByText('50,000 requests')).toBeInTheDocument();

    // Check styles data
    expect(screen.getByText('2 ms')).toBeInTheDocument();
    expect(screen.getByText('200 requests')).toBeInTheDocument();

    // Check fonts data
    expect(screen.getByText('5 ms')).toBeInTheDocument();
    expect(screen.getAllByText('500 requests')[0]).toBeInTheDocument();

    // Check sprites data
    expect(screen.getByText('11 ms')).toBeInTheDocument();
    expect(screen.getByText('1,000 requests')).toBeInTheDocument();
  });

  it('renders error state and calls onRetry', () => {
    const onRetry = jest.fn();
    const error = new Error('Test error');
    render(<AnalyticsSection error={error} isRetrying={false} onRetry={onRetry} />);

    // Check error state is rendered
    expect(screen.getByText('Failed to Load Analytics')).toBeInTheDocument();
    expect(screen.getByText('Unable to fetch server metrics and usage data')).toBeInTheDocument();
    expect(screen.getByText('Test error')).toBeInTheDocument();

    // Check retry button and click it
    const retryButton = screen.getByText('Try Again');
    expect(retryButton).toBeInTheDocument();
    fireEvent.click(retryButton);
    expect(onRetry).toHaveBeenCalled();
  });

  it('renders retrying state (button disabled)', () => {
    const onRetry = jest.fn();
    const error = new Error('Retry error');
    render(<AnalyticsSection error={error} isRetrying={true} onRetry={onRetry} />);

    // Check that the button shows "Retrying..." and is disabled
    const retryButton = screen.getByText('Retrying...');
    expect(retryButton).toBeInTheDocument();
    expect(retryButton).toBeDisabled();
  });
});
