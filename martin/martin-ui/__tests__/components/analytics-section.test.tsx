import type React from "react";
import { AnalyticsSection } from "@/components/analytics-section";
import type { AnalyticsData } from "@/lib/types";
import { fireEvent, render, screen } from "../test-utils";

// Mock icons and UI components to avoid unnecessary complexity in snapshots
jest.mock("lucide-react", () => ({
  Image: () => <div data-testid="icon-image" />,
  Layers: () => <div data-testid="icon-layers" />,
  Palette: () => <div data-testid="icon-palette" />,
  Type: () => <div data-testid="icon-type" />,
}));
jest.mock("@/components/ui/card", () => ({
  Card: ({ children }: { children: React.ReactNode }) => <div data-testid="card">{children}</div>,
  CardContent: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="card-content">{children}</div>
  ),
  CardHeader: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="card-header">{children}</div>
  ),
  CardTitle: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="card-title">{children}</div>
  ),
}));
jest.mock("@/components/ui/skeleton", () => ({
  Skeleton: ({ className }: { className?: string }) => (
    <div className={className} data-testid="skeleton" />
  ),
}));
jest.mock("@/components/error/error-state", () => ({
  ErrorState: (props: any) => (
    <div data-testid="error-state">
      {props.title} - {props.description} - {props.error?.message || props.error}
      <button disabled={props.isRetrying} onClick={props.onRetry}>
        Retry
      </button>
    </div>
  ),
}));

const analytics: AnalyticsData = {
  sprites: {
    averageRequestDurationMs: 10.5,
    requestCount: 1000,
  },
  tiles: {
    averageRequestDurationMs: 25.2,
    requestCount: 50000,
  },
  fonts: {
    averageRequestDurationMs: 5.1,
    requestCount: 500,
  },
  styles: {
    averageRequestDurationMs: 2.3,
    requestCount: 200,
  },
};

describe("AnalyticsSection", () => {
  it("renders loading state (skeletons)", () => {
    const { container } = render(<AnalyticsSection isLoading />);
    expect(container).toMatchSnapshot();
    expect(screen.getAllByTestId("skeleton").length).toBeGreaterThan(0);
  });

  it("renders analytics data", () => {
    const { container } = render(<AnalyticsSection analytics={analytics} />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText("11 ms")).toBeInTheDocument();
    expect(screen.getByText("1,000 requests")).toBeInTheDocument();
    expect(screen.getByText("25 ms")).toBeInTheDocument();
    expect(screen.getByText("50,000 requests")).toBeInTheDocument();
    expect(screen.getByText("5 ms")).toBeInTheDocument();
    expect(screen.getByText("500 requests")).toBeInTheDocument();
    expect(screen.getByText("2 ms")).toBeInTheDocument();
    expect(screen.getByText("200 requests")).toBeInTheDocument();
  });

  it("renders error state and calls onRetry", () => {
    const onRetry = jest.fn();
    const error = new Error("Test error");
    render(<AnalyticsSection error={error} isRetrying={false} onRetry={onRetry} />);
    expect(screen.getByTestId("error-state")).toHaveTextContent("Test error");
    fireEvent.click(screen.getByText("Retry"));
    expect(onRetry).toHaveBeenCalled();
  });

  it("renders retrying state (button disabled)", () => {
    const onRetry = jest.fn();
    const error = new Error("Retry error");
    render(<AnalyticsSection error={error} isRetrying={true} onRetry={onRetry} />);
    expect(screen.getByText("Retry")).toBeDisabled();
  });
});
