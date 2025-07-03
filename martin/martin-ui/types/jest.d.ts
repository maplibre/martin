import "@testing-library/jest-dom";

declare global {
	namespace jest {
		interface Matchers<R> {
			toBeInTheDocument(): R;
			toHaveAttribute(attr: string, value?: string): R;
			toBeVisible(): R;
			toBeChecked(): R;
			toBeDisabled(): R;
			toBeEmpty(): R;
			toBeEmptyDOMElement(): R;
			toBeEnabled(): R;
			toBeInvalid(): R;
			toBeRequired(): R;
			toBeValid(): R;
			toContainElement(element: HTMLElement | null): R;
			toContainHTML(htmlText: string): R;
			toHaveClass(...classNames: string[]): R;
			toHaveFocus(): R;
			toHaveFormValues(expectedValues: Record<string, any>): R;
			toHaveStyle(css: string | Record<string, any>): R;
			toHaveTextContent(
				text: string | RegExp,
				options?: { normalizeWhitespace: boolean },
			): R;
			toHaveValue(value?: string | string[] | number): R;
			toBeInTheDOM(): R;
			toHaveDescription(text?: string | RegExp): R;
		}
	}
}
