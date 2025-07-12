import "@testing-library/jest-dom";
import React from "react";

// Make React available globally for JSX
global.React = React;

// Mock React.lazy for tests
jest.mock("react", () => ({
  ...jest.requireActual("react"),
  lazy: (fn) => {
    const Component = fn();
    return Component.default || Component;
  },
}));

// Mock import.meta.env globally
global.import = {
  meta: {
    env: {
      VITE_MARTIN_VERSION: "v0.0.0-test",
      VITE_MARTIN_BASE: "http://localhost:3000",
    },
  },
};

// Mock environment variables for Jest (transform converts import.meta.env to process.env)
process.env.VITE_MARTIN_VERSION = "v0.0.0-test";
process.env.VITE_MARTIN_BASE = "http://localhost:3000";

// Suppress console errors during tests
global.console = {
  ...console,
  // Uncomment to ignore specific console methods during tests
  // error: jest.fn(),
  // warn: jest.fn(),
};

// Mock window.matchMedia
Object.defineProperty(window, "matchMedia", {
  value: jest.fn().mockImplementation((query) => ({
    addEventListener: jest.fn(),
    addListener: jest.fn(),
    dispatchEvent: jest.fn(),
    matches: false,
    media: query,
    onchange: null,
    removeEventListener: jest.fn(),
    removeListener: jest.fn(),
  })),
  writable: true,
});

// Mock browser APIs needed for MapLibre GL
Object.defineProperty(window.URL, "createObjectURL", {
  value: jest.fn(() => "mock-object-url"),
  writable: true,
});

Object.defineProperty(window.URL, "revokeObjectURL", {
  value: jest.fn(),
  writable: true,
});

// Mock ResizeObserver
global.ResizeObserver = jest.fn().mockImplementation(() => ({
  observe: jest.fn(),
  unobserve: jest.fn(),
  disconnect: jest.fn(),
}));

// Mock IntersectionObserver
global.IntersectionObserver = jest.fn().mockImplementation(() => ({
  observe: jest.fn(),
  unobserve: jest.fn(),
  disconnect: jest.fn(),
}));

// Mock Radix UI Tooltip components only when they cause context errors
// This allows test-utils to work properly with the real TooltipProvider
jest.mock("@radix-ui/react-tooltip", () => {
  const originalModule = jest.requireActual("@radix-ui/react-tooltip");
  const React = require("react");
  return {
    ...originalModule,
    __esModule: true,
    Provider: originalModule.Provider,
    Root: ({ children }) => React.createElement("div", null, children),
    Trigger: ({ children }) => React.createElement("div", null, children),
    Content: ({ children }) => React.createElement("div", null, children),
  };
});

// Mock canvas context for MapLibre GL
HTMLCanvasElement.prototype.getContext = jest.fn((contextType) => {
  if (contextType === '2d') {
    return {
      fillRect: jest.fn(),
      clearRect: jest.fn(),
      getImageData: jest.fn(() => ({ data: new Array(4) })),
      putImageData: jest.fn(),
      createImageData: jest.fn(() => ({ data: new Array(4) })),
      setTransform: jest.fn(),
      drawImage: jest.fn(),
      save: jest.fn(),
      fillText: jest.fn(),
      restore: jest.fn(),
      beginPath: jest.fn(),
      moveTo: jest.fn(),
      lineTo: jest.fn(),
      closePath: jest.fn(),
      stroke: jest.fn(),
      translate: jest.fn(),
      scale: jest.fn(),
      rotate: jest.fn(),
      arc: jest.fn(),
      fill: jest.fn(),
      measureText: jest.fn(() => ({ width: 0 })),
      transform: jest.fn(),
      rect: jest.fn(),
      clip: jest.fn(),
    };
  }
  return null;
});

// Mock global fetch
global.fetch = jest.fn(() =>
  Promise.resolve({
    ok: true,
    json: () => Promise.resolve({}),
    text: () => Promise.resolve(""),
    blob: () => Promise.resolve(new Blob()),
  })
);

// Mock MapLibre GL and react-maplibre to avoid dynamic import issues
jest.mock("@vis.gl/react-maplibre", () => ({
  Map: ({ children, style, onMove, onLoad, ...props }) => {
    const React = require("react");
    return React.createElement(
      "div",
      {
        "data-testid": "maplibre-map",
        style: style || { width: "100%", height: "400px" },
        onClick: () => {
          if (onMove) {
            onMove({ viewState: { longitude: 0, latitude: 0, zoom: 1 } });
          }
          if (onLoad) {
            onLoad();
          }
        },
      },
      children
    );
  },
  Source: ({ children, ...props }) => {
    const React = require("react");
    return React.createElement("div", { "data-testid": "maplibre-source" }, children);
  },
  FullscreenControl: ({ ...props }) => {
    const React = require("react");
    return React.createElement("div", { "data-testid": "fullscreen-control" });
  },
}));

// Mock maplibre-gl itself
jest.mock("maplibre-gl", () => ({
  Map: jest.fn().mockImplementation(() => ({
    on: jest.fn(),
    off: jest.fn(),
    addControl: jest.fn(),
    removeControl: jest.fn(),
    addSource: jest.fn(),
    removeSource: jest.fn(),
    addLayer: jest.fn(),
    removeLayer: jest.fn(),
    setStyle: jest.fn(),
    getStyle: jest.fn(() => ({})),
    remove: jest.fn(),
  })),
  Popup: jest.fn().mockImplementation(() => ({
    addTo: jest.fn(),
    remove: jest.fn(),
    setLngLat: jest.fn(),
    setHTML: jest.fn(),
  })),
  NavigationControl: jest.fn(),
  ScaleControl: jest.fn(),
  GeolocateControl: jest.fn(),
}));

// Mock maplibre-gl CSS import
jest.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));

// Mock MapLibre GL Inspect
jest.mock("@maplibre/maplibre-gl-inspect", () => {
  return jest.fn().mockImplementation(() => ({
    onAdd: jest.fn(),
    onRemove: jest.fn(),
  }));
});

// Mock MapLibre GL Inspect CSS
jest.mock("@maplibre/maplibre-gl-inspect/dist/maplibre-gl-inspect.css", () => ({}));
