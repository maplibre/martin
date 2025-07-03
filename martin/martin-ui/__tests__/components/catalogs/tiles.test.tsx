import { within } from "@testing-library/dom";
import { fireEvent, render, screen } from "@testing-library/react";
import type React from "react";
import { TilesCatalog } from "@/components/catalogs/tiles";
import type { TileSource } from "@/lib/types";

// Mock all dependencies
jest.mock("@/components/error/error-state", () => ({
	ErrorState: ({
		title,
		description,
	}: {
		title: string;
		description: string;
	}) => (
		<div data-testid="error-state">
			<div data-testid="error-title">{title}</div>
			<div data-testid="error-description">{description}</div>
		</div>
	),
}));

jest.mock("@/components/loading/catalog-skeleton", () => ({
	CatalogSkeleton: ({
		title,
		description,
	}: {
		title: string;
		description: string;
	}) => (
		<div data-testid="catalog-skeleton">
			<div data-testid="skeleton-title">{title}</div>
			<div data-testid="skeleton-description">{description}</div>
		</div>
	),
}));

// Mock UI components to avoid tooltip provider issues
jest.mock("@/components/ui/tooltip", () => ({
	Tooltip: ({ children }: { children: React.ReactNode }) => (
		<div>{children}</div>
	),
	TooltipTrigger: ({ children }: { children: React.ReactNode }) => (
		<div>{children}</div>
	),
	TooltipContent: ({ children }: { children: React.ReactNode }) => (
		<div data-testid="tooltip-content">{children}</div>
	),
}));

jest.mock("@/components/ui/button", () => ({
	Button: ({ children, ...props }: any) => (
		<button {...props}>{children}</button>
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
	CardDescription: ({ children, ...props }: any) => (
		<div data-testid="card-description" {...props}>
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
	Database: () => <div data-testid="database-icon">DB</div>,
	Eye: () => <div data-testid="eye-icon">Eye</div>,
	ImageIcon: () => <div data-testid="image-icon">Image</div>,
	Layers: () => <div data-testid="layers-icon">Layers</div>,
	Palette: () => <div data-testid="palette-icon">Palette</div>,
	Search: () => <div data-testid="search-icon">Search</div>,
}));

describe("TilesCatalog Component", () => {
	const mockTileSources: { [tile_id: string]: TileSource } = {
		source1: {
			name: "Test Source 1",
			description: "Description for test source 1",
			content_type: "application/x-protobuf",
			attribution: "Test attribution 1",
			layerCount: 5,
		},
		source2: {
			name: "Test Source 2",
			description: "Description for test source 2",
			content_type: "image/png",
			attribution: "Test attribution 2",
			layerCount: 3,
		},
		source3: {
			name: "Test Source 3",
			description: "Description for test source 3",
			content_type: "application/json",
			attribution: "Test attribution 3",
			layerCount: 7,
		},
	};

	const defaultProps = {
		tileSources: mockTileSources,
		searchQuery: "",
		onSearchChangeAction: jest.fn(),
		isLoading: false,
		error: null,
		onRetry: jest.fn(),
		isRetrying: false,
	};

	it("renders loading skeleton when isLoading is true", () => {
		render(<TilesCatalog {...defaultProps} isLoading={true} />);
		expect(screen.getByTestId("catalog-skeleton")).toBeInTheDocument();
	});

	it("renders error state when there is an error", () => {
		const error = new Error("Test error");
		render(<TilesCatalog {...defaultProps} error={error} />);
		expect(screen.getByTestId("error-state")).toBeInTheDocument();
		expect(screen.getByTestId("error-title").textContent).toBe(
			"Failed to Load Tiles Catalog",
		);
	});

	it("renders tile sources correctly", () => {
		render(<TilesCatalog {...defaultProps} />);
		expect(screen.getByText("Tiles Sources Catalog")).toBeInTheDocument();

		// Get all card descriptions - these contain the source names
		const descriptions = screen.getAllByTestId("card-description");

		// Check that each of our expected source names exists somewhere
		expect(
			descriptions.some((el) => el.textContent?.includes("Test Source 1")),
		).toBe(true);
		expect(
			descriptions.some((el) => el.textContent?.includes("Test Source 2")),
		).toBe(true);
		expect(
			descriptions.some((el) => el.textContent?.includes("Test Source 3")),
		).toBe(true);
	});

	it("filters tile sources based on search query", () => {
		render(<TilesCatalog {...defaultProps} searchQuery="source 2" />);

		// Get all card descriptions - these contain the source names
		const descriptions = screen.getAllByTestId("card-description");

		// Should have only one source that matches
		expect(descriptions.length).toBe(1);
		expect(descriptions[0].textContent).toContain("Test Source 2");

		// Grid should only contain the source2 component
		const gridItems = screen.getAllByTestId("card-header");
		expect(gridItems.length).toBe(1);
	});

	it("shows no results message when search has no matches", () => {
		render(<TilesCatalog {...defaultProps} searchQuery="nonexistent" />);
		expect(
			screen.getByText(/No tile sources found matching "nonexistent"/i),
		).toBeInTheDocument();

		// Should not render any cards
		const gridItems = screen.queryAllByTestId("card-header");
		expect(gridItems.length).toBe(0);
	});

	it("calls onSearchChangeAction when search input changes", () => {
		render(<TilesCatalog {...defaultProps} />);
		const searchInput = screen.getByPlaceholderText("Search tiles sources...");

		fireEvent.change(searchInput, { target: { value: "new search" } });
		expect(defaultProps.onSearchChangeAction).toHaveBeenCalledWith(
			"new search",
		);
	});

	it("renders different icons based on content_type", () => {
		render(<TilesCatalog {...defaultProps} />);

		// Each icon type should be present exactly once (one for each source)
		const layersIcon = screen.getAllByTestId("layers-icon");
		const imageIcon = screen.getAllByTestId("image-icon");
		const databaseIcon = screen.getAllByTestId("database-icon");

		expect(layersIcon.length).toBe(1);
		expect(imageIcon.length).toBe(1);
		expect(databaseIcon.length).toBe(1);
	});
});
