import { fireEvent, screen } from "@testing-library/react";
import React from "react";
import { ErrorState, InlineErrorState } from "@/components/error/error-state";
import { render } from "../../utils/test-utils";

describe("ErrorState Component", () => {
  it("renders generic error state correctly", () => {
    const { container } = render(<ErrorState />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    expect(screen.getByText("An unexpected error occurred. Please try again.")).toBeInTheDocument();
  });

  it("renders network error state correctly", () => {
    const { container } = render(<ErrorState variant="network" />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Network Error")).toBeInTheDocument();
    expect(
      screen.getByText("Unable to connect to the server. Please check your internet connection."),
    ).toBeInTheDocument();
  });

  it("renders server error state correctly", () => {
    const { container } = render(<ErrorState variant="server" />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Server Error")).toBeInTheDocument();
    expect(
      screen.getByText("The server encountered an error. Please try again later."),
    ).toBeInTheDocument();
  });

  it("renders timeout error state correctly", () => {
    const { container } = render(<ErrorState variant="timeout" />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Request Timeout")).toBeInTheDocument();
    expect(
      screen.getByText("The request took too long to complete. Please try again."),
    ).toBeInTheDocument();
  });

  it("renders with custom title and description", () => {
    const { container } = render(
      <ErrorState description="Custom error description for testing" title="Custom Error Title" />,
    );
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Custom Error Title")).toBeInTheDocument();
    expect(screen.getByText("Custom error description for testing")).toBeInTheDocument();
  });

  it("renders error details when showDetails is true", () => {
    const errorMessage = "This is a detailed error message";
    const { container } = render(<ErrorState error={errorMessage} showDetails={true} />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText(errorMessage)).toBeInTheDocument();
  });

  it("renders retry button when onRetry is provided", () => {
    const handleRetry = jest.fn();
    const { container } = render(<ErrorState onRetry={handleRetry} />);
    expect(container).toMatchSnapshot();

    const retryButton = screen.getByText("Try Again");
    expect(retryButton).toBeInTheDocument();

    fireEvent.click(retryButton);
    expect(handleRetry).toHaveBeenCalledTimes(1);
  });

  it("shows retrying state when isRetrying is true", () => {
    const handleRetry = jest.fn();
    const { container } = render(<ErrorState isRetrying={true} onRetry={handleRetry} />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Retrying...")).toBeInTheDocument();
  });
});

describe("InlineErrorState Component", () => {
  it("renders generic inline error state correctly", () => {
    const { container } = render(<InlineErrorState message="Something went wrong" />);
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
  });

  it("renders network inline error state correctly", () => {
    const { container } = render(
      <InlineErrorState message="Network error occurred" variant="network" />,
    );
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Network error occurred")).toBeInTheDocument();
  });

  it("renders with retry button when onRetry is provided", () => {
    const handleRetry = jest.fn();
    const { container } = render(
      <InlineErrorState message="Error with retry" onRetry={handleRetry} />,
    );
    expect(container).toMatchSnapshot();

    const retryButton = screen.getByText("Retry");
    expect(retryButton).toBeInTheDocument();

    fireEvent.click(retryButton);
    expect(handleRetry).toHaveBeenCalledTimes(1);
  });

  it("shows retrying state when isRetrying is true", () => {
    const { container } = render(
      <InlineErrorState isRetrying={true} message="Error while retrying" onRetry={() => {}} />,
    );
    expect(container).toMatchSnapshot();
    expect(screen.getByText("Retrying")).toBeInTheDocument();
  });
});
