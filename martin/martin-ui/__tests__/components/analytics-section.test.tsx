import React from "react";
import { render, screen, fireEvent } from "../utils/test-utils";
import { AnalyticsSection } from "@/components/analytics-section";
import type { AnalyticsData } from "@/lib/types";

// Mock icons and UI components to avoid unnecessary complexity in snapshots
jest.mock("lucide-react", () => ({
  Activity: () => <div data-testid="icon-activity" />,
  Server: () => <div data-testid="icon-server" />,
  Zap: () => <div data-testid="icon-zap" />,
  Database: () => <div data-testid="icon-database" />,
}));
jest.mock("@/components/ui/card", () => ({
  Card: ({ children }: { children: React.ReactNode }) => <div data-testid="card">{children}</div>,
  CardHeader: ({ children }: { children: React.ReactNode }) => <div data-testid="card-header">{children}</div>,
  CardTitle: ({ children }: { children: React.ReactNode }) => <div data-testid="card-title">{children}</div>,
  CardContent: ({ children }: { children: React.ReactNode }) => <div data-testid="card-content">{children}</div>,
}));
jest.mock("@/components/ui/skeleton", () => ({
  Skeleton: ({ className }: { className?: string }) => <div data-testid="skeleton" className={className} />,
}));
jest.mock("@/components/error/error-state", () => ({
  ErrorState: (props: any) => (
    <div data-testid="error-state">
      {props.title} - {props.description} - {props.error?.message || props.error}
      <button onClick={props.onRetry} disabled={props.isRetrying}>Retry</button>
    </div>
  ),
}));

const analytics: AnalyticsData = {
  requestsPerSecond: 42,
  memoryUsage: 70,
  cacheHitRate: 95,
  activeSources: 8,
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
    render(<AnalyticsSection error={error} onRetry={onRetry} isRetrying={false} />);
    expect(screen.getByTestId("error-state")).toHaveTextContent("Test error");
    fireEvent.click(screen.getByText("Retry"));
    expect(onRetry).toHaveBeenCalled();
  });

  it("renders retrying state (button disabled)", () => {
    const onRetry = jest.fn();
    const error = new Error("Retry error");
    render(<AnalyticsSection error={error} onRetry={onRetry} isRetrying={true} />);
    expect(screen.getByText("Retry")).toBeDisabled();
  });
});
