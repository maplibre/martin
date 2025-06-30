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
import { AnalyticsSkeleton } from "./loading/analytics-skeleton";

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
	if (isLoading) {
		return <AnalyticsSkeleton />;
	}

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
						<Activity className="h-4 w-4 text-purple-600" />
					</CardHeader>
					<CardContent>
						<div className="text-2xl font-bold">
							{serverMetrics.requestsPerSecond.toLocaleString()}
						</div>
						<p className="text-xs text-muted-foreground">+12% from last hour</p>
					</CardContent>
				</Card>
				<Card>
					<CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle className="text-sm font-medium">Memory Usage</CardTitle>
						<Server className="h-4 w-4 text-purple-600" />
					</CardHeader>
					<CardContent>
						<div className="text-2xl font-bold">
							{serverMetrics.memoryUsage}%
						</div>
						<p className="text-xs text-muted-foreground">4.2 GB of 6 GB used</p>
					</CardContent>
				</Card>
				<Card>
					<CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle className="text-sm font-medium">
							Cache Hit Rate
						</CardTitle>
						<Zap className="h-4 w-4 text-purple-600" />
					</CardHeader>
					<CardContent>
						<div className="text-2xl font-bold">
							{serverMetrics.cacheHitRate}%
						</div>
						<p className="text-xs text-muted-foreground">
							Excellent performance
						</p>
					</CardContent>
				</Card>
				<Card>
					<CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle className="text-sm font-medium">
							Active Sources
						</CardTitle>
						<Database className="h-4 w-4 text-purple-600" />
					</CardHeader>
					<CardContent>
						<div className="text-2xl font-bold">
							{serverMetrics.activeSources}
						</div>
						<p className="text-xs text-muted-foreground">All sources healthy</p>
					</CardContent>
				</Card>
			</div>

			{/* Analytics Charts */}
			<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
				<Card>
					<CardHeader>
						<CardTitle>Server Performance</CardTitle>
						<CardDescription>
							Requests per second and memory usage over time
						</CardDescription>
					</CardHeader>
					<CardContent>
						<ChartContainer
							config={{
								requests: {
									label: "Requests/s",
									color: "hsl(var(--chart-1))",
								},
								memory: {
									label: "Memory %",
									color: "hsl(var(--chart-2))",
								},
							}}
							className="h-[200px]"
						>
							<LineChart data={usageData}>
								<CartesianGrid strokeDasharray="3 3" />
								<XAxis dataKey="time" />
								<YAxis />
								<ChartTooltip content={<ChartTooltipContent />} />
								<Line
									type="monotone"
									dataKey="requests"
									stroke="var(--color-requests)"
									strokeWidth={2}
								/>
								<Line
									type="monotone"
									dataKey="memory"
									stroke="var(--color-memory)"
									strokeWidth={2}
								/>
							</LineChart>
						</ChartContainer>
					</CardContent>
				</Card>

				<Card>
					<CardHeader>
						<CardTitle>Tile Source Usage</CardTitle>
						<CardDescription>Request volume by data source</CardDescription>
					</CardHeader>
					<CardContent>
						<ChartContainer
							config={{
								requests: {
									label: "Requests",
									color: "hsl(var(--chart-1))",
								},
							}}
							className="h-[200px]"
						>
							<BarChart data={tileSourcesData} layout="horizontal">
								<CartesianGrid strokeDasharray="3 3" />
								<XAxis type="number" />
								<YAxis dataKey="name" type="category" width={120} />
								<ChartTooltip content={<ChartTooltipContent />} />
								<Bar dataKey="requests" fill="var(--color-requests)" />
							</BarChart>
						</ChartContainer>
					</CardContent>
				</Card>
			</div>
		</div>
	);
}
