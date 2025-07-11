import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import type { HistogramBucket, HistogramData } from "@/lib/prometheus";
import { cn } from "@/lib/utils";

interface MiniHistogramProps {
	histogram?: HistogramData;
	className?: string;
}

const formatDuration = (seconds: number): string => {
	if (seconds < 1) {
		return `${Math.round(seconds * 1000)}ms`;
	}
	return `${seconds}s`;
};

interface RebucketedHistogram {
	height: number;
	opacity: number;
	timeRange: string;
	requestCount: number;
}

// Calculate bar heights based on the distribution between buckets
// Histogram buckets are cumulative (le = "less than or equal"), so we need differences
function rebucket(buckets: HistogramBucket[]): RebucketedHistogram[] {
	// First, calculate bucket differences (actual requests in each bucket range)
	const bucketDifferences = [];
	const bucketRanges = [];
	let maxBucketDiff = 1; // minimum of 1 to avoid division by zero
	let prevCount = 0;
	for (let i = 0; i < buckets.length; i++) {
		const bucket = buckets[i];
		const bucketCount = bucket.count - prevCount; // This gives us actual requests in this bucket
		prevCount = bucket.count;
		// Calculate bucket range (from, to]
		const fromValue = i > 0 ? buckets[i - 1].le : 0;
		const toValue = bucket.le;

		bucketDifferences.push(bucketCount);
		maxBucketDiff = Math.max(maxBucketDiff, bucketCount);
		bucketRanges.push({
			timeRange: `${fromValue === 0 ? "[" : "("}${formatDuration(fromValue)}, ${formatDuration(toValue)}]`,
			requestCount: bucketCount,
		});
	}

	// Create bars based on bucket differences
	const bars = [] as RebucketedHistogram[];
	for (let i = 0; i < bucketDifferences.length; i++) {
		const bucketCount = bucketDifferences[i];
		const height = (bucketCount / maxBucketDiff) * 100;

		bars.push({
			height: Math.max(height, 10), // Minimum for visibility
			opacity: 0.2 + (height / 100) * 0.8,
			...bucketRanges[i],
		});
	}
	return bars;
}

export function MiniHistogram({
	histogram,
	className = "",
}: MiniHistogramProps) {
	if (!histogram || !histogram.buckets || histogram.buckets.length === 0) {
		return (
			<div
				className={cn(
					"w-20 h-12 bg-muted/10 rounded-md opacity-40 animate-pulse bg-gradient-to-r from-transparent to-muted",
					className,
				)}
			></div>
		);
	}
	const bars = rebucket(histogram.buckets);

	return (
		<TooltipProvider>
			<div className={cn("w-20 h-12 flex items-end gap-px", className)}>
				{bars.map((bar) => (
					<Tooltip key={bar.timeRange}>
						<TooltipTrigger asChild>
							<div
								className="flex-1 bg-primary rounded-[1px] cursor-default"
								style={{
									height: `${bar.height}%`,
									opacity: bar.opacity,
								}}
							/>
						</TooltipTrigger>
						<TooltipContent>
							<div className="text-xs">
								<div className="font-medium">
									{bar.requestCount.toLocaleString()} requests
								</div>
								<div className="text-muted-foreground">{bar.timeRange}</div>
							</div>
						</TooltipContent>
					</Tooltip>
				))}
			</div>
		</TooltipProvider>
	);
}
