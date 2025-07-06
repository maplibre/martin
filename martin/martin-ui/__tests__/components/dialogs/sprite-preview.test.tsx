import userEvent from "@testing-library/user-event";
import React from "react";
import { SpritePreviewDialog } from "@/components/dialogs/sprite-preview";
import { render, screen } from "@testing-library/react";

// Mock LoadingSpinner component
jest.mock("@/components/loading/loading-spinner", () => ({
  LoadingSpinner: () => <div data-testid="loading-spinner">Loading Spinner Mock</div>,
}));

// Mock the dynamic SpritePreview component to avoid async loading issues
jest.mock("next/dynamic", () => {
  return () => {
    function MockSpritePreview() {
      return (
        <div data-testid="sprite-preview">
          <div data-testid="sprite-item">icon1</div>
          <div data-testid="sprite-item">icon2</div>
          <div data-testid="sprite-item">icon3</div>
        </div>
      );
    }
    return MockSpritePreview;
  };
});

describe("SpritePreviewDialog Component", () => {
  const mockSprite = {
    id: "test-sprite",
    images: ["icon1", "icon2", "icon3"],
  };

  const mockProps = {
    name: "test-sprite",
    onCloseAction: jest.fn(),
    onDownloadAction: jest.fn(),
    sprite: mockSprite,
  };

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it("displays sprite name in the title", () => {
    render(<SpritePreviewDialog {...mockProps} />);
    expect(screen.getByText("test-sprite")).toBeInTheDocument();
  });

  it("renders download button", () => {
    render(<SpritePreviewDialog {...mockProps} />);
    expect(screen.getByText("Download")).toBeInTheDocument();
  });

  it("calls onDownloadAction when download button is clicked", async () => {
    const user = userEvent.setup();
    render(<SpritePreviewDialog {...mockProps} />);

    const downloadButton = screen.getByRole("button", { name: /download/i });
    await user.click(downloadButton);

    expect(mockProps.onDownloadAction).toHaveBeenCalledWith(mockSprite);
  });

  it("enables download button correctly", () => {
    render(<SpritePreviewDialog {...mockProps} />);

    const downloadButton = screen.getByRole("button", { name: /download/i });
    expect(downloadButton).toBeEnabled();
  });

  it("calls onCloseAction when dialog is closed", async () => {
    const user = userEvent.setup();
    render(<SpritePreviewDialog {...mockProps} />);

    // Find the close button (X button)
    const closeButton = screen.getByRole("button", { name: /close/i });
    await user.click(closeButton);

    expect(mockProps.onCloseAction).toHaveBeenCalled();
  });

  it("renders sprite preview component", () => {
    render(<SpritePreviewDialog {...mockProps} />);

    // Check that the mocked sprite preview is rendered
    expect(screen.getByTestId("sprite-preview")).toBeInTheDocument();

    // Check that sprite items are rendered
    const spriteItems = screen.getAllByTestId("sprite-item");
    expect(spriteItems).toHaveLength(3);
  });
});
