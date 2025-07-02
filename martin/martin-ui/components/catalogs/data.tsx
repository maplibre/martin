"use client";

import {
	Database,
	Eye,
	Globe,
	ImageIcon,
	Layers,
	Palette,
	Search,
	Type,
} from "lucide-react";
import { ErrorState } from "@/components/error/error-state";
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";
import { Badge } from "@/components/ui/badge";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { DisabledNonInteractiveButton } from "../ui/disabledNonInteractiveButton";
import { Tooltip, TooltipContent, TooltipTrigger } from "../ui/tooltip";
import type{ DataSource } from "@/lib/types";

interface DataCatalogProps {
	dataSources: DataSource[];
	searchQuery: string;
	onSearchChangeAction: (query: string) => void;
	isLoading?: boolean;
	error?: Error | null;
	onRetry?: () => void;
	isRetrying?: boolean;
}

export function DataCatalog({
	dataSources,
	searchQuery,
	onSearchChangeAction,
	isLoading = false,
	error = null,
	onRetry,
	isRetrying = false,
}: DataCatalogProps) {
	if (isLoading) {
		return (
			<CatalogSkeleton
				title="Data Sources Catalog"
				description="Manage and explore all available tile sources, sprites, and fonts"
			/>
		);
	}

	if (error) {
		return (
			<ErrorState
				title="Failed to Load Data Catalog"
				description="Unable to fetch data sources from the server"
				error={error}
				onRetry={onRetry}
				isRetrying={isRetrying}
				variant="server"
				showDetails={true}
			/>
		);
	}

	const filteredDataSources = dataSources.filter(
		(source) =>
			source.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
			source.type.toLowerCase().includes(searchQuery.toLowerCase()),
	);

	const getIcon = (type: string) => {
		switch (type) {
			case "Vector Tiles":
				return <Layers className="w-5 h-5 text-primary" />;
			case "Raster Tiles":
				return <ImageIcon className="w-5 h-5 text-primary" />;
			case "Sprites":
				return <Globe className="w-5 h-5 text-primary" />;
			case "Fonts":
				return <Type className="w-5 h-5 text-primary" />;
			default:
				return <Database className="w-5 h-5 text-primary" />;
		}
	};

	return (
		<div className="space-y-6">
			<div className="flex items-center justify-between">
				<div>
					<h2 className="text-2xl font-bold text-foreground">
						Data Sources Catalog
					</h2>
					<p className="text-muted-foreground">
						Manage and explore all available tile sources, sprites, and fonts
					</p>
				</div>
				<div className="relative">
					<Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
					<Input
						placeholder="Search data sources..."
						value={searchQuery}
						onChange={(e) => onSearchChangeAction(e.target.value)}
						className="pl-10 w-64 bg-card"
					/>
				</div>
			</div>

			<div>
				<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
					{filteredDataSources.map((source) => (
						<Card key={source.id} className="hover:shadow-lg transition-shadow">
							<CardHeader>
								<div className="flex items-center justify-between">
									<div className="flex items-center space-x-2">
										{getIcon(source.type)}
										<CardTitle className="text-lg">{source.name}</CardTitle>
									</div>
									<Badge variant="secondary">{source.type}</Badge>
								</div>
								<CardDescription>{source.description}</CardDescription>
							</CardHeader>
							<CardContent>
								<div className="space-y-2 text-sm text-muted-foreground">
									<div className="flex justify-between">
										<span>Layers:</span>
										<span>{source.layers}</span>
									</div>
									<div className="flex justify-between">
										<span>Size:</span>
										<span>{source.sizeBytes} bytes</span>
									</div>
									<div className="flex justify-between">
										<span>Updated:</span>
										<span>{source.lastUpdatedAt.toLocaleDateString()}</span>
									</div>
								</div>
								<div className="flex space-x-2 mt-4">
									<Tooltip>
										<TooltipTrigger className="flex flex-1">
											<DisabledNonInteractiveButton
												size="sm"
												variant="outline"
												className="flex-1 bg-transparent"
											>
												<Eye className="w-4 h-4 mr-2" />
												Inspect
											</DisabledNonInteractiveButton>
										</TooltipTrigger>
										<TooltipContent>
											<p>Not currently implemented in the frontend</p>
										</TooltipContent>
									</Tooltip>
									<Tooltip>
										<TooltipTrigger className="flex flex-1">
											<DisabledNonInteractiveButton
												size="sm"
												className="flex-1"
											>
												<Palette className="w-4 h-4 mr-2" />
												Style
											</DisabledNonInteractiveButton>
										</TooltipTrigger>
										<TooltipContent>
											<p>Not currently implemented in the frontend</p>
										</TooltipContent>
									</Tooltip>
								</div>
							</CardContent>
						</Card>
					))}
				</div>
			</div>

			{filteredDataSources.length === 0 && searchQuery && (
				<div className="text-center py-12">
					<p className="text-muted-foreground">
						No data sources found matching "{searchQuery}"
					</p>
				</div>
			)}
		</div>
	);
}
