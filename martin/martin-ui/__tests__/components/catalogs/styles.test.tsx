import { fireEvent, render, screen } from "@testing-library/react";
import type React from "react";
import { StylesCatalog } from "@/components/catalogs/styles";
import type { Style } from "@/lib/types";

// Mock all dependencies
jest.mock("@/components/error/error-state", () => ({
  ErrorState: ({ title, description }: { title: string; description: string }) => (
    <div data-testid="error-state">
      <div data-testid="error-title">{title}</div>
      <div data-testid="error-description">{description}</div>
    </div>
  ),
}));

jest.mock("@/components/loading/catalog-skeleton", () => ({
  CatalogSkeleton: ({ title, description }: { title: string; description: string }) => (
    <div data-testid="catalog-skeleton">
      <div data-testid="skeleton-title">{title}</div>
      <div data-testid="skeleton-description">{description}</div>
    </div>
  ),
}));

// Mock UI components to avoid tooltip provider issues
jest.mock("@/components/ui/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  TooltipContent: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="tooltip-content">{children}</div>
  ),
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="tooltip-trigger">{children}</div>
  ),
}));

jest.mock("@/components/ui/button", () => ({
  Button: ({ asChild, children, ...props }: any) => <button {...props}>{children}</button>,
}));

jest.mock("@/components/ui/copy-link-button", () => ({
  CopyLinkButton: ({ toastMessage, children, ...props }: any) => (
    <button data-testid="copy-link-button" {...props}>{children ?? "Copy Link"}</button>
  ),
}));

jest.mock("@/components/ui/badge", () => ({
  Badge: ({ children, ...props }: any) => (
    <span data-testid="badge" {...props}>
      {children}
    </span>
  ),
}));

jest.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

jest.mock("@/components/ui/card", () => ({
  Card: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  CardContent: ({ children, ...props }: any) => (
    <div data-testid="card-content" {...props}>
      {children}
    </div>
  ),
  CardDescription: ({ children, ...props }: any) => (
    <div data-testid="card-description" {...props}>
      {children}
    </div>
  ),
  CardHeader: ({ children, ...props }: any) => (
    <div data-testid="card-header" {...props}>
      {children}
    </div>
  ),
  CardTitle: ({ children, ...props }: any) => (
    <div data-testid="card-title" {...props}>
      {children}
    </div>
  ),
}));

jest.mock("@/components/ui/disabledNonInteractiveButton", () => ({
  DisabledNonInteractiveButton: ({ children, ...props }: any) => (
    <button {...props} disabled>
      {children}
    </button>
  ),
}));

jest.mock("lucide-react", () => ({
  Brush: () => <div data-testid="brush-icon">Brush</div>,
  Download: () => <div data-testid="download-icon">Download</div>,
  Eye: () => <div data-testid="eye-icon">Eye</div>,
  Map: () => <div data-testid="map-icon">Map</div>,
  Search: () => <div data-testid="search-icon">Search</div>,
}));

// Mock MapLibre to avoid dynamic import issues
jest.mock("@vis.gl/react-maplibre", () => ({
  Map: ({ children, ...props }: any) => (
    <div data-testid="maplibre-map" style={props.style}>
      {children}
    </div>
  ),
}));

// Mock maplibre-gl CSS import
jest.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));

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

  it("matches snapshot for loading state", () => {
    const { asFragment } = render(<StylesCatalog {...defaultProps} isLoading={true} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it("matches snapshot for loaded state with mock data", () => {
    const { asFragment } = render(<StylesCatalog {...defaultProps} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it("renders loading skeleton when isLoading is true", () => {
    render(<StylesCatalog {...defaultProps} isLoading={true} />);
    expect(screen.getByTestId("catalog-skeleton")).toBeInTheDocument();
    expect(screen.getByTestId("skeleton-title").textContent).toBe("Styles Catalog");
    expect(screen.getByTestId("skeleton-description").textContent).toBe(
      "Preview all available map styles and themes",
    );
  });

  it("renders error state when there is an error", () => {
    const error = new Error("Test error");
    render(<StylesCatalog {...defaultProps} error={error} />);
    expect(screen.getByTestId("error-state")).toBeInTheDocument();
    expect(screen.getByTestId("error-title").textContent).toBe("Failed to Load Styles");
  });

  it("renders styles catalog correctly", () => {
    render(<StylesCatalog {...defaultProps} />);
    expect(screen.getByText("Styles Catalog")).toBeInTheDocument();

    // Get all card headers
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(3);

    // Check that each style name is displayed
    expect(screen.getByText("Basic Style")).toBeInTheDocument();
    expect(screen.getByText("Satellite Style")).toBeInTheDocument();
    expect(screen.getByText("Hybrid Style")).toBeInTheDocument();

    // Verify paths are displayed
    expect(screen.getByText("/styles/basic.json")).toBeInTheDocument();
    expect(screen.getByText("/styles/satellite.json")).toBeInTheDocument();
    expect(screen.getByText("/styles/hybrid.json")).toBeInTheDocument();

    // Verify type badges are displayed
    const badges = screen.getAllByTestId("badge");
    expect(badges.length).toBe(2);
    expect(badges[0].textContent).toBe("vector");
    expect(badges[1].textContent).toBe("hybrid");

    // Verify layer counts are displayed
    expect(screen.getByText("10")).toBeInTheDocument();
    expect(screen.getByText("15")).toBeInTheDocument();

    // Verify version hashes are displayed
    expect(screen.getByText("abc123")).toBeInTheDocument();
  });

  it("filters styles based on search query - by name", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="basic" />);

    // Should only show the Basic Style
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(1);
    expect(screen.getByText("Basic Style")).toBeInTheDocument();
    expect(screen.queryByText("Satellite Style")).not.toBeInTheDocument();
    expect(screen.queryByText("Hybrid Style")).not.toBeInTheDocument();
  });

  it("filters styles based on search query - by path", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="satellite.json" />);

    // Should only show the Satellite Style
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(1);
    expect(screen.queryByText("Basic Style")).not.toBeInTheDocument();
    expect(screen.getByText("Satellite Style")).toBeInTheDocument();
    expect(screen.queryByText("Hybrid Style")).not.toBeInTheDocument();
  });

  it("filters styles based on search query - by type", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="hybrid" />);

    // Should only show the Hybrid Style
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(1);
    expect(screen.queryByText("Basic Style")).not.toBeInTheDocument();
    expect(screen.queryByText("Satellite Style")).not.toBeInTheDocument();
    expect(screen.getByText("Hybrid Style")).toBeInTheDocument();
  });

  it("shows no results message when search has no matches", () => {
    render(<StylesCatalog {...defaultProps} searchQuery="nonexistent" />);
    expect(screen.getByText(/No styles found matching "nonexistent"/i)).toBeInTheDocument();

    // Should not render any cards
    const headers = screen.queryAllByTestId("card-header");
    expect(headers.length).toBe(0);
  });

  it("calls onSearchChangeAction when search input changes", () => {
    render(<StylesCatalog {...defaultProps} />);
    const searchInput = screen.getByPlaceholderText("Search styles...");

    fireEvent.change(searchInput, { target: { value: "new search" } });
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith("new search");
  });

  it("displays color palettes when available", () => {
    render(<StylesCatalog {...defaultProps} />);

    // We should have color palettes for each style
    const colorPalettes = screen.getAllByText("Color Palette:");
    expect(colorPalettes.length).toBe(1);

    // Check that not implemented buttons have a tooltip
    const mapPreviews = screen.getAllByTestId("tooltip-trigger");
    expect(mapPreviews.length).toBe(3);

    // Each preview button should contain an eye icon
    const eyeIcons = screen.getAllByTestId("eye-icon");
    expect(eyeIcons.length).toBe(3);
  });

  it("renders copy link and preview buttons for each style", () => {
    render(<StylesCatalog {...defaultProps} />);

    // We should have 3 copy link buttons (one for each style)
    const copyLinkButtons = screen.getAllByTestId("copy-link-button");
    expect(copyLinkButtons.length).toBe(3);

    // We should have 3 eye icons for preview (one for each style)
    const eyeIcons = screen.getAllByTestId("eye-icon");
    expect(eyeIcons.length).toBe(3);
  });
});
