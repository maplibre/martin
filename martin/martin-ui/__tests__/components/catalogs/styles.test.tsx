import { cleanup, fireEvent, render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { StylesCatalog } from '@/components/catalogs/styles';
import type { ErrorStateProps } from '@/components/error/error-state';
import type { CatalogSkeletonProps } from '@/components/loading/catalog-skeleton';
import type { Style } from '@/lib/types';

interface MockComponentProps {
  children?: ReactNode;
  [key: string]: unknown;
}

interface MockTooltipProps {
  children?: ReactNode;
}

// Mock UI components
vi.mock('@/components/ui/badge', () => ({
  Badge: ({ children, ...props }: MockComponentProps) => <span {...props}>{children}</span>,
}));

vi.mock('@/components/ui/button', () => ({
  Button: ({ children, asChild, ...props }: MockComponentProps) =>
    asChild ? <a {...props}>{children}</a> : <button {...props}>{children}</button>,
}));

vi.mock('@/components/ui/card', () => ({
  Card: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardContent: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardDescription: ({ children, ...props }: MockComponentProps) => <p {...props}>{children}</p>,
  CardHeader: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardTitle: ({ children, ...props }: MockComponentProps) => <h3 {...props}>{children}</h3>,
}));

vi.mock('@/components/ui/input', () => ({
  Input: ({ ...props }: MockComponentProps) => <input {...props} />,
}));

vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: MockTooltipProps) => children,
  TooltipContent: ({ children }: MockTooltipProps) => <div>{children}</div>,
  TooltipTrigger: ({ children }: MockTooltipProps) => children,
}));

vi.mock('@/components/ui/disabled-non-interactive-button', () => ({
  DisabledNonInteractiveButton: ({ children, ...props }: MockComponentProps) => (
    <button {...props}>{children}</button>
  ),
}));

vi.mock('@/lib/api', () => ({
  buildMartinUrl: vi.fn((path: string) => `http://localhost:3000${path}`),
}));

// Mock error and loading components
vi.mock('@/components/error/error-state', () => ({
  ErrorState: ({ title, description, error, onRetry }: ErrorStateProps) => (
    <div>
      <h2>{title}</h2>
      <p>{description}</p>
      <p>{typeof error === 'string' ? error : error?.message}</p>
      <button onClick={onRetry} type="button">
        Try Again
      </button>
    </div>
  ),
}));

vi.mock('@/components/loading/catalog-skeleton', () => ({
  CatalogSkeleton: ({ title, description }: CatalogSkeletonProps) => (
    <div>
      <h2>{title}</h2>
      <p>{description}</p>
      <div className="animate-pulse">Loading...</div>
    </div>
  ),
}));

// Mock lucide-react icons
vi.mock('lucide-react', () => ({
  Brush: () => (
    <svg aria-label="Brush icon" data-testid="brush-icon" role="img">
      🎨
    </svg>
  ),
  Code: () => (
    <svg aria-label="Code icon" data-testid="code-icon" role="img">
      📄
    </svg>
  ),
  Eye: () => (
    <svg aria-label="Eye icon" data-testid="eye-icon" role="img">
      👁
    </svg>
  ),
  Search: () => (
    <svg aria-label="Search icon" data-testid="search-icon" role="img">
      🔍
    </svg>
  ),
  SquarePen: () => (
    <svg aria-label="Square pen icon" data-testid="squarepen-icon" role="img">
      ✏️
    </svg>
  ),
}));

describe('StylesCatalog Component', () => {
  const mockStyles: { [name: string]: Style } = {
    'Basic Style': {
      colors: ['#FF5733', '#33FF57', '#3357FF', '#F3FF33'],
      lastModifiedAt: new Date('2023-01-15'),
      layerCount: 10,
      path: '/styles/basic.json',
      type: 'vector',
    },
    'Hybrid Style': {
      lastModifiedAt: new Date('2023-03-25'),
      layerCount: 15,
      path: '/styles/hybrid.json',
      type: 'hybrid',
      versionHash: 'abc123',
    },
    'Satellite Style': {
      path: '/styles/satellite.json',
    },
  };

  const defaultProps = {
    error: null,
    isLoading: false,
    isRetrying: false,
    onRetry: vi.fn(),
    onSearchChangeAction: vi.fn(),
    searchQuery: '',
    styles: mockStyles,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it('renders loading skeleton when isLoading is true', () => {
    const { container } = render(<StylesCatalog {...defaultProps} isLoading={true} />);

    expect(container.textContent).toContain('Styles Catalog');
    expect(container.textContent).toContain('Preview all available map styles and themes');

    // Check for skeleton loading elements (they have animate-pulse class)
    const skeletonElements = container.querySelectorAll('.animate-pulse');
    expect(skeletonElements.length).toBeGreaterThan(0);
  });

  it('renders error state when there is an error', () => {
    const error = new Error('Test error');
    const { container } = render(<StylesCatalog {...defaultProps} error={error} />);

    expect(container.textContent).toContain('Failed to Load Styles');
    expect(container.textContent).toContain('Unable to fetch style catalog from the server');
    expect(container.textContent).toContain('Test error');
    expect(container.textContent).toContain('Try Again');
  });

  it('renders styles catalog correctly', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    expect(container.textContent).toContain('Styles Catalog');
    expect(container.textContent).toContain(
      'Browse and preview all available map styles and themes',
    );

    // Check that each style name is displayed
    expect(container.textContent).toContain('Basic Style');
    expect(container.textContent).toContain('Satellite Style');
    expect(container.textContent).toContain('Hybrid Style');

    // Verify paths are displayed
    expect(container.textContent).toContain('/styles/basic.json');
    expect(container.textContent).toContain('/styles/satellite.json');
    expect(container.textContent).toContain('/styles/hybrid.json');

    // Verify type badges are displayed
    expect(container.textContent).toContain('vector');
    expect(container.textContent).toContain('hybrid');

    // Verify layer counts are displayed
    expect(container.textContent).toContain('10');
    expect(container.textContent).toContain('15');

    // Verify version hashes are displayed
    expect(container.textContent).toContain('abc123');
  });

  it('filters styles based on search query - by name', () => {
    const { container } = render(<StylesCatalog {...defaultProps} searchQuery="basic" />);

    // Should only show the Basic Style
    expect(container.textContent).toContain('Basic Style');
    expect(container.textContent).not.toContain('Satellite Style');
    expect(container.textContent).not.toContain('Hybrid Style');
  });

  it('filters styles based on search query - by path', () => {
    const { container } = render(<StylesCatalog {...defaultProps} searchQuery="satellite.json" />);

    // Should only show the Satellite Style
    expect(container.textContent).not.toContain('Basic Style');
    expect(container.textContent).toContain('Satellite Style');
    expect(container.textContent).not.toContain('Hybrid Style');
  });

  it('filters styles based on search query - by type', () => {
    const { container } = render(<StylesCatalog {...defaultProps} searchQuery="hybrid" />);

    // Should only show the Hybrid Style
    expect(container.textContent).not.toContain('Basic Style');
    expect(container.textContent).not.toContain('Satellite Style');
    expect(container.textContent).toContain('Hybrid Style');
  });

  it('shows no results message when search has no matches', () => {
    const { container } = render(<StylesCatalog {...defaultProps} searchQuery="nonexistent" />);

    expect(container.textContent).toMatch(/No styles found matching "nonexistent"/i);
    expect(container.textContent).toContain('Learn how to configure Styles');

    // Should not render any style names
    expect(container.textContent).not.toContain('Basic Style');
    expect(container.textContent).not.toContain('Satellite Style');
    expect(container.textContent).not.toContain('Hybrid Style');
  });

  it('shows no results message when no styles provided', () => {
    const { container } = render(<StylesCatalog {...defaultProps} styles={{}} />);

    expect(container.textContent).toContain('No styles found.');
    expect(container.textContent).toContain('Learn how to configure Styles');
  });

  it('shows no results message when styles is undefined', () => {
    const { container } = render(<StylesCatalog {...defaultProps} styles={undefined} />);

    expect(container.textContent).toContain('No styles found.');
    expect(container.textContent).toContain('Learn how to configure Styles');
  });

  it('calls onSearchChangeAction when search input changes', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);
    const searchInput = container.querySelector('input[placeholder="Search styles..."]');
    if (!searchInput) {
      throw new Error('Search input not found');
    }
    fireEvent.change(searchInput, { target: { value: 'new search' } });
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith('new search');
  });

  it('calls onRetry when retry button is clicked in error state', () => {
    const mockOnRetry = vi.fn();
    const error = new Error('Test error');

    const { container } = render(
      <StylesCatalog {...defaultProps} error={error} onRetry={mockOnRetry} />,
    );

    const retryButton = Array.from(container.querySelectorAll('button')).find((btn) =>
      btn.textContent?.includes('Try Again'),
    );
    if (!retryButton) {
      throw new Error('Search input not found');
    }
    fireEvent.click(retryButton);

    expect(mockOnRetry).toHaveBeenCalledTimes(1);
  });

  it('displays color palettes when available', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    // Only Basic Style has colors defined
    expect(container.textContent).toContain('Color Palette:');

    // Check that color swatches are rendered
    const colorSwatches = container.querySelectorAll('[style*="background-color"]');
    expect(colorSwatches.length).toBeGreaterThan(0);
  });

  it('renders edit and integration guide buttons for each style', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    // We should have 3 edit buttons (one for each style)
    const editButtons = Array.from(container.querySelectorAll('button')).filter((btn) =>
      btn.textContent?.includes('Edit'),
    );
    expect(editButtons.length).toBe(3);

    // We should have 3 integration guide buttons (one for each style)
    const integrationButtons = Array.from(container.querySelectorAll('button')).filter((btn) =>
      btn.textContent?.includes('Integration Guide'),
    );
    expect(integrationButtons.length).toBe(3);
  });

  it('renders integration guide buttons', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    const integrationButtons = Array.from(container.querySelectorAll('button')).filter((btn) =>
      btn.textContent?.includes('Integration Guide'),
    );
    expect(integrationButtons.length).toBe(3);
  });

  it('displays search input with correct placeholder', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    const searchInput = container.querySelector(
      'input[placeholder="Search styles..."]',
    ) as HTMLInputElement;
    expect(searchInput).toBeTruthy();
    expect(searchInput.value).toBe('');
  });

  it('displays search input with correct value', () => {
    const { container } = render(<StylesCatalog {...defaultProps} searchQuery="test query" />);

    const searchInput = container.querySelector(
      'input[placeholder="Search styles..."]',
    ) as HTMLInputElement;
    expect(searchInput.value).toBe('test query');
  });

  it('case-insensitive search works correctly', () => {
    const { container } = render(<StylesCatalog {...defaultProps} searchQuery="BASIC" />);

    expect(container.textContent).toContain('Basic Style');
    expect(container.textContent).not.toContain('Satellite Style');
    expect(container.textContent).not.toContain('Hybrid Style');
  });

  it('displays last modified dates when available', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    // Check that dates are formatted and displayed - expect locale string format
    const modifiedMatches = container.textContent?.match(/Modified:/g) || [];
    expect(modifiedMatches.length).toBe(2); // Only Basic Style and Hybrid Style have dates
  });

  it('renders correct icons for different style types', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    // Check for SVG elements - different style types should have different icons
    const svgElements = container.querySelectorAll('svg');

    // Should have at least 4 SVGs: search icon + 3 style type icons
    expect(svgElements.length).toBeGreaterThan(3);
  });

  it('shows layer count for styles that have it', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    // Check that layer count labels are present
    const layerMatches = container.textContent?.match(/Layers:/g) || [];
    expect(layerMatches.length).toBe(2); // Only Basic Style and Hybrid Style have layer counts

    expect(container.textContent).toContain('10'); // Basic Style layerCount
    expect(container.textContent).toContain('15'); // Hybrid Style layerCount
  });

  it('shows version hash for styles that have it', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    // Check that version hash label is present
    expect(container.textContent).toContain('Version:');
    expect(container.textContent).toContain('abc123'); // Hybrid Style versionHash
  });

  it('renders MapLibre map components for each style', () => {
    const { container } = render(<StylesCatalog {...defaultProps} />);

    // Check that MapLibre maps are rendered for each style
    const mapElements = container.querySelectorAll('[data-testid="maplibre-map"]');
    expect(mapElements.length).toBe(3);
  });
});
