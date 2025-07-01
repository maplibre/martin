import { Download, Eye, Search, Type } from "lucide-react";
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

interface Font {
	name: string;
	family: string;
	weight: number;
	format: "otf" | "ttf" | "ttc";
	sizeInBytes: number;
	usagePerDay: number;
}

interface FontCatalogProps {
	isLoading?: boolean;
	error?: Error | null;
	onRetry?: () => void;
	isRetrying?: boolean;
}

const fonts: Font[] = [
	{
		name: "Roboto Regular",
		family: "Roboto",
		weight: 400,
		format: "ttf",
		sizeInBytes: 156 * 1024,
		usagePerDay: 12450,
	},
	{
		name: "Roboto Bold",
		family: "Roboto",
		weight: 700,
		format: "ttf",
		sizeInBytes: 164 * 1024,
		usagePerDay: 8230,
	},
	{
		name: "Open Sans Regular",
		family: "Open Sans",
		weight: 400,
		format: "ttc",
		sizeInBytes: 142 * 1024,
		usagePerDay: 15680,
	},
	{
		name: "Noto Sans CJK",
		family: "Noto Sans",
		weight: 400,
		format: "otf",
		sizeInBytes: 2.1 * 1024 * 1024,
		usagePerDay: 3420,
	},
	{
		name: "Source Code Pro",
		family: "Source Code Pro",
		weight: 400,
		format: "ttf",
		sizeInBytes: 198 * 1024,
		usagePerDay: 1890,
	},
	{
		name: "Inter Medium",
		family: "Inter",
		weight: 500,
		format: "ttc",
		sizeInBytes: 178 * 1024,
		usagePerDay: 9340,
	},
];

export function FontCatalog({
	isLoading = false,
	error = null,
	onRetry,
	isRetrying = false,
}: FontCatalogProps) {
	if (isLoading) {
		return (
			<CatalogSkeleton
				title="Font Catalog"
				description="Manage and preview all available font assets"
			/>
		);
	}

	if (error) {
		return (
			<ErrorState
				title="Failed to Load Fonts"
				description="Unable to fetch font catalog from the server"
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
					<h2 className="text-2xl font-bold text-foreground">Font Catalog</h2>
					<p className="text-muted-foreground">
						Manage and preview all available font assets
					</p>
				</div>
				<div className="relative">
					<Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
					<Input placeholder="Search fonts..." className="pl-10 w-64" />
				</div>
			</div>

			<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
				{fonts.map((font, index) => (
					<Card key={index} className="hover:shadow-lg transition-shadow">
						<CardHeader>
							<div className="flex items-center justify-between">
								<div className="flex items-center space-x-2">
									<Type className="w-5 h-5 text-primary" />
									<CardTitle className="text-lg">{font.name}</CardTitle>
								</div>
								<Badge variant="secondary" className="uppercase">{font.format}</Badge>
							</div>
							<CardDescription>
								Family: {font.family} • Weight: {font.weight}
							</CardDescription>
						</CardHeader>
						<CardContent>
							<div className="space-y-4">
								<div className="p-3 bg-gray-50 text-gray-900 rounded-lg">
									<p className="text-sm font-medium mb-2 text-gray-900">Preview:</p>
									<p
										className="text-base text-gray-900"
										style={{ fontFamily: font.family, fontWeight: font.weight }}
									>
									  The quick brown fox jumps over the lazy dog
									</p>
								</div>
								<div className="space-y-2 text-sm text-muted-foreground">
									<div className="flex justify-between">
										<span>File Size:</span>
										<span>{font.sizeInBytes} bytes</span>
									</div>
									<div className="flex justify-between">
										<span>Usage:</span>
										<span>{font.usagePerDay} requests/day</span>
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
                    <TooltipTrigger className="flex-1 flex" asChild>
     									<Button
      										size="sm"
      										className="flex-1 grow bg-primary hover:bg-purple-700"
      										disabled
     									>
										<Eye className="w-4 h-4 mr-2" />
										Details
									</Button>
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
	)
}
