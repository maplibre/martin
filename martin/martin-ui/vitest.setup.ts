import React from "react";
import { vi } from "vitest";

// Make React available globally for JSX
globalThis.React = React;

// Mock React.lazy for tests
vi.mock("react", async () => {
	const actual = await vi.importActual("react");
	return {
		...actual,
		lazy: (
			fn: () => React.ComponentType | { default: React.ComponentType },
		) => {
			const Component = fn();
			// Check if Component has default export
			if (
				typeof Component === "object" &&
				Component !== null &&
				"default" in Component
			) {
				return (Component as { default: React.ComponentType }).default;
			}
			return Component as React.ComponentType;
		},
	};
});

// Mock import.meta.env globally - Vitest handles this natively, but we set defaults
vi.stubEnv("VITE_MARTIN_VERSION", "v0.0.0-test");
vi.stubEnv("VITE_MARTIN_BASE", "http://localhost:3000");

// Suppress console errors during tests
globalThis.console = {
	...console,
	// Uncomment to ignore specific console methods during tests
	// error: vi.fn(),
	// warn: vi.fn(),
};

// Mock window.matchMedia
Object.defineProperty(window, "matchMedia", {
	value: vi.fn().mockImplementation((query) => ({
		addEventListener: vi.fn(),
		addListener: vi.fn(),
		dispatchEvent: vi.fn(),
		matches: false,
		media: query,
		onchange: null,
		removeEventListener: vi.fn(),
		removeListener: vi.fn(),
	})),
	writable: true,
});

// Mock browser APIs needed for MapLibre GL
Object.defineProperty(window.URL, "createObjectURL", {
	value: vi.fn(() => "mock-object-url"),
	writable: true,
});

Object.defineProperty(window.URL, "revokeObjectURL", {
	value: vi.fn(),
	writable: true,
});

// Mock ResizeObserver
globalThis.ResizeObserver = vi.fn().mockImplementation(() => ({
	disconnect: vi.fn(),
	observe: vi.fn(),
	unobserve: vi.fn(),
}));

// Mock IntersectionObserver
globalThis.IntersectionObserver = vi.fn().mockImplementation(() => ({
	disconnect: vi.fn(),
	observe: vi.fn(),
	unobserve: vi.fn(),
}));

// Mock Radix UI Tooltip components only when they cause context errors
// This allows test-utils to work properly with the real TooltipProvider
vi.mock("@radix-ui/react-tooltip", async () => {
	const originalModule = await vi.importActual("@radix-ui/react-tooltip");
	const React = await import("react");
	return {
		...originalModule,
		__esModule: true,
		Content: ({ children }: { children: React.ReactNode }) =>
			React.createElement("div", null, children),
		Provider: (originalModule as { Provider: React.ComponentType }).Provider,
		Root: ({ children }: { children: React.ReactNode }) =>
			React.createElement("div", null, children),
		Trigger: ({ children }: { children: React.ReactNode }) =>
			React.createElement("div", null, children),
	};
});

// Mock canvas context for MapLibre GL
HTMLCanvasElement.prototype.getContext = vi.fn((contextType: string) => {
	if (contextType === "2d") {
		const mockCanvas = {} as HTMLCanvasElement;
		const mockImageData = {
			data: new Uint8ClampedArray(4),
			width: 1,
			height: 1,
			colorSpace: "srgb" as PredefinedColorSpace,
		} as ImageData;

		return {
			arc: vi.fn(),
			beginPath: vi.fn(),
			canvas: mockCanvas,
			clearRect: vi.fn(),
			clip: vi.fn(),
			closePath: vi.fn(),
			createImageData: vi.fn(() => mockImageData),
			drawImage: vi.fn(),
			fill: vi.fn(),
			fillRect: vi.fn(),
			fillText: vi.fn(),
			getImageData: vi.fn(() => mockImageData),
			globalAlpha: 1,
			globalCompositeOperation: "source-over" as GlobalCompositeOperation,
			isPointInPath: vi.fn(() => false),
			isPointInStroke: vi.fn(() => false),
			lineTo: vi.fn(),
			measureText: vi.fn(() => ({ width: 0 })),
			moveTo: vi.fn(),
			putImageData: vi.fn(),
			rect: vi.fn(),
			restore: vi.fn(),
			rotate: vi.fn(),
			save: vi.fn(),
			scale: vi.fn(),
			setTransform: vi.fn(),
			stroke: vi.fn(),
			transform: vi.fn(),
			translate: vi.fn(),
		} as unknown as CanvasRenderingContext2D;
	}
	return null;
}) as typeof HTMLCanvasElement.prototype.getContext;

// Mock global fetch
globalThis.fetch = vi.fn(() =>
	Promise.resolve({
		blob: () => Promise.resolve(new Blob()),
		json: () => Promise.resolve({}),
		ok: true,
		text: () => Promise.resolve(""),
	} as Response),
);

// Mock MapLibre GL and react-maplibre to avoid dynamic import issues
vi.mock("@vis.gl/react-maplibre", () => ({
	FullscreenControl: () => {
		const React = require("react");
		return React.createElement("div", { "data-testid": "fullscreen-control" });
	},
	Map: ({
		children,
		style,
		onMove,
		onLoad,
	}: {
		children?: React.ReactNode;
		style?: React.CSSProperties;
		onMove?: () => void;
		onLoad?: () => void;
	}) => {
		const React = require("react");
		return React.createElement(
			"div",
			{
				"data-testid": "maplibre-map",
				onClick: () => {
					if (onMove) {
						onMove();
					}
					if (onLoad) {
						onLoad();
					}
				},
				style: style || { height: "400px", width: "100%" },
			},
			children,
		);
	},
	Source: ({ children }: { children?: React.ReactNode }) => {
		const React = require("react");
		return React.createElement(
			"div",
			{ "data-testid": "maplibre-source" },
			children,
		);
	},
}));

// Mock maplibre-gl itself
vi.mock("maplibre-gl", () => ({
	GeolocateControl: vi.fn(),
	Map: vi.fn().mockImplementation(() => ({
		addControl: vi.fn(),
		addLayer: vi.fn(),
		addSource: vi.fn(),
		getStyle: vi.fn(() => ({})),
		off: vi.fn(),
		on: vi.fn(),
		remove: vi.fn(),
		removeControl: vi.fn(),
		removeLayer: vi.fn(),
		removeSource: vi.fn(),
		setStyle: vi.fn(),
	})),
	NavigationControl: vi.fn(),
	Popup: vi.fn().mockImplementation(() => ({
		addTo: vi.fn(),
		remove: vi.fn(),
		setHTML: vi.fn(),
		setLngLat: vi.fn(),
	})),
	ScaleControl: vi.fn(),
}));

// Mock maplibre-gl CSS import
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));

// Mock MapLibre GL Inspect
vi.mock("@maplibre/maplibre-gl-inspect", () => {
	return vi.fn().mockImplementation(() => ({
		onAdd: vi.fn(),
		onRemove: vi.fn(),
	}));
});

// Mock MapLibre GL Inspect CSS
vi.mock(
	"@maplibre/maplibre-gl-inspect/dist/maplibre-gl-inspect.css",
	() => ({}),
);
