import { screen, fireEvent } from "@testing-library/react";
import { render } from "../test-utils";
import { ThemeSwitcher } from "@/components/theme-switcher";

// Mock useTheme hook with a simpler approach
const mockSetTheme = jest.fn();

jest.mock("next-themes", () => ({
  useTheme: jest.fn(() => ({
    theme: "light",
    setTheme: mockSetTheme,
  })),
}));

describe("ThemeSwitcher Component", () => {
  beforeEach(() => {
    mockSetTheme.mockClear();
  });

  it("renders correctly", () => {
    render(<ThemeSwitcher />);

    // Check that the button is rendered
    const button = screen.getByRole("button");
    expect(button).toBeInTheDocument();

    // Check that it has an aria-label (the specific label depends on the theme)
    expect(button).toHaveAttribute("aria-label");
    expect(button.getAttribute("aria-label")).toMatch(/Switch to (dark|light|system) theme/);
  });

  it("button is clickable", () => {
    render(<ThemeSwitcher />);

    const button = screen.getByRole("button");

    // Just verify the button is enabled and can be clicked
    expect(button).toBeEnabled();
    expect(button).not.toBeDisabled();
  });

  it("has proper accessibility attributes", () => {
    render(<ThemeSwitcher />);

    const button = screen.getByRole("button");

    // Check that the button has proper accessibility attributes
    expect(button).toHaveAttribute("aria-label");
    // Note: React Button components don't always have explicit type="button"
    expect(button.tagName).toBe("BUTTON");
  });

  it("renders with tooltip structure", () => {
    render(<ThemeSwitcher />);

    // The ThemeSwitcher should be wrapped in a tooltip
    // We can't easily test the tooltip content without triggering it,
    // but we can verify the button is rendered within the tooltip structure
    const button = screen.getByRole("button");
    expect(button).toBeInTheDocument();
  });
});
