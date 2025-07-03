import userEvent from "@testing-library/user-event";
import React from "react";
import { SpritePreviewDialog } from "@/components/dialogs/sprite-preview";
import { render, screen } from "../../utils/test-utils";

// Mock LoadingSpinner component
jest.mock("@/components/loading/loading-spinner", () => ({
  LoadingSpinner: () => <div data-testid="loading-spinner">Loading Spinner Mock</div>,
}));

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

  it("renders correctly with sprite data", () => {
    const { container } = render(<SpritePreviewDialog {...mockProps} />);
    expect(container).toMatchSnapshot();
  });

  it("displays sprite name in the title", () => {
    render(<SpritePreviewDialog {...mockProps} />);
    expect(screen.getByText("test-sprite")).toBeInTheDocument();
  });

  it("shows loading state when isLoading is true", () => {
    render(<SpritePreviewDialog {...mockProps} isLoading={true} />);
    expect(screen.getByText("Loading sprites...")).toBeInTheDocument();
    expect(screen.getByTestId("loading-spinner")).toBeInTheDocument();
  });

  it("displays sprite icons when not loading", () => {
    render(<SpritePreviewDialog {...mockProps} isLoading={false} />);

    // Check for each sprite name
    expect(screen.getByText("icon1")).toBeInTheDocument();
    expect(screen.getByText("icon2")).toBeInTheDocument();
    expect(screen.getByText("icon3")).toBeInTheDocument();
  });

  it("calls onDownloadAction when download button is clicked", async () => {
    const user = userEvent.setup();
    render(<SpritePreviewDialog {...mockProps} />);

    // Find and click the download button
    const downloadButton = screen.getByRole("button", { name: /download/i });
    await user.click(downloadButton);

    expect(mockProps.onDownloadAction).toHaveBeenCalledWith(mockSprite);
  });

  it("disables download button when loading", () => {
    render(<SpritePreviewDialog {...mockProps} isLoading={true} />);

    const downloadButton = screen.getByRole("button", { name: /download/i });
    expect(downloadButton).toBeDisabled();
  });

  it("calls onCloseAction when dialog is closed", async () => {
    const user = userEvent.setup();
    const { getByRole } = render(<SpritePreviewDialog {...mockProps} />);

    // Find and click the close button (X in the dialog)
    const closeButton = getByRole("button", { name: /close/i });
    await user.click(closeButton);

    expect(mockProps.onCloseAction).toHaveBeenCalled();
  });

  it("shows tooltip content when hovering over sprite item", async () => {
    const user = userEvent.setup();
    render(<SpritePreviewDialog {...mockProps} isLoading={false} />);

    // Find a sprite item to hover over (using its label)
    const spriteItem = screen.getByText("icon1");
    // The button is the TooltipTrigger's parent
    await user.hover(spriteItem.closest("button") || spriteItem);

    // Now the tooltip content should appear (may be multiple tooltips)
    const tooltips = await screen.findAllByText(
      "Sprite preview not currently implemented in the frontend",
    );
    expect(tooltips.length).toBeGreaterThan(0);
  });
});
