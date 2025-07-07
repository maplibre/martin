import { fireEvent, render, screen } from "@testing-library/react";
import { SpriteCatalog } from "@/components/catalogs/sprite";
import type { SpriteCollection } from "@/lib/types";

// Mock the SpritePreview component to avoid complex rendering
jest.mock("@/components/sprite/SpritePreview", () => {
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
            <div className="w-7 h-7 bg-gray-200 rounded" data-testid={`sprite-icon-${id}`} key={id}>
              {id}
            </div>
          ))}
        </div>
      );
    },
  };
});

// Mock the dialog components
jest.mock("@/components/dialogs/sprite-preview", () => ({
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

jest.mock("@/components/dialogs/sprite-download", () => ({
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

describe("SpriteCatalog Component", () => {
  const mockSpriteCollections: { [name: string]: SpriteCollection } = {
    "map-icons": {
      images: ["pin", "marker", "building", "park", "poi"],
      lastModifiedAt: new Date("2023-01-10"),
      sizeInBytes: 25000,
    },
    transportation: {
      images: ["car", "bus", "train", "bicycle", "walk", "plane", "ferry"],
      lastModifiedAt: new Date("2023-03-20"),
      sizeInBytes: 30000,
    },
    "ui-elements": {
      images: ["arrow", "plus", "minus", "close", "menu", "search", "filter", "settings"],
      lastModifiedAt: new Date("2023-02-15"),
      sizeInBytes: 35000,
    },
  };

  const defaultProps = {
    error: null,
    isLoading: false,
    isRetrying: false,
    onRetry: jest.fn(),
    onSearchChangeAction: jest.fn(),
    searchQuery: "",
    spriteCollections: mockSpriteCollections,
  };

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it("matches snapshot for loading state", () => {
    const { asFragment } = render(<SpriteCatalog {...defaultProps} isLoading={true} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it("matches snapshot for loaded state with mock data", () => {
    const { asFragment } = render(<SpriteCatalog {...defaultProps} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it("renders loading skeleton when isLoading is true", () => {
    render(<SpriteCatalog {...defaultProps} isLoading={true} />);

    // The loading skeleton should show the title and description
    expect(screen.getByText("Sprite Catalog")).toBeInTheDocument();
    expect(screen.getByText("Preview all available sprite sheets and icons")).toBeInTheDocument();

    // Should not show the search input or actual sprite collections
    expect(screen.queryByText("map-icons")).not.toBeInTheDocument();
    expect(screen.queryByText("transportation")).not.toBeInTheDocument();
    expect(screen.queryByText("ui-elements")).not.toBeInTheDocument();
  });

  it("renders error state when there is an error", () => {
    const error = new Error("Test error");
    render(<SpriteCatalog {...defaultProps} error={error} />);

    // Check for error state elements
    expect(screen.getByText("Failed to Load Sprites")).toBeInTheDocument();
    expect(screen.getByText("Unable to fetch sprite catalog from the server")).toBeInTheDocument();
  });

  it("renders sprite collections correctly", () => {
    render(<SpriteCatalog {...defaultProps} />);

    expect(screen.getByText("Sprite Catalog")).toBeInTheDocument();
    expect(screen.getByText("Preview all available sprite sheets and icons")).toBeInTheDocument();

    // Check that each sprite collection name is displayed
    expect(screen.getByText("map-icons")).toBeInTheDocument();
    expect(screen.getByText("ui-elements")).toBeInTheDocument();
    expect(screen.getByText("transportation")).toBeInTheDocument();

    // Verify image counts are displayed
    expect(screen.getByText("5 total icons")).toBeInTheDocument();
    expect(screen.getByText("8 total icons")).toBeInTheDocument();
    expect(screen.getByText("7 total icons")).toBeInTheDocument();

    // Verify file size labels are displayed (the exact format may vary)
    expect(screen.getAllByText("File Size:")).toHaveLength(3);
  });

  it("filters sprite collections based on search query", () => {
    render(<SpriteCatalog {...defaultProps} searchQuery="transportation" />);

    // Should only show the transportation sprite collection
    expect(screen.queryByText("map-icons")).not.toBeInTheDocument();
    expect(screen.queryByText("ui-elements")).not.toBeInTheDocument();
    expect(screen.getByText("transportation")).toBeInTheDocument();
  });

  it("shows no results message when search has no matches", () => {
    render(<SpriteCatalog {...defaultProps} searchQuery="nonexistent" />);

    expect(
      screen.getByText(/No sprite collections found matching "nonexistent"/i),
    ).toBeInTheDocument();

    // Should not show any sprite collections
    expect(screen.queryByText("map-icons")).not.toBeInTheDocument();
    expect(screen.queryByText("ui-elements")).not.toBeInTheDocument();
    expect(screen.queryByText("transportation")).not.toBeInTheDocument();
  });

  it("filters sprite collections as the user types in the search input", () => {
    const { rerender } = render(<SpriteCatalog {...defaultProps} />);
    const searchInput = screen.getByPlaceholderText("Search sprites...");

    // Simulate typing "ui" into the search box
    fireEvent.change(searchInput, { target: { value: "ui" } });

    // Check that the search handler was called
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith("ui");

    // Rerender with the search query to simulate parent component updating
    rerender(<SpriteCatalog {...defaultProps} searchQuery="ui" />);

    // Verify only the filtered result is present
    expect(screen.getByText("ui-elements")).toBeInTheDocument();
    expect(screen.queryByText("map-icons")).not.toBeInTheDocument();
    expect(screen.queryByText("transportation")).not.toBeInTheDocument();
  });

  it("calls onSearchChangeAction when search input changes", () => {
    render(<SpriteCatalog {...defaultProps} />);
    const searchInput = screen.getByPlaceholderText("Search sprites...");

    fireEvent.change(searchInput, { target: { value: "new search" } });
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith("new search");
  });

  it("renders download and preview buttons for each sprite collection", () => {
    render(<SpriteCatalog {...defaultProps} />);

    // We should have 3 download buttons (one for each sprite collection)
    const downloadButtons = screen.getAllByText("Download");
    expect(downloadButtons.length).toBe(3);

    // We should have 3 preview buttons (one for each sprite collection)
    const previewButtons = screen.getAllByText("Preview");
    expect(previewButtons.length).toBe(3);
  });

  it("has working preview and download buttons", () => {
    render(<SpriteCatalog {...defaultProps} />);

    const previewButtons = screen.getAllByText("Preview");
    const downloadButtons = screen.getAllByText("Download");

    // Check that buttons exist and are clickable
    expect(previewButtons.length).toBe(3);
    expect(downloadButtons.length).toBe(3);

    // Buttons should be enabled and clickable
    expect(previewButtons[0]).toBeEnabled();
    expect(downloadButtons[0]).toBeEnabled();
  });

  it("shows sprite preview sections for each collection", () => {
    render(<SpriteCatalog {...defaultProps} />);

    // Check that "Icon Preview:" labels are rendered for each collection
    expect(screen.getAllByText("Icon Preview:")).toHaveLength(3);

    // Check that preview sections exist (the actual sprite rendering is complex and tested separately)
    const previewSections = screen.getAllByText("Icon Preview:");
    expect(previewSections).toHaveLength(3);
  });

  it("shows empty state with configuration link when no collections", () => {
    render(<SpriteCatalog {...defaultProps} spriteCollections={{}} />);

    expect(screen.getByText("No sprite collections found.")).toBeInTheDocument();
    expect(screen.getByText("Learn how to configure Sprites")).toBeInTheDocument();

    const configLink = screen.getByRole("link", { name: "Learn how to configure Sprites" });
    expect(configLink).toHaveAttribute("href", "https://maplibre.org/martin/sources-sprites.html");
    expect(configLink).toHaveAttribute("target", "_blank");
  });

  it("shows search-specific empty state when search has no matches", () => {
    render(<SpriteCatalog {...defaultProps} searchQuery="nonexistent" />);

    expect(
      screen.getByText(/No sprite collections found matching "nonexistent"/i),
    ).toBeInTheDocument();
    expect(screen.getByText("Learn how to configure Sprites")).toBeInTheDocument();
  });
});
