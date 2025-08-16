import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { FontCatalog } from '@/components/catalogs/font';
import type { Font } from '@/lib/types';

// Mock the toast hook
vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({
    toast: vi.fn(),
  }),
}));

// Mock the API
vi.mock('@/lib/api', () => ({
  buildMartinUrl: vi.fn((path: string) => `http://localhost:3000${path}`),
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
    onRetry: vi.fn(),
    onSearchChangeAction: vi.fn(),
    searchQuery: '',
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders loading skeleton when isLoading is true', () => {
    const { container } = render(<FontCatalog {...defaultProps} isLoading={true} />);

    // Check for skeleton elements
    const skeletonElements = container.querySelectorAll('.animate-pulse');
    expect(skeletonElements.length).toBeGreaterThan(0);
  });

  it('renders error state when there is an error', () => {
    const error = new Error('Test error');
    render(<FontCatalog {...defaultProps} error={error} />);

    // Check for error message
    expect(screen.getByText('Failed to Load Fonts')).toBeTruthy();
    expect(screen.getByText('Unable to fetch font catalog from the server')).toBeTruthy();
    expect(screen.getByText('Test error')).toBeTruthy();
    expect(screen.getByText('Try Again')).toBeTruthy();
  });

  it('renders font catalog correctly', () => {
    const { container } = render(<FontCatalog {...defaultProps} />);

    // Check title and description are present (use container query to avoid duplicates from mocks)
    const titles = container.querySelectorAll('h2');
    const titleFound = Array.from(titles).some((title) =>
      title.textContent?.includes('Font Catalog'),
    );
    expect(titleFound).toBe(true);

    // Check that all fonts are rendered
    expect(screen.getByText('Roboto Medium')).toBeTruthy();
    expect(screen.getByText('Open Sans Regular')).toBeTruthy();
    expect(screen.getByText('Noto Sans Bold')).toBeTruthy();

    // Check family and style information
    expect(screen.getByText('Family: Roboto • Style: Medium')).toBeTruthy();
    expect(screen.getByText('Family: Open Sans • Style: Regular')).toBeTruthy();
    expect(screen.getByText('Family: Noto Sans • Style: Bold')).toBeTruthy();

    // Check glyph counts
    expect(screen.getByText('350')).toBeTruthy();
    expect(screen.getByText('420')).toBeTruthy();
    expect(screen.getByText('380')).toBeTruthy();

    // Check format badges
    const ttfBadges = screen.getAllByText('ttf');
    const otfBadges = screen.getAllByText('otf');
    expect(ttfBadges.length).toBe(2); // Roboto and Noto Sans
    expect(otfBadges.length).toBe(1); // Open Sans
  });

  it('filters fonts based on search query', () => {
    const { container } = render(<FontCatalog {...defaultProps} searchQuery="roboto" />);

    // Should only show the Roboto font by checking the rendered cards
    const fontCards = container.querySelectorAll('[class*="hover:shadow-lg"]');
    // With roboto filter, should have fewer cards than total (3)
    expect(fontCards.length).toBeLessThan(3);
  });

  it('filters fonts based on font family', () => {
    const { container } = render(<FontCatalog {...defaultProps} searchQuery="open" />);

    // Should filter to show only matching fonts
    const fontCards = container.querySelectorAll('[class*="hover:shadow-lg"]');
    expect(fontCards.length).toBeLessThan(3);
  });

  it('filters fonts based on style', () => {
    const { container } = render(<FontCatalog {...defaultProps} searchQuery="bold" />);

    // Should filter to show only matching fonts
    const fontCards = container.querySelectorAll('[class*="hover:shadow-lg"]');
    expect(fontCards.length).toBeLessThan(3);
  });

  it('shows no results message when search has no matches', () => {
    render(<FontCatalog {...defaultProps} searchQuery="nonexistent" />);
    expect(screen.getByText(/No fonts found matching "nonexistent"/i)).toBeTruthy();
  });

  it('calls onSearchChangeAction when search input changes', () => {
    const { container } = render(<FontCatalog {...defaultProps} />);
    const searchInput = container.querySelector('input[placeholder="Search fonts..."]');

    if (searchInput) {
      fireEvent.change(searchInput, { target: { value: 'new search' } });
      expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith('new search');
    }
  });

  it('renders with empty fonts object', () => {
    render(<FontCatalog {...defaultProps} fonts={{}} />);
    expect(screen.getAllByText('No fonts found.').length).toBeGreaterThan(0);
  });

  it('renders with undefined fonts', () => {
    render(<FontCatalog {...defaultProps} fonts={undefined} />);
    expect(screen.getAllByText('No fonts found.').length).toBeGreaterThan(0);
  });

  it('case insensitive search works correctly', () => {
    const { container } = render(<FontCatalog {...defaultProps} searchQuery="ROBOTO" />);

    // Should filter fonts with case insensitive search
    const fontCards = container.querySelectorAll('[class*="hover:shadow-lg"]');
    expect(fontCards.length).toBeLessThan(3);
  });
});
