import { describe, expect, it, jest } from "@jest/globals";
import type { ReactNode } from "react";
import { StyleEditor } from "@/components/style-editor";
import type { ButtonProps } from "@/components/ui/button";
import { render, screen } from "../test-utils";

// Mock component interfaces
interface MockComponentProps {
  children?: ReactNode;
  [key: string]: unknown;
}

// Mock the UI components
jest.mock("@/components/ui/button", () => ({
  Button: ({ children, onClick, ...props }: ButtonProps & { onClick?: () => void }) => (
    <button onClick={onClick} {...props}>
      {children}
    </button>
  ),
}));

jest.mock("@/components/ui/card", () => ({
  Card: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardContent: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardHeader: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardTitle: ({ children, ...props }: MockComponentProps) => <h3 {...props}>{children}</h3>,
}));

jest.mock("@/components/ui/separator", () => ({
  Separator: () => <hr />,
}));

jest.mock("@/lib/api", () => ({
  buildMartinUrl: jest.fn((path: string) => `http://localhost:3000${path}`),
}));

// Mock lucide-react icons
jest.mock("lucide-react", () => ({
  ArrowLeft: () => <span>←</span>,
  X: () => <span>×</span>,
}));

describe("StyleEditor", () => {
  const mockStyle = {
    colors: ["#ff0000", "#00ff00", "#0000ff"],
    lastModifiedAt: new Date("2023-01-01"),
    layerCount: 5,
    path: "/styles/test-style.json",
    type: "vector" as const,
    versionHash: "abc123",
  };

  const defaultProps = {
    onClose: jest.fn(),
    style: mockStyle,
    styleName: "test-style",
  };

  it("renders the style editor with correct title", () => {
    render(<StyleEditor {...defaultProps} />);

    expect(screen.getByText("test-style")).toBeDefined();
    expect(screen.getByText("/styles/test-style.json")).toBeDefined();
  });

  it("renders navigation buttons", () => {
    render(<StyleEditor {...defaultProps} />);

    expect(screen.getByText("Back to Catalog")).toBeDefined();
  });

  it("renders iframe with correct src", () => {
    render(<StyleEditor {...defaultProps} />);

    const iframe = screen.getByTitle("Maputnik Style Editor - test-style");
    expect(iframe).toBeDefined();
    expect(iframe.getAttribute("src")).toBeDefined();

    const src = iframe.getAttribute("src");
    expect(src).toContain("https://maplibre.org/maputnik/");
    expect(src).toContain("style=http%3A%2F%2Flocalhost%3A3000%2Fstyle%2Ftest-style");
  });

  it("renders iframe without loading state", () => {
    render(<StyleEditor {...defaultProps} />);

    const iframe = screen.getByTitle("Maputnik Style Editor - test-style");
    expect(iframe).toBeDefined();
  });

  it("calls onClose when back button is clicked", () => {
    const onClose = jest.fn();
    render(<StyleEditor {...defaultProps} onClose={onClose} />);

    const backButton = screen.getByText("Back to Catalog");
    backButton.click();

    expect(onClose).toHaveBeenCalled();
  });

  it("renders with proper iframe sandbox attributes", () => {
    render(<StyleEditor {...defaultProps} />);

    const iframe = screen.getByTitle("Maputnik Style Editor - test-style");
    expect(iframe.getAttribute("sandbox")).toBe(
      "allow-same-origin allow-scripts allow-forms allow-popups allow-downloads allow-modals",
    );
  });

  it("constructs proper Maputnik URL with encoded style parameter", () => {
    render(<StyleEditor {...defaultProps} />);

    const iframe = screen.getByTitle("Maputnik Style Editor - test-style");
    const src = iframe.getAttribute("src");

    expect(src).toContain("https://maplibre.org/maputnik/");
    expect(src).toContain("style=");
    expect(src).toContain("test-style");
  });
});
