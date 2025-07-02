import { Brush, Download, Eye, Map, Search } from "lucide-react";
import { ErrorState } from "@/components/error/error-state";
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "../ui/tooltip";
import { DisabledNonInteractiveButton } from "../ui/disabledNonInteractiveButton";

interface Style {
	name: string;
	description: string;
	type: string;
	version: string;
	usage: string;
	layers: number;
	colors: string[];
	lastModified: string;
}

interface StylesCatalogProps {
	isLoading?: boolean;
	error?: Error | null;
	onRetry?: () => void;
	isRetrying?: boolean;
}

const styles: Style[] = [
	{
		name: "OSM Bright",
		description: "Clean and bright OpenStreetMap styling",
		type: "Vector",
		version: "1.2.0",
		usage: "45,230 requests/day",
		layers: 12,
		colors: ["#ffffff", "#f8f8f8", "#e8e8e8", "#4a90e2"],
		lastModified: "2 days ago",
	},
	{
		name: "Dark Theme",
		description: "Modern dark theme for night viewing",
		type: "Vector",
		version: "2.1.0",
		usage: "32,180 requests/day",
		layers: 15,
		colors: ["#1a1a1a", "#2d2d2d", "#404040", "#8b5cf6"],
		lastModified: "1 week ago",
	},
	{
		name: "Satellite Hybrid",
		description: "Satellite imagery with vector overlays",
		type: "Hybrid",
		version: "1.0.3",
		usage: "28,450 requests/day",
		layers: 8,
		colors: ["#2c5234", "#4a7c59", "#8fbc8f", "#ffffff"],
		lastModified: "3 days ago",
	},
	{
		name: "Terrain",
		description: "Topographic style with elevation contours",
		type: "Vector",
		version: "1.5.2",
		usage: "18,920 requests/day",
		layers: 18,
		colors: ["#f4f1de", "#e07a5f", "#3d405b", "#81b29a"],
		lastModified: "5 days ago",
	},
	{
		name: "Minimal",
		description: "Clean minimal style for data visualization",
		type: "Vector",
		version: "1.0.0",
		usage: "22,340 requests/day",
		layers: 6,
		colors: ["#ffffff", "#f5f5f5", "#cccccc", "#666666"],
		lastModified: "1 day ago",
	},
	{
		name: "Retro",
		description: "Vintage-inspired map styling",
		type: "Vector",
		version: "1.3.1",
		usage: "12,670 requests/day",
		layers: 14,
		colors: ["#f7e7ce", "#d4a574", "#8b4513", "#2f4f4f"],
		lastModified: "1 week ago",
	},
];

export function StylesCatalog({
	isLoading = false,
	error = null,
	onRetry,
	isRetrying = false,
}: StylesCatalogProps) {
	if (isLoading) {
		return (
			<CatalogSkeleton
				title="Styles Catalog"
				description="Browse and preview all available map styles and themes"
			/>
		);
	}

	if (error) {
		return (
			<ErrorState
				title="Failed to Load Styles"
				description="Unable to fetch style catalog from the server"
				error={error}
				onRetry={onRetry}
				isRetrying={isRetrying}
				variant="server"
				showDetails={true}
			/>
		);
	}

	return (
		<div className="space-y-6">
			<div className="flex items-center justify-between">
				<div>
					<h2 className="text-2xl font-bold text-foreground">Styles Catalog</h2>
					<p className="text-muted-foreground">
						Browse and preview all available map styles and themes
					</p>
				</div>
				<div className="relative">
					<Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
					<Input placeholder="Search styles..." className="pl-10 w-64" />
				</div>
			</div>

			<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
				{styles.map((style, index) => (
					<Card key={index} className="hover:shadow-lg transition-shadow">
						<CardHeader>
							<div className="flex items-center justify-between">
								<div className="flex items-center space-x-2">
									<Brush className="w-5 h-5 text-primary" />
									<CardTitle className="text-lg">{style.name}</CardTitle>
								</div>
								<Badge variant="secondary">{style.type}</Badge>
							</div>
							<CardDescription>{style.description}</CardDescription>
						</CardHeader>
						<CardContent>
							<div className="space-y-4">
								<div className="p-3 aspect-video rounded-lg bg-gradient-to-br from-gray-200 to-gray-300 flex items-center justify-center relative overflow-hidden">
									<div className="absolute inset-0 opacity-20">
										<div
											className="w-full h-full bg-gradient-to-r"
											style={{
												background: `linear-gradient(45deg, ${style.colors.join(", ")})`,
											}}
										></div>
									</div>
									<Map className="w-8 h-8 text-gray-600 z-10" />
								</div>
								<div className="space-y-2 text-sm text-muted-foreground">
									<div className="flex justify-between">
										<span>Version:</span>
										<span>{style.version}</span>
									</div>
									<div className="flex justify-between">
										<span>Layers:</span>
										<span>{style.layers}</span>
									</div>
									<div className="flex justify-between">
										<span>Usage:</span>
										<span>{style.usage}</span>
									</div>
									<div className="flex justify-between">
										<span>Modified:</span>
										<span>{style.lastModified}</span>
									</div>
								</div>
								<div>
									<p className="text-sm font-medium mb-2">Color Palette:</p>
									<div className="flex space-x-1">
										{style.colors.map((color, i) => (
											<div
												key={i}
												className="w-6 h-6 rounded border border-gray-200"
												style={{ backgroundColor: color }}
												title={color}
											></div>
										))}
									</div>
								</div>
								<div className="flex space-x-2">
									<Button
										size="sm"
										variant="outline"
										className="flex-1 bg-transparent"
									>
										<Download className="w-4 h-4 mr-2" />
										Download
									</Button>

									<Tooltip>
										<TooltipTrigger className="flex flex-1">
											<DisabledNonInteractiveButton
												size="sm"
												className="flex-1"
											>
												<Eye className="w-4 h-4 mr-2" />
												Preview
											</DisabledNonInteractiveButton>
										</TooltipTrigger>
										<TooltipContent>
											<p>Not currently implemented in the frontend</p>
										</TooltipContent>
									</Tooltip>
								</div>
							</div>
						</CardContent>
					</Card>
				))}
			</div>
		</div>
	);
}
