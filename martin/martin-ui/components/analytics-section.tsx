import { Activity, Database, Server, Zap } from "lucide-react";
import {
	Bar,
	BarChart,
	CartesianGrid,
	Line,
	LineChart,
	XAxis,
	YAxis,
} from "recharts";
import { ErrorState } from "@/components/error/error-state";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import {
	ChartContainer,
	ChartTooltip,
	ChartTooltipContent,
} from "@/components/ui/chart";
import { Skeleton } from "@/components/ui/skeleton";

interface AnalyticsSectionProps {
	serverMetrics: {
		requestsPerSecond: number;
		memoryUsage: number;
		cacheHitRate: number;
		activeSources: number;
	};
	usageData: Array<{ time: string; requests: number; memory: number }>;
	tileSourcesData: Array<{
		name: string;
		requests: number;
		type: string;
		status: string;
	}>;
	isLoading?: boolean;
	error?: Error | null;
	onRetry?: () => void;
	isRetrying?: boolean;
}

export function AnalyticsSection({
	serverMetrics,
	usageData,
	tileSourcesData,
	isLoading = false,
	error = null,
	onRetry,
	isRetrying = false,
}: AnalyticsSectionProps) {
	if (error) {
		return (
			<div className="mb-8">
				<ErrorState
					title="Failed to Load Analytics"
					description="Unable to fetch server metrics and usage data"
					error={error}
					onRetry={onRetry}
					isRetrying={isRetrying}
					variant="server"
					showDetails={true}
				/>
			</div>
		);
	}

	return (
		<div className="space-y-6 mb-8">
			{/* Server Status Cards */}
			<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
				<Card>
					<CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle className="text-sm font-medium">
							Requests/Second
						</CardTitle>
						<Activity className="h-4 w-4 text-primary" />
					</CardHeader>
					<CardContent>
						{isLoading ? (
							<>
								<Skeleton className="h-8 w-16 mb-2" />
								<Skeleton className="h-3 w-32" />
							</>
						) : (
							<>
								<div className="text-2xl font-bold">
									{serverMetrics.requestsPerSecond.toLocaleString()}
								</div>
								<p className="text-xs text-muted-foreground">
									+12% from last hour
								</p>
							</>
						)}
					</CardContent>
				</Card>
				<Card>
					<CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle className="text-sm font-medium">Memory Usage</CardTitle>
						<Server className="h-4 w-4 text-primary" />
					</CardHeader>
					<CardContent>
						{isLoading ? (
							<>
								<Skeleton className="h-8 w-16 mb-2" />
								<Skeleton className="h-3 w-32" />
							</>
						) : (
							<>
								<div className="text-2xl font-bold">
									{serverMetrics.memoryUsage}%
								</div>
								<p className="text-xs text-muted-foreground">
									4.2 GB of 6 GB used
								</p>
							</>
						)}
					</CardContent>
				</Card>
				<Card>
					<CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle className="text-sm font-medium">
							Cache Hit Rate
						</CardTitle>
						<Zap className="h-4 w-4 text-primary" />
					</CardHeader>
					<CardContent>
						{isLoading ? (
							<>
								<Skeleton className="h-8 w-16 mb-2" />
								<Skeleton className="h-3 w-32" />
							</>
						) : (
							<>
								<div className="text-2xl font-bold">
									{serverMetrics.cacheHitRate}%
								</div>
								<p className="text-xs text-muted-foreground">
									Excellent performance
								</p>
							</>
						)}
					</CardContent>
				</Card>
				<Card>
					<CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle className="text-sm font-medium">
							Active Sources
						</CardTitle>
						<Database className="h-4 w-4 text-primary" />
					</CardHeader>
					<CardContent>
						{isLoading ? (
							<>
								<Skeleton className="h-8 w-16 mb-2" />
								<Skeleton className="h-3 w-32" />
							</>
						) : (
							<>
								<div className="text-2xl font-bold">
									{serverMetrics.activeSources}
								</div>
								<p className="text-xs text-muted-foreground">
									All sources healthy
								</p>
							</>
						)}
					</CardContent>
				</Card>
			</div>
		</div>
	);
}
