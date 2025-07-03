import { render, screen } from "@testing-library/react";
import type React from "react";
import { ThemeSwitcher } from "@/components/theme-switcher";

// Mock useTheme hook
const mockSetTheme = jest.fn();
let mockTheme = "light";

jest.mock("next-themes", () => ({
	useTheme: () => ({
		theme: mockTheme,
		setTheme: mockSetTheme,
	}),
}));

// Mock UI components to avoid tooltip provider issues
jest.mock("@/components/ui/tooltip", () => ({
	Tooltip: ({ children }: { children: React.ReactNode }) => (
		<div data-testid="tooltip">{children}</div>
	),
	TooltipTrigger: ({ children }: { children: React.ReactNode }) => (
		<div data-testid="tooltip-trigger">{children}</div>
	),
	TooltipContent: ({ children }: { children: React.ReactNode }) => (
		<div data-testid="tooltip-content">{children}</div>
	),
}));

jest.mock("@/components/ui/button", () => ({
	Button: ({
		children,
		...props
	}: {
		children: React.ReactNode;
		[key: string]: any;
	}) => <button {...props}>{children}</button>,
}));

jest.mock("lucide-react", () => ({
	Sun: () => <div data-testid="sun-icon">Sun</div>,
	Moon: () => <div data-testid="moon-icon">Moon</div>,
	SunMoon: () => <div data-testid="sun-moon-icon">SunMoon</div>,
}));

describe("ThemeSwitcher Component", () => {
	it("renders correctly", () => {
		render(<ThemeSwitcher />);
		expect(screen.getByTestId("tooltip-trigger")).toBeInTheDocument();
	});

	it("renders with light theme", () => {
		// The mock is already set to light theme
		render(<ThemeSwitcher />);
		expect(screen.getByTestId("sun-icon")).toBeInTheDocument();
	});

	it("renders with dark theme", () => {
		// Override the mock for this test
		mockTheme = "dark";
		render(<ThemeSwitcher />);
		expect(screen.getByTestId("moon-icon")).toBeInTheDocument();

		// Reset the mock for other tests
		mockTheme = "light";
	});

	it("renders with system theme", () => {
		// Override the mock for this test
		mockTheme = "system";
		render(<ThemeSwitcher />);
		expect(screen.getByTestId("sun-moon-icon")).toBeInTheDocument();

		// Reset the mock for other tests
		mockTheme = "light";
	});
});
