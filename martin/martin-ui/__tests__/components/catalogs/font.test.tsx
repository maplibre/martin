import { fireEvent, render, screen } from "@testing-library/react";
import type React from "react";
import { FontCatalog } from "@/components/catalogs/font";
import type { Font } from "@/lib/types";

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
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
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
  Download: () => <div data-testid="download-icon">Download</div>,
  Eye: () => <div data-testid="eye-icon">Eye</div>,
  Search: () => <div data-testid="search-icon">Search</div>,
  Type: () => <div data-testid="type-icon">Type</div>,
}));

describe("FontCatalog Component", () => {
  const mockFonts: { [name: string]: Font } = {
    "Noto Sans Bold": {
      end: 255,
      family: "Noto Sans",
      format: "ttf",
      glyphs: 380,
      lastModifiedAt: new Date("2023-03-20"),
      start: 0,
      style: "Bold",
    },
    "Open Sans Regular": {
      end: 255,
      family: "Open Sans",
      format: "otf",
      glyphs: 420,
      lastModifiedAt: new Date("2023-02-15"),
      start: 0,
      style: "Regular",
    },
    "Roboto Medium": {
      end: 255,
      family: "Roboto",
      format: "ttf",
      glyphs: 350,
      lastModifiedAt: new Date("2023-01-01"),
      start: 0,
      style: "Medium",
    },
  };

  const defaultProps = {
    error: null,
    fonts: mockFonts,
    isLoading: false,
    isRetrying: false,
    onRetry: jest.fn(),
    onSearchChangeAction: jest.fn(),
    searchQuery: "",
  };

  it("matches snapshot for loading state", () => {
    const { asFragment } = render(<FontCatalog {...defaultProps} isLoading={true} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it("matches snapshot for loaded state with mock data", () => {
    const { asFragment } = render(<FontCatalog {...defaultProps} />);
    expect(asFragment()).toMatchSnapshot();
  });

  it("renders loading skeleton when isLoading is true", () => {
    render(<FontCatalog {...defaultProps} isLoading={true} />);
    expect(screen.getByTestId("catalog-skeleton")).toBeInTheDocument();
    expect(screen.getByTestId("skeleton-title").textContent).toBe("Font Catalog");
    expect(screen.getByTestId("skeleton-description").textContent).toBe(
      "Preview all available font assets",
    );
  });

  it("renders error state when there is an error", () => {
    const error = new Error("Test error");
    render(<FontCatalog {...defaultProps} error={error} />);
    expect(screen.getByTestId("error-state")).toBeInTheDocument();
    expect(screen.getByTestId("error-title").textContent).toBe("Failed to Load Fonts");
  });

  it("renders font catalog correctly", () => {
    render(<FontCatalog {...defaultProps} />);
    expect(screen.getByText("Font Catalog")).toBeInTheDocument();

    // Get all card headers
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(3);

    // Check that each font name is displayed
    expect(screen.getByText("Roboto Medium")).toBeInTheDocument();
    expect(screen.getByText("Open Sans Regular")).toBeInTheDocument();
    expect(screen.getByText("Noto Sans Bold")).toBeInTheDocument();

    // Verify format badges are displayed
    const badges = screen.getAllByTestId("badge");
    expect(badges.length).toBe(3);
    expect(badges[0].textContent).toBe("ttf");
    expect(badges[1].textContent).toBe("otf");
    expect(badges[2].textContent).toBe("ttf");

    // Verify glyph counts are displayed
    expect(screen.getByText("350")).toBeInTheDocument();
    expect(screen.getByText("420")).toBeInTheDocument();
    expect(screen.getByText("380")).toBeInTheDocument();

    // Verify family and style information is displayed
    expect(screen.getByText("Family: Roboto • Style: Medium")).toBeInTheDocument();
    expect(screen.getByText("Family: Open Sans • Style: Regular")).toBeInTheDocument();
    expect(screen.getByText("Family: Noto Sans • Style: Bold")).toBeInTheDocument();
  });

  it("filters fonts based on search query", () => {
    render(<FontCatalog {...defaultProps} searchQuery="roboto" />);

    // Should only show the Roboto font
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(1);
    expect(screen.getByText("Roboto Medium")).toBeInTheDocument();
    expect(screen.queryByText("Open Sans Regular")).not.toBeInTheDocument();
    expect(screen.queryByText("Noto Sans Bold")).not.toBeInTheDocument();
  });

  it("filters fonts based on font family", () => {
    render(<FontCatalog {...defaultProps} searchQuery="open" />);

    // Should only show the Open Sans font
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(1);
    expect(screen.queryByText("Roboto Medium")).not.toBeInTheDocument();
    expect(screen.getByText("Open Sans Regular")).toBeInTheDocument();
    expect(screen.queryByText("Noto Sans Bold")).not.toBeInTheDocument();
  });

  it("filters fonts based on style", () => {
    render(<FontCatalog {...defaultProps} searchQuery="bold" />);

    // Should only show the Noto Sans Bold font
    const headers = screen.getAllByTestId("card-header");
    expect(headers.length).toBe(1);
    expect(screen.queryByText("Roboto Medium")).not.toBeInTheDocument();
    expect(screen.queryByText("Open Sans Regular")).not.toBeInTheDocument();
    expect(screen.getByText("Noto Sans Bold")).toBeInTheDocument();
  });

  it("shows no results message when search has no matches", () => {
    render(<FontCatalog {...defaultProps} searchQuery="nonexistent" />);
    expect(screen.getByText(/No fonts found matching "nonexistent"/i)).toBeInTheDocument();

    // Should not render any cards
    const headers = screen.queryAllByTestId("card-header");
    expect(headers.length).toBe(0);
  });

  it("calls onSearchChangeAction when search input changes", () => {
    render(<FontCatalog {...defaultProps} />);
    const searchInput = screen.getByPlaceholderText("Search fonts...");

    fireEvent.change(searchInput, { target: { value: "new search" } });
    expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith("new search");
  });

  it("renders download and details buttons for each font", () => {
    render(<FontCatalog {...defaultProps} />);

    // We should have 3 copy link buttons (one for each font)
    const copyLinkButtons = screen.getAllByTestId("copy-link-button");
    expect(copyLinkButtons.length).toBe(3);

    // We should have 3 eye icons for details (one for each font)
    const eyeIcons = screen.getAllByTestId("eye-icon");
    expect(eyeIcons.length).toBe(3);
  });
});
