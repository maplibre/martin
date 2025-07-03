import { TooltipProvider } from "@radix-ui/react-tooltip";
import { type RenderOptions, render } from "@testing-library/react";
import { ThemeProvider } from "next-themes";
import type React from "react";
import type { ReactElement } from "react";

const AllTheProviders = ({ children }: { children: React.ReactNode }) => {
	return (
		<ThemeProvider defaultTheme="light" enableSystem attribute="class">
			<TooltipProvider>{children}</TooltipProvider>
		</ThemeProvider>
	);
};

const customRender = (
	ui: ReactElement,
	options?: Omit<RenderOptions, "wrapper">,
) => {
	// Suppress specific warnings during tests
	const originalConsoleError = console.error;
	console.error = (...args) => {
		// Filter out warnings about missing keys and Dialog Description
		const msg = args[0] || '';
		if (
			typeof msg === 'string' &&
			(msg.includes('unique "key" prop') ||
			 msg.includes('Missing `Description`'))
		) {
			return;
		}
		originalConsoleError(...args);
	};

	const result = render(ui, { wrapper: AllTheProviders, ...options });

	// Restore console.error after test
	console.error = originalConsoleError;

	return result;
};

// re-export everything
export * from "@testing-library/react";

// override render method
export { customRender as render };

// This dummy test ensures Jest doesn't complain about an empty test suite
describe("Test Utils", () => {
	it("should have a valid render function", () => {
		expect(customRender).toBeDefined();
	});
});
