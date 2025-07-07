import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";

// Mock the SpritePreview component first before importing anything else
jest.mock("@/components/sprite/SpritePreview", () => ({
  SpritePreview: function MockSpritePreview() {
    return (
      <div data-testid="sprite-preview">
        <div data-testid="sprite-item">icon1</div>
        <div data-testid="sprite-item">icon2</div>
        <div data-testid="sprite-item">icon3</div>
      </div>
    );
  },
}));

// Mock LoadingSpinner component
jest.mock("@/components/loading/loading-spinner", () => ({
  LoadingSpinner: () => <div data-testid="loading-spinner">Loading Spinner Mock</div>,
}));

import { SpritePreviewDialog } from "@/components/dialogs/sprite-preview";

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

    // Check that the sprite preview container is rendered
    const spriteContainer = screen.getByRole("dialog");
    expect(spriteContainer).toBeInTheDocument();

    // Check that sprite items are rendered (look for the actual sprite labels)
    expect(screen.getByText("icon1")).toBeInTheDocument();
    expect(screen.getByText("icon2")).toBeInTheDocument();
    expect(screen.getByText("icon3")).toBeInTheDocument();
  });
});
