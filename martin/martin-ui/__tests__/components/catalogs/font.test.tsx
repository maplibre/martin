import { fireEvent, render, screen } from '@testing-library/react';
import type React from 'react';
import { FontCatalog } from '@/components/catalogs/font';
import type { Font } from '@/lib/types';

// Create a test wrapper component that provides TooltipProvider
const TestWrapper = ({ children }: { children: React.ReactNode }) => {
  const TooltipProvider = ({ children }: { children: React.ReactNode }) => <div>{children}</div>;
  return <TooltipProvider>{children}</TooltipProvider>;
};

// Mock all UI components
jest.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  TooltipContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  TooltipProvider: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

jest.mock('@/components/ui/button', () => ({
  Button: ({ children, ...props }: React.ComponentProps<'button'>) => (
    <button {...props}>{children}</button>
  ),
}));

jest.mock('@/components/ui/copy-link-button', () => ({
  CopyLinkButton: ({ children, ...props }: React.ComponentProps<'button'>) => (
    <button data-testid="copy-link-button" {...props}>
      {children ?? 'Copy Link'}
    </button>
  ),
}));

jest.mock('@/components/ui/badge', () => ({
  Badge: ({ children, ...props }: React.ComponentProps<'span'>) => (
    <span data-slot="badge" {...props}>
      {children}
    </span>
  ),
}));

jest.mock('@/components/ui/input', () => ({
  Input: (props: React.ComponentProps<'input'>) => <input {...props} />,
}));

jest.mock('@/components/ui/card', () => ({
  Card: ({ children, ...props }: React.ComponentProps<'div'>) => <div {...props}>{children}</div>,
  CardContent: ({ children, ...props }: React.ComponentProps<'div'>) => (
    <div data-testid="card-content" {...props}>
      {children}
    </div>
  ),
  CardDescription: ({ children, ...props }: React.ComponentProps<'div'>) => (
    <div data-testid="card-description" {...props}>
      {children}
    </div>
  ),
  CardHeader: ({ children, ...props }: React.ComponentProps<'div'>) => (
    <div data-testid="card-header" {...props}>
      {children}
    </div>
  ),
  CardTitle: ({ children, ...props }: React.ComponentProps<'div'>) => (
    <div data-testid="card-title" {...props}>
      {children}
    </div>
  ),
}));

jest.mock('@/components/ui/disabledNonInteractiveButton', () => ({
  DisabledNonInteractiveButton: ({ children, ...props }: React.ComponentProps<'button'>) => (
    <button {...props} disabled>
      {children}
    </button>
  ),
}));

jest.mock('@/components/ui/skeleton', () => ({
  Skeleton: ({ className, ...props }: React.ComponentProps<'div'>) => (
    <div className={className} data-testid="skeleton" {...props} />
  ),
}));

jest.mock('@/components/loading/catalog-skeleton', () => ({
  CatalogSkeleton: ({ title, description }: { title: string; description: string }) => (
    <div data-testid="catalog-skeleton">
      <div data-testid="skeleton-title">{title}</div>
      <div data-testid="skeleton-description">{description}</div>
    </div>
  ),
}));

jest.mock('@/components/error/error-state', () => ({
  ErrorState: ({ title, description }: { title: string; description: string }) => (
    <div data-testid="error-state">
      <div data-testid="error-title">{title}</div>
      <div data-testid="error-description">{description}</div>
    </div>
  ),
}));

jest.mock('lucide-react', () => ({
  Download: () => <div data-testid="download-icon">Download</div>,
  Eye: () => <div data-testid="eye-icon">Eye</div>,
  Search: () => <div data-testid="search-icon">Search</div>,
  Type: () => <div data-testid="type-icon">Type</div>,
}));

// Mock the toast hook
jest.mock('@/hooks/use-toast', () => ({
  useToast: () => ({
    toast: jest.fn(),
  }),
}));

// Mock the API
jest.mock('@/lib/api', () => ({
  buildMartinUrl: jest.fn((path: string) => `http://localhost:3000${path}`),
}));

describe('FontCatalog Component', () => {
  const mockFonts: { [name: string]: Font } = {
    'Noto Sans Bold': {
      end: 255,
      family: 'Noto Sans',
      format: 'ttf',
      glyphs: 380,
      lastModifiedAt: new Date('2023-03-20'),
      start: 0,
      style: 'Bold',
    },
    'Open Sans Regular': {
      end: 255,
      family: 'Open Sans',
      format: 'otf',
      glyphs: 420,
      lastModifiedAt: new Date('2023-02-15'),
      start: 0,
      style: 'Regular',
    },
    'Roboto Medium': {
      end: 255,
      family: 'Roboto',
      format: 'ttf',
      glyphs: 350,
      lastModifiedAt: new Date('2023-01-01'),
      start: 0,
      style: 'Medium',
    },
  };

  const defaultProps = {
    error: null,
    fonts: mockFonts,
    isLoading: false,
    isRetrying: false,
    onRetry: jest.fn(),
    onSearchChangeAction: jest.fn(),
    searchQuery: '',
  };

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders loading skeleton when isLoading is true', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} isLoading={true} />
      </TestWrapper>,
    );
    expect(screen.getByText('Font Catalog')).toBeInTheDocument();
    expect(screen.getByText('Preview all available font assets')).toBeInTheDocument();
    // Check that skeleton elements are rendered
    expect(document.querySelectorAll('.animate-pulse').length).toBeGreaterThan(0); // Multiple skeleton elements
  });

  it('renders error state when there is an error', () => {
    const error = new Error('Test error');
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} error={error} />
      </TestWrapper>,
    );
    expect(screen.getByText('Failed to Load Fonts')).toBeInTheDocument();
    expect(screen.getByText('Unable to fetch font catalog from the server')).toBeInTheDocument();
    expect(screen.getByText('Test error')).toBeInTheDocument();
    expect(screen.getByText('Try Again')).toBeInTheDocument();
  });

  it('renders font catalog correctly', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} />
      </TestWrapper>,
    );

    // Check title and description
    expect(screen.getByText('Font Catalog')).toBeInTheDocument();
    expect(screen.getByText('Preview all available font assets')).toBeInTheDocument();

    // Check that all fonts are rendered
    expect(screen.getByText('Roboto Medium')).toBeInTheDocument();
    expect(screen.getByText('Open Sans Regular')).toBeInTheDocument();
    expect(screen.getByText('Noto Sans Bold')).toBeInTheDocument();

    // Check family and style information
    expect(screen.getByText('Family: Roboto • Style: Medium')).toBeInTheDocument();
    expect(screen.getByText('Family: Open Sans • Style: Regular')).toBeInTheDocument();
    expect(screen.getByText('Family: Noto Sans • Style: Bold')).toBeInTheDocument();

    // Check glyph counts
    expect(screen.getByText('350')).toBeInTheDocument();
    expect(screen.getByText('420')).toBeInTheDocument();
    expect(screen.getByText('380')).toBeInTheDocument();

    // Check format badges - badges should be rendered with uppercase format
    const badges = document.querySelectorAll('[data-slot="badge"]');
    expect(badges).toHaveLength(3);
    expect(badges[0]).toHaveTextContent('ttf');
    expect(badges[1]).toHaveTextContent('otf');
    expect(badges[2]).toHaveTextContent('ttf');
  });

  it('filters fonts based on search query', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} searchQuery="roboto" />
      </TestWrapper>,
    );

    // Should only show the Roboto font
    expect(screen.getByText('Roboto Medium')).toBeInTheDocument();
    expect(screen.queryByText('Open Sans Regular')).not.toBeInTheDocument();
    expect(screen.queryByText('Noto Sans Bold')).not.toBeInTheDocument();

    // Should have only one badge
    const badges = document.querySelectorAll('[data-slot="badge"]');
    expect(badges).toHaveLength(1);
  });

  it('filters fonts based on font family', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} searchQuery="open" />
      </TestWrapper>,
    );

    // Should only show the Open Sans font
    expect(screen.queryByText('Roboto Medium')).not.toBeInTheDocument();
    expect(screen.getByText('Open Sans Regular')).toBeInTheDocument();
    expect(screen.queryByText('Noto Sans Bold')).not.toBeInTheDocument();
  });

  it('filters fonts based on style', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} searchQuery="bold" />
      </TestWrapper>,
    );

    // Should only show the Noto Sans Bold font
    expect(screen.queryByText('Roboto Medium')).not.toBeInTheDocument();
    expect(screen.queryByText('Open Sans Regular')).not.toBeInTheDocument();
    expect(screen.getByText('Noto Sans Bold')).toBeInTheDocument();
  });

  it('shows no results message when search has no matches', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} searchQuery="nonexistent" />
      </TestWrapper>,
    );
    expect(screen.getByText(/No fonts found matching "nonexistent"/i)).toBeInTheDocument();

    // Should not render any badges
    const badges = document.querySelectorAll('[data-slot="badge"]');
    expect(badges).toHaveLength(0);
  });

  it('calls onSearchChangeAction when search input changes', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} />
      </TestWrapper>,
    );
    const searchInput = screen.getByPlaceholderText('Search fonts...');

    fireEvent.change(searchInput, { target: { value: 'new search' } });
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith('new search');
  });

  it('renders copy link buttons for each font', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} />
      </TestWrapper>,
    );

    // Should have 3 copy link buttons (one for each font) - they render as buttons with specific classes
    const copyLinkButtons = document.querySelectorAll('button[class*="bg-transparent"]');
    expect(copyLinkButtons).toHaveLength(3);
  });

  it('renders details buttons for each font', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} />
      </TestWrapper>,
    );

    // Should have 3 details buttons (one for each font)
    const detailsButtons = screen.getAllByText('Details');
    expect(detailsButtons).toHaveLength(3);
  });

  it('renders with empty fonts object', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} fonts={{}} />
      </TestWrapper>,
    );

    expect(screen.getByText('Font Catalog')).toBeInTheDocument();
    expect(screen.getByText('No fonts found.')).toBeInTheDocument();
  });

  it('renders with undefined fonts', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} fonts={undefined} />
      </TestWrapper>,
    );

    expect(screen.getByText('Font Catalog')).toBeInTheDocument();
    expect(screen.getByText('No fonts found.')).toBeInTheDocument();
  });

  it('case insensitive search works correctly', () => {
    render(
      <TestWrapper>
        <FontCatalog {...defaultProps} searchQuery="ROBOTO" />
      </TestWrapper>,
    );

    // Should still show the Roboto font despite case difference
    expect(screen.getByText('Roboto Medium')).toBeInTheDocument();
    expect(screen.queryByText('Open Sans Regular')).not.toBeInTheDocument();
    expect(screen.queryByText('Noto Sans Bold')).not.toBeInTheDocument();
  });
});
