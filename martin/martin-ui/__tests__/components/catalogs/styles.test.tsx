import { fireEvent, render, screen } from "@testing-library/react";
import { StylesCatalog } from "@/components/catalogs/styles";
import type { Style } from "@/lib/types";

describe("StylesCatalog Component", () => {
  const mockStyles: { [name: string]: Style } = {
    "Basic Style": {
      colors: ["#FF5733", "#33FF57", "#3357FF", "#F3FF33"],
      lastModifiedAt: new Date("2023-01-15"),
      layerCount: 10,
      path: "/styles/basic.json",
      type: "vector",
    },
    "Hybrid Style": {
      lastModifiedAt: new Date("2023-03-25"),
      layerCount: 15,
      path: "/styles/hybrid.json",
      type: "hybrid",
      versionHash: "abc123",
    },
    "Satellite Style": {
      path: "/styles/satellite.json",
    },
  };

  const defaultProps = {
    error: null,
    isLoading: false,
    isRetrying: false,
    onRetry: jest.fn(),
    onSearchChangeAction: jest.fn(),
    searchQuery: "",
    styles: mockStyles,
  };

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it("renders loading skeleton when isLoading is true", () => {
    render(<StylesCatalog {...defaultProps} isLoading={true} />);

    expect(screen.getByText("Styles Catalog")).toBeInTheDocument();
    expect(screen.getByText("Preview all available map styles and themes")).toBeInTheDocument();

    // Check for skeleton loading elements (they have animate-pulse class)
    const skeletonElements = document.querySelectorAll('.animate-pulse');
    expect(skeletonElements.length).toBeGreaterThan(0);
  });

  it("renders error state when there is an error", () => {
    const error = new Error("Test error");
    render(<StylesCatalog {...defaultProps} error={error} />);

    expect(screen.getByText("Failed to Load Styles")).toBeInTheDocument();
    expect(screen.getByText("Unable to fetch style catalog from the server")).toBeInTheDocument();
    expect(screen.getByText("Test error")).toBeInTheDocument();
    expect(screen.getByText("Try Again")).toBeInTheDocument();
  });

  it("renders styles catalog correctly", () => {
    render(<StylesCatalog {...defaultProps} />);

    expect(screen.getByText("Styles Catalog")).toBeInTheDocument();
    expect(screen.getByText("Browse and preview all available map styles and themes")).toBeInTheDocument();

    // Check that each style name is displayed
    expect(screen.getByText("Basic Style")).toBeInTheDocument();
    expect(screen.getByText("Satellite Style")).toBeInTheDocument();
    expect(screen.getByText("Hybrid Style")).toBeInTheDocument();

    // Verify paths are displayed
    expect(screen.getByText("/styles/basic.json")).toBeInTheDocument();
    expect(screen.getByText("/styles/satellite.json")).toBeInTheDocument();
    expect(screen.getByText("/styles/hybrid.json")).toBeInTheDocument();

    // Verify type badges are displayed
    expect(screen.getByText("vector")).toBeInTheDocument();
    expect(screen.getByText("hybrid")).toBeInTheDocument();

    // Verify layer counts are displayed
    expect(screen.getByText("10")).toBeInTheDocument();
    expect(screen.getByText("15")).toBeInTheDocument();

    // Verify version hashes are displayed
    expect(screen.getByText("abc123")).toBeInTheDocument();
  });

  it("filters styles based on search query - by name", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="basic" />);

    // Should only show the Basic Style
    expect(screen.getByText("Basic Style")).toBeInTheDocument();
    expect(screen.queryByText("Satellite Style")).not.toBeInTheDocument();
    expect(screen.queryByText("Hybrid Style")).not.toBeInTheDocument();
  });

  it("filters styles based on search query - by path", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="satellite.json" />);

    // Should only show the Satellite Style
    expect(screen.queryByText("Basic Style")).not.toBeInTheDocument();
    expect(screen.getByText("Satellite Style")).toBeInTheDocument();
    expect(screen.queryByText("Hybrid Style")).not.toBeInTheDocument();
  });

  it("filters styles based on search query - by type", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="hybrid" />);

    // Should only show the Hybrid Style
    expect(screen.queryByText("Basic Style")).not.toBeInTheDocument();
    expect(screen.queryByText("Satellite Style")).not.toBeInTheDocument();
    expect(screen.getByText("Hybrid Style")).toBeInTheDocument();
  });

  it("shows no results message when search has no matches", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="nonexistent" />);

    expect(screen.getByText(/No styles found matching "nonexistent"/i)).toBeInTheDocument();
    expect(screen.getByText("Learn how to configure Styles")).toBeInTheDocument();

    // Should not render any style names
    expect(screen.queryByText("Basic Style")).not.toBeInTheDocument();
    expect(screen.queryByText("Satellite Style")).not.toBeInTheDocument();
    expect(screen.queryByText("Hybrid Style")).not.toBeInTheDocument();
  });

  it("shows no results message when no styles provided", () => {
    render(<StylesCatalog {...defaultProps} styles={{}} />);

    expect(screen.getByText("No styles found.")).toBeInTheDocument();
    expect(screen.getByText("Learn how to configure Styles")).toBeInTheDocument();
  });

  it("shows no results message when styles is undefined", () => {
    render(<StylesCatalog {...defaultProps} styles={undefined} />);

    expect(screen.getByText("No styles found.")).toBeInTheDocument();
    expect(screen.getByText("Learn how to configure Styles")).toBeInTheDocument();
  });

  it("calls onSearchChangeAction when search input changes", () => {
    render(<StylesCatalog {...defaultProps} />);
    const searchInput = screen.getByPlaceholderText("Search styles...");

    fireEvent.change(searchInput, { target: { value: "new search" } });
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith("new search");
  });

  it("calls onRetry when retry button is clicked in error state", () => {
    const mockOnRetry = jest.fn();
    const error = new Error("Test error");

    render(<StylesCatalog {...defaultProps} error={error} onRetry={mockOnRetry} />);

    const retryButton = screen.getByText("Try Again");
    fireEvent.click(retryButton);

    expect(mockOnRetry).toHaveBeenCalledTimes(1);
  });

  it("displays color palettes when available", () => {
    render(<StylesCatalog {...defaultProps} />);

    // Only Basic Style has colors defined
    expect(screen.getByText("Color Palette:")).toBeInTheDocument();

    // Check that color swatches are rendered
    const colorSwatches = document.querySelectorAll('[style*="background-color"]');
    expect(colorSwatches.length).toBeGreaterThan(0);
  });

  it("renders copy link and preview buttons for each style", () => {
    render(<StylesCatalog {...defaultProps} />);

    // We should have 3 copy link buttons (one for each style)
    const copyLinkButtons = screen.getAllByText("Copy Link");
    expect(copyLinkButtons.length).toBe(3);

    // We should have 3 preview buttons (one for each style)
    const previewButtons = screen.getAllByText("Preview");
    expect(previewButtons.length).toBe(3);
  });

  it("renders disabled preview buttons with tooltip", () => {
    render(<StylesCatalog {...defaultProps} />);

    const previewButtons = screen.getAllByText("Preview");
    expect(previewButtons.length).toBe(3);

    // Check for tooltip content indicating buttons are disabled/not implemented (there are 3 instances)
    const tooltipTexts = screen.getAllByText("Not currently implemented in the frontend");
    expect(tooltipTexts.length).toBe(3);
  });

  it("displays search input with correct placeholder", () => {
    render(<StylesCatalog {...defaultProps} />);

    const searchInput = screen.getByPlaceholderText("Search styles...");
    expect(searchInput).toBeInTheDocument();
    expect(searchInput).toHaveValue("");
  });

  it("displays search input with correct value", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="test query" />);

    const searchInput = screen.getByPlaceholderText("Search styles...");
    expect(searchInput).toHaveValue("test query");
  });

  it("case-insensitive search works correctly", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="BASIC" />);

    expect(screen.getByText("Basic Style")).toBeInTheDocument();
    expect(screen.queryByText("Satellite Style")).not.toBeInTheDocument();
    expect(screen.queryByText("Hybrid Style")).not.toBeInTheDocument();
  });

  it("displays last modified dates when available", () => {
    render(<StylesCatalog {...defaultProps} />);

    // Check that dates are formatted and displayed - expect locale string format
    const modifiedLabels = screen.getAllByText("Modified:");
    expect(modifiedLabels.length).toBe(2); // Only Basic Style and Hybrid Style have dates
  });

  it("renders correct icons for different style types", () => {
    render(<StylesCatalog {...defaultProps} />);

    // Check for SVG elements - different style types should have different icons
    const svgElements = document.querySelectorAll('svg');

    // Should have at least 4 SVGs: search icon + 3 style type icons
    expect(svgElements.length).toBeGreaterThan(3);
  });

  it("shows layer count for styles that have it", () => {
    render(<StylesCatalog {...defaultProps} />);

    // Check that layer count labels are present
    const layerLabels = screen.getAllByText("Layers:");
    expect(layerLabels.length).toBe(2); // Only Basic Style and Hybrid Style have layer counts

    expect(screen.getByText("10")).toBeInTheDocument(); // Basic Style layerCount
    expect(screen.getByText("15")).toBeInTheDocument(); // Hybrid Style layerCount
  });

  it("shows version hash for styles that have it", () => {
    render(<StylesCatalog {...defaultProps} />);

    // Check that version hash label is present
    expect(screen.getByText("Version:")).toBeInTheDocument();
    expect(screen.getByText("abc123")).toBeInTheDocument(); // Hybrid Style versionHash
  });

  it("renders MapLibre map components for each style", () => {
    render(<StylesCatalog {...defaultProps} />);

    // Check that MapLibre maps are rendered for each style
    const mapElements = screen.getAllByTestId("maplibre-map");
    expect(mapElements.length).toBe(3); // One for each style
  });
});
