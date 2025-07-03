import type React from "react";
import { AnalyticsSection } from "@/components/analytics-section";
import type { AnalyticsData } from "@/lib/types";
import { fireEvent, render, screen } from "../utils/test-utils";

// Mock icons and UI components to avoid unnecessary complexity in snapshots
jest.mock("lucide-react", () => ({
  Activity: () => <div data-testid="icon-activity" />,
  Database: () => <div data-testid="icon-database" />,
  Server: () => <div data-testid="icon-server" />,
  Zap: () => <div data-testid="icon-zap" />,
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
  activeSources: 8,
  cacheHitRate: 95,
  memoryUsage: 70,
  requestsPerSecond: 42,
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
    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText("70%")).toBeInTheDocument();
    expect(screen.getByText("95%")).toBeInTheDocument();
    expect(screen.getByText("8")).toBeInTheDocument();
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
