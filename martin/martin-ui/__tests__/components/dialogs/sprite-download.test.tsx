martin-ui/__tests__/components/dialogs/sprite-download.test.tsx
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import { SpriteDownloadDialog } from "@/components/dialogs/sprite-download";
import { render } from "../../utils/test-utils";

// Mock navigator.clipboard
Object.assign(navigator, {
  clipboard: {
    writeText: jest.fn().mockImplementation(() => Promise.resolve()),
  },
});

// Mock useToast hook
jest.mock("@/hooks/use-toast", () => ({
  useToast: () => ({
    toast: jest.fn(),
  }),
}));

describe("SpriteDownloadDialog Component", () => {
  const mockSprite = {
    id: "test-sprite",
    images: ["icon1", "icon2", "icon3"],
  };

  const mockProps = {
    name: "test-sprite",
    sprite: mockSprite,
    onCloseAction: jest.fn(),
  };

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it("renders correctly", () => {
    const { container } = render(<SpriteDownloadDialog {...mockProps} />);
    expect(container).toMatchSnapshot();
  });

  it("displays the sprite name", () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText("test-sprite")).toBeInTheDocument();
  });

  it("renders the PNG format section", () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText("PNG")).toBeInTheDocument();
    expect(screen.getByText("Standard Format")).toBeInTheDocument();
    expect(
      screen.getByText("Standard sprite format with multiple colors and transparency.")
    ).toBeInTheDocument();
  });

  it("renders the SDF format section", () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText("SDF")).toBeInTheDocument();
    expect(screen.getByText("Signed Distance Field")).toBeInTheDocument();
    expect(screen.getByText("For dynamic coloring at runtime.")).toBeInTheDocument();
  });

  it("lists all download options for PNG format", () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText("PNG JSON")).toBeInTheDocument();
    expect(screen.getByText("PNG Spritesheet")).toBeInTheDocument();
    expect(screen.getByText("High DPI PNG Spritesheet")).toBeInTheDocument();
  });

  it("lists all download options for SDF format", () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText("SDF Spritesheet")).toBeInTheDocument();
    expect(screen.getByText("SDF JSON")).toBeInTheDocument();
    expect(screen.getByText("High DPI SDF Spritesheet")).toBeInTheDocument();
  });

  it("copies URL to clipboard when button is clicked", async () => {
    // Mock window.location.origin
    const originalLocation = window.location;
    delete (window as any).location;
    (window as any).location = { origin: "https://example.com" };

    const user = userEvent.setup();
    render(<SpriteDownloadDialog {...mockProps} />);

    // Find the "Copy URL" button for the PNG JSON format
    const copyButtons = screen.getAllByText("Copy URL");
    const pngJsonCopyButton = copyButtons[0];

    // Click the copy button
    await user.click(pngJsonCopyButton);

    // Check if clipboard.writeText was called with the correct URL
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
      "https://example.com/sprites/test-sprite.json"
    );

    // Button text should change to "Copied"
    await waitFor(() => {
      expect(screen.getByText("Copied")).toBeInTheDocument();
    });

    // Restore window.location
    window.location = originalLocation;
  });

  it("does not render if sprite is null", () => {
    const { container } = render(
      <SpriteDownloadDialog
        name="test-sprite"
        sprite={null as any}
        onCloseAction={mockProps.onCloseAction}
      />
    );
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });

  it("calls onCloseAction when dialog is closed", async () => {
    const user = userEvent.setup();
    const { getByRole } = render(<SpriteDownloadDialog {...mockProps} />);

    // Find and click the close button (X in the dialog)
    const closeButton = getByRole("button", { name: /close/i });
    await user.click(closeButton);

    // Check if onCloseAction was called
    expect(mockProps.onCloseAction).toHaveBeenCalled();
  });
});
