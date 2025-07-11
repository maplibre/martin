import { render } from "@testing-library/react";
import { MiniHistogram } from "@/components/charts/mini-histogram";
import type { HistogramData } from "@/lib/prometheus";

describe("MiniHistogram", () => {
	it("renders empty state when no histogram data", () => {
		const { container } = render(<MiniHistogram />);

		const emptyHistogram = container.querySelector(".w-16.h-8.bg-muted\\/20");
		expect(emptyHistogram).toBeInTheDocument();

		const gradient = container.querySelector(".bg-gradient-to-r");
		expect(gradient).toBeInTheDocument();
	});

	it("renders empty state when histogram has no buckets", () => {
		const histogram: HistogramData = {
			buckets: [],
			sum: 0,
			count: 0,
		};

		const { container } = render(<MiniHistogram histogram={histogram} />);

		const emptyHistogram = container.querySelector(".w-16.h-8.bg-muted\\/20");
		expect(emptyHistogram).toBeInTheDocument();
	});

	it("renders histogram bars when data is available", () => {
		const histogram: HistogramData = {
			buckets: [
				{ le: 0.005, count: 100 },
				{ le: 0.01, count: 200 },
				{ le: 0.025, count: 250 },
				{ le: 0.05, count: 280 },
				{ le: 0.1, count: 300 },
			],
			sum: 5.0,
			count: 300,
		};

		const { container } = render(<MiniHistogram histogram={histogram} />);

		// Should render container with bars
		const histogramContainer = container.querySelector(
			".w-16.h-8.flex.items-end",
		);
		expect(histogramContainer).toBeInTheDocument();

		// Should render bars (limited to 8 max, but we have 5)
		const bars = container.querySelectorAll(".flex-1.bg-primary");
		expect(bars).toHaveLength(5);

		// Each bar should have minimum height and opacity
		bars.forEach((bar) => {
			const style = window.getComputedStyle(bar);
			expect(style.minHeight).toBe("2px");
		});
	});

	it("limits bars to maximum of 8", () => {
		const histogram: HistogramData = {
			buckets: Array.from({ length: 12 }, (_, i) => ({
				le: 0.001 * (i + 1),
				count: (i + 1) * 10,
			})),
			sum: 15.0,
			count: 120,
		};

		const { container } = render(<MiniHistogram histogram={histogram} />);

		// Should render only 8 bars even though we have 12 buckets
		const bars = container.querySelectorAll(".flex-1.bg-primary");
		expect(bars).toHaveLength(8);
	});

	it("applies custom className", () => {
		const { container } = render(<MiniHistogram className="custom-class" />);

		const histogramElement = container.firstChild as HTMLElement;
		expect(histogramElement).toHaveClass("custom-class");
	});

	it("calculates bar heights based on bucket distribution", () => {
		const histogram: HistogramData = {
			buckets: [
				{ le: 0.005, count: 50 }, // 50 requests in this bucket (50-0)
				{ le: 0.01, count: 150 }, // 100 more requests (150-50)
				{ le: 0.025, count: 200 }, // 50 more requests (200-150)
			],
			sum: 2.5,
			count: 200,
		};

		const { container } = render(<MiniHistogram histogram={histogram} />);

		const bars = container.querySelectorAll(".flex-1.bg-primary");
		expect(bars).toHaveLength(3);

		// The logic calculates bucket differences, then finds max difference
		// Bucket differences: 50, 100, 50. Max difference is 100.
		// Heights: 50/100=50%, 100/100=100%, 50/100=50%
		const firstBar = bars[0] as HTMLElement;
		const secondBar = bars[1] as HTMLElement;
		const thirdBar = bars[2] as HTMLElement;

		// Second bar should have 100% height (100 is max bucket difference)
		expect(secondBar.style.height).toBe("100%");

		// First and third bars should have 50% height (50/100)
		expect(firstBar.style.height).toBe("50%");
		expect(thirdBar.style.height).toBe("50%");
	});

	it("ensures minimum bar height for visibility", () => {
		const histogram: HistogramData = {
			buckets: [
				{ le: 0.005, count: 1000 }, // Very large bucket
				{ le: 0.01, count: 1001 }, // Only 1 additional request
			],
			sum: 1.0,
			count: 1001,
		};

		const { container } = render(<MiniHistogram histogram={histogram} />);

		const bars = container.querySelectorAll(".flex-1.bg-primary");
		const secondBar = bars[1] as HTMLElement;

		// Even though the second bar represents only 1 request out of 1000,
		// it should still have at least 2% height for visibility
		const height = parseFloat(secondBar.style.height);
		expect(height).toBeGreaterThanOrEqual(2);
	});
});
