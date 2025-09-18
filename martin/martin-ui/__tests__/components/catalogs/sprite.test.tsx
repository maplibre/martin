import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SpriteCatalog } from '@/components/catalogs/sprite';
import type { SpriteCollection } from '@/lib/types';

// Mock the SpritePreview component to avoid complex rendering
vi.mock('@/components/sprite/SpritePreview', async () => {
  return {
    __esModule: true,
    default: function MockSpritePreview({
      spriteIds,
      className,
    }: {
      spriteIds: string[];
      className?: string;
    }) {
      return (
        <div className={className} data-testid="sprite-preview">
          {spriteIds.map((id) => (
            <div
              className="w-7 h-7 bg-gray-200 rounded-sm"
              data-testid={`sprite-icon-${id}`}
              key={id}
            >
              {id}
            </div>
          ))}
        </div>
      );
    },
  };
});

// Mock the dialog components
vi.mock('@/components/dialogs/sprite-preview', () => ({
  SpritePreviewDialog: ({
    name,
    onCloseAction,
    onDownloadAction,
  }: {
    name: string;
    sprite: SpriteCollection;
    onCloseAction: () => void;
    onDownloadAction: () => void;
  }) => (
    <div data-testid="sprite-preview-dialog">
      <div data-testid="sprite-preview-name">{name}</div>
      <button data-testid="sprite-preview-close" onClick={onCloseAction} type="button">
        Close
      </button>
      <button data-testid="sprite-preview-download" onClick={onDownloadAction} type="button">
        Download
      </button>
    </div>
  ),
}));

vi.mock('@/components/dialogs/sprite-download', () => ({
  SpriteDownloadDialog: ({
    name,
    onCloseAction,
  }: {
    name: string;
    sprite: SpriteCollection;
    onCloseAction: () => void;
  }) => (
    <div data-testid="sprite-download-dialog">
      <div data-testid="sprite-download-name">{name}</div>
      <button data-testid="sprite-download-close" onClick={onCloseAction} type="button">
        Close
      </button>
    </div>
  ),
}));

describe('SpriteCatalog Component', () => {
  const mockSpriteCollections: { [name: string]: SpriteCollection } = {
    'map-icons': {
      images: ['pin', 'marker', 'building', 'park', 'poi'],
      lastModifiedAt: new Date('2023-01-10'),
      sizeInBytes: 25000,
    },
    transportation: {
      images: ['car', 'bus', 'train', 'bicycle', 'walk', 'plane', 'ferry'],
      lastModifiedAt: new Date('2023-03-20'),
      sizeInBytes: 30000,
    },
    'ui-elements': {
      images: ['arrow', 'plus', 'minus', 'close', 'menu', 'search', 'filter', 'settings'],
      lastModifiedAt: new Date('2023-02-15'),
      sizeInBytes: 35000,
    },
  };

  const defaultProps = {
    error: null,
    isLoading: false,
    isRetrying: false,
    onRetry: vi.fn(),
    onSearchChangeAction: vi.fn(),
    searchQuery: '',
    spriteCollections: mockSpriteCollections,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it('matches snapshot for loading state', () => {
    const { asFragment } = render(<SpriteCatalog {...defaultProps} isLoading={true} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it('matches snapshot for loaded state with mock data', () => {
    const { asFragment } = render(<SpriteCatalog {...defaultProps} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it('renders loading skeleton when isLoading is true', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} isLoading={true} />);

    // The loading skeleton should show the title and description
    expect(container.textContent).toContain('Sprite Catalog');
    expect(container.textContent).toContain('Preview all available sprite sheets and icons');

    // Should not show the search input or actual sprite collections
    expect(container.textContent).not.toContain('map-icons');
    expect(container.textContent).not.toContain('transportation');
    expect(container.textContent).not.toContain('ui-elements');
  });

  it('renders error state when there is an error', () => {
    const error = new Error('Test error');
    const { container } = render(<SpriteCatalog {...defaultProps} error={error} />);

    // Check for error state elements
    expect(container.textContent).toContain('Failed to Load Sprites');
    expect(container.textContent).toContain('Unable to fetch sprite catalog from the server');
  });

  it('renders sprite collections correctly', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} />);

    expect(container.textContent).toContain('Sprite Catalog');
    expect(container.textContent).toContain('Preview all available sprite sheets and icons');

    // Check that each sprite collection name is displayed
    expect(container.textContent).toContain('map-icons');
    expect(container.textContent).toContain('ui-elements');
    expect(container.textContent).toContain('transportation');

    // Verify image counts are displayed
    expect(container.textContent).toContain('5 total icons');
    expect(container.textContent).toContain('8 total icons');
    expect(container.textContent).toContain('7 total icons');

    // Verify file size labels are displayed (the exact format may vary)
    expect(container.textContent).toContain('File Size:');
  });

  it('filters sprite collections based on search query', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} searchQuery="transportation" />);

    // Should only show the transportation sprite collection
    expect(container.textContent).not.toContain('map-icons');
    expect(container.textContent).not.toContain('ui-elements');
    expect(container.textContent).toContain('transportation');
  });

  it('shows no results message when search has no matches', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} searchQuery="nonexistent" />);

    expect(container.textContent).toMatch(/No sprite collections found matching "nonexistent"/i);

    // Should not show any sprite collections
    expect(container.textContent).not.toContain('map-icons');
    expect(container.textContent).not.toContain('ui-elements');
    expect(container.textContent).not.toContain('transportation');
  });

  it('filters sprite collections as the user types in the search input', () => {
    const { rerender, container } = render(<SpriteCatalog {...defaultProps} />);
    const searchInput = container.querySelector('input[placeholder="Search sprites..."]');
    if (!searchInput) {
      throw new Error('Search input not found');
    }

    // Simulate typing "ui" into the search box
    fireEvent.change(searchInput, { target: { value: 'ui' } });

    // Check that the search handler was called
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith('ui');

    // Rerender with the search query to simulate parent component updating
    rerender(<SpriteCatalog {...defaultProps} searchQuery="ui" />);

    // Verify only the filtered result is present
    expect(container.textContent).toContain('ui-elements');
    expect(container.textContent).not.toContain('map-icons');
    expect(container.textContent).not.toContain('transportation');
  });

  it('calls onSearchChangeAction when search input changes', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} />);
    const searchInput = container.querySelector('input[placeholder="Search sprites..."]');
    if (!searchInput) {
      throw new Error('Search input not found');
    }

    fireEvent.change(searchInput, { target: { value: 'new search' } });
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith('new search');
  });

  it('renders download and preview buttons for each sprite collection', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} />);

    // We should have download and preview buttons
    const buttons = container.querySelectorAll('button');
    const downloadButtons = Array.from(buttons).filter((btn) =>
      btn.textContent?.includes('Download'),
    );
    const previewButtons = Array.from(buttons).filter((btn) =>
      btn.textContent?.includes('Preview'),
    );

    expect(downloadButtons.length).toBe(3);
    expect(previewButtons.length).toBe(3);
  });

  it('has working preview and download buttons', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} />);

    const buttons = container.querySelectorAll('button');
    const previewButtons = Array.from(buttons).filter((btn) =>
      btn.textContent?.includes('Preview'),
    );
    const downloadButtons = Array.from(buttons).filter((btn) =>
      btn.textContent?.includes('Download'),
    );

    // Check that buttons exist and are clickable
    expect(previewButtons.length).toBe(3);
    expect(downloadButtons.length).toBe(3);

    // Buttons should be enabled and clickable
    expect(previewButtons[0]?.disabled).toBeFalsy();
    expect(downloadButtons[0]?.disabled).toBeFalsy();
  });

  it('shows sprite preview sections for each collection', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} />);

    // Check that "Icon Preview:" labels are rendered for each collection
    const iconPreviewMatches = container.textContent?.match(/Icon Preview:/g) || [];
    expect(iconPreviewMatches.length).toBe(3);
  });

  it('shows empty state with configuration link when no collections', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} spriteCollections={{}} />);

    expect(container.textContent).toContain('No sprite collections found.');
    expect(container.textContent).toContain('Learn how to configure Sprites');

    const configLink = container.querySelector(
      'a[href="https://maplibre.org/martin/sources-sprites.html"]',
    );
    expect(configLink).toBeTruthy();
    expect(configLink?.getAttribute('target')).toBe('_blank');
  });

  it('shows search-specific empty state when search has no matches', () => {
    const { container } = render(<SpriteCatalog {...defaultProps} searchQuery="nonexistent" />);

    expect(container.textContent).toMatch(/No sprite collections found matching "nonexistent"/i);
    expect(container.textContent).toContain('Learn how to configure Sprites');
  });
});
