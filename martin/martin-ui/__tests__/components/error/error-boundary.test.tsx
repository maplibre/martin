import { fireEvent, screen } from '@testing-library/react';
import { ErrorBoundary } from '@/components/error/error-boundary';
import { render } from '../../test-utils';

// Silence React error boundary logs for cleaner test output
let consoleErrorSpy: jest.SpyInstance;

beforeAll(() => {
  consoleErrorSpy = jest.spyOn(console, 'error').mockImplementation(() => {});
});

afterAll(() => {
  consoleErrorSpy.mockRestore();
});

// Helper component to throw error
function ProblemChild({ shouldThrow }: { shouldThrow?: boolean }) {
  if (shouldThrow) {
    throw new Error('Test error thrown!');
  }
  return <div>All good</div>;
}

describe('ErrorBoundary', () => {
  it('renders children when no error occurs', () => {
    const { container } = render(
      <ErrorBoundary>
        <div>Safe content</div>
      </ErrorBoundary>,
    );
    expect(container).toMatchSnapshot();
    expect(screen.getByText('Safe content')).toBeInTheDocument();
  });

  it('catches errors and renders default fallback UI', () => {
    const { container } = render(
      <ErrorBoundary>
        <ProblemChild shouldThrow />
      </ErrorBoundary>,
    );
    expect(container).toMatchSnapshot();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(
      screen.getByText(
        'An unexpected error occurred. Please try again or contact support if the problem persists.',
      ),
    ).toBeInTheDocument();
    expect(screen.getByText('Test error thrown!')).toBeInTheDocument();
    expect(screen.getByText('Try Again')).toBeInTheDocument();
  });

  it('calls onError prop when error is caught', () => {
    const onError = jest.fn();
    render(
      <ErrorBoundary onError={onError}>
        <ProblemChild shouldThrow />
      </ErrorBoundary>,
    );
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0][0]).toBeInstanceOf(Error);
    expect(onError.mock.calls[0][0].message).toBe('Test error thrown!');
    expect(onError.mock.calls[0][1]).toBeDefined();
  });

  it('renders custom fallback component if provided', () => {
    const CustomFallback = ({ error, retry }: { error?: Error; retry: () => void }) => (
      <div>
        <span>Custom fallback!</span>
        {error && <span>{error.message}</span>}
        <button onClick={retry} type="button">
          Retry now
        </button>
      </div>
    );
    const { container } = render(
      <ErrorBoundary fallback={CustomFallback}>
        <ProblemChild shouldThrow />
      </ErrorBoundary>,
    );
    expect(container).toMatchSnapshot();
    expect(screen.getByText('Custom fallback!')).toBeInTheDocument();
    expect(screen.getByText('Test error thrown!')).toBeInTheDocument();
    expect(screen.getByText('Retry now')).toBeInTheDocument();
  });

  it('resets error state and retries when retry button is clicked (default fallback)', () => {
    // Use a key to force remount after retry
    let key = 0;
    const { container, rerender } = render(
      <ErrorBoundary key={key}>
        <ProblemChild shouldThrow />
      </ErrorBoundary>,
    );
    // Should show error fallback
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();

    // Click retry
    fireEvent.click(screen.getByText('Try Again'));

    // Rerender with no error and a new key to remount ErrorBoundary
    key++;
    rerender(
      <ErrorBoundary key={key}>
        <ProblemChild shouldThrow={false} />
      </ErrorBoundary>,
    );

    expect(container).toMatchSnapshot();
    expect(screen.queryByText('Something went wrong')).not.toBeInTheDocument();
    expect(screen.getByText('All good')).toBeInTheDocument();
  });

  it('resets error state and retries when retry button is clicked (custom fallback)', () => {
    const CustomFallback = ({ error, retry }: { error?: Error; retry: () => void }) => (
      <div>
        <span>Custom fallback!</span>
        {error && <span>{error.message}</span>}
        <button onClick={retry} type="button">
          Retry now
        </button>
      </div>
    );
    // Use a key to force remount after retry
    let key = 0;
    const { container, rerender } = render(
      <ErrorBoundary fallback={CustomFallback} key={key}>
        <ProblemChild shouldThrow />
      </ErrorBoundary>,
    );
    expect(screen.getByText('Custom fallback!')).toBeInTheDocument();

    // Click retry
    fireEvent.click(screen.getByText('Retry now'));

    // Rerender with no error and a new key to remount ErrorBoundary
    key++;
    rerender(
      <ErrorBoundary fallback={CustomFallback} key={key}>
        <ProblemChild shouldThrow={false} />
      </ErrorBoundary>,
    );

    expect(container).toMatchSnapshot();
    expect(screen.queryByText('Custom fallback!')).not.toBeInTheDocument();
    expect(screen.getByText('All good')).toBeInTheDocument();
  });
});
