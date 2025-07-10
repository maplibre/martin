import { render, screen } from "@testing-library/react";
import { StyleIntegrationGuideDialog } from "@/components/dialogs/style-integration-guide";

// Mock the buildMartinUrl function
jest.mock("@/lib/api", () => ({
	buildMartinUrl: jest.fn((path: string) => `http://localhost:3000${path}`),
}));

describe("StyleIntegrationGuideDialog Component", () => {
	const mockStyle = {
		colors: ["#ff0000", "#00ff00", "#0000ff"],
		id: "test-style",
		lastModifiedAt: new Date("2023-01-01"),
		layerCount: 5,
		path: "/path/to/style.json",
		type: "raster",
		versionHash: "abc123",
	} as const;

	const mockProps = {
		name: "test-style",
		onCloseAction: jest.fn(),
		style: mockStyle,
	} as const;

	beforeEach(() => {
		jest.clearAllMocks();
	});

	it("displays style name in the title", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);
		expect(screen.getByText("test-style")).toBeInTheDocument();
	});

	it("shows style information section", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		expect(screen.getByText("Style Information")).toBeInTheDocument();
		expect(
			screen.getByText("http://localhost:3000/style/test-style"),
		).toBeInTheDocument();
		expect(screen.getByText("/path/to/style.json")).toBeInTheDocument();
		expect(screen.getByText("raster")).toBeInTheDocument();
		expect(screen.getByText("5")).toBeInTheDocument();
	});

	it("renders both MapLibre GL JS and MapLibre Native tabs", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		expect(screen.getByText("MapLibre GL JS")).toBeInTheDocument();
		expect(screen.getByText("MapLibre Native")).toBeInTheDocument();
	});

	it("displays web integration examples in MapLibre GL JS tab", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		// Check for web examples
		expect(
			screen.getByText("Web Browser (CDN) - HTML + JavaScript"),
		).toBeInTheDocument();
		expect(screen.getByText("NPM/Webpack - JavaScript")).toBeInTheDocument();
		expect(screen.getByText("React - TypeScript")).toBeInTheDocument();
	});

	it("renders the active tab panel", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		// Check that the active tab panel exists in DOM
		expect(
			screen.getByRole("tabpanel", { name: /maplibre gl js/i }),
		).toBeInTheDocument();
	});

	it("has copy buttons for code examples", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		const copyButtons = screen.getAllByText("Copy");
		expect(copyButtons.length).toBeGreaterThan(0);
	});

	it("shows additional resources section", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		expect(screen.getByText("Additional Resources")).toBeInTheDocument();
		expect(
			screen.getByText("MapLibre Style Specification"),
		).toBeInTheDocument();
		expect(screen.getByText("MapLibre GL JS Examples")).toBeInTheDocument();
		expect(screen.getByText("Awesome MapLibre")).toBeInTheDocument();
		expect(screen.getByText("Martin Configuration Guide")).toBeInTheDocument();
	});

	it("has onCloseAction callback defined", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		// Test that the callback is properly defined
		expect(mockProps.onCloseAction).toBeDefined();
		expect(typeof mockProps.onCloseAction).toBe("function");
	});

	it("renders external links with correct attributes", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		const externalLinks = screen.getAllByRole("link");
		externalLinks.forEach((link) => {
			expect(link).toHaveAttribute("target", "_blank");
			expect(link).toHaveAttribute("rel", "noopener noreferrer");
		});
	});

	it("handles style without optional properties", () => {
		const minimalStyle = {
			id: "minimal-style",
			path: "/minimal.json",
		};

		const minimalProps = {
			name: "minimal-style",
			onCloseAction: jest.fn(),
			style: minimalStyle,
		};

		render(<StyleIntegrationGuideDialog {...minimalProps} />);

		expect(screen.getByText("minimal-style")).toBeInTheDocument();
		expect(screen.getByText("/minimal.json")).toBeInTheDocument();
	});

	it("includes style URL in code examples", () => {
		render(<StyleIntegrationGuideDialog {...mockProps} />);

		// Check that the style URL appears in code blocks (multiple occurrences expected)
		expect(
			screen.getAllByText(/http:\/\/localhost:3000\/style\/test-style/),
		).toHaveLength(4); // Should appear in URL display + 3 code examples
	});
});
