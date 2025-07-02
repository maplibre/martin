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
import { DisabledNonInteractiveButton } from "../ui/disabledNonInteractiveButton";
import { Tooltip, TooltipContent, TooltipTrigger } from "../ui/tooltip";
import type { Style } from "@/lib/types";

interface StylesCatalogProps {
	styles?: {[name: string]: Style};
	searchQuery?: string;
	onSearchChangeAction?: (query: string) => void;
	isLoading?: boolean;
	error?: Error | null;
	onRetry?: () => void;
	isRetrying?: boolean;
}

export function StylesCatalog({
	styles,
	searchQuery = "",
	onSearchChangeAction = () => {},
	isLoading,
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

	const filteredStyles = Object.entries(styles ||{}).filter(
		([name,style]) =>
			name.toLowerCase().includes(searchQuery.toLowerCase()) ||
			style.description.toLowerCase().includes(searchQuery.toLowerCase()) ||
			style.type.toLowerCase().includes(searchQuery.toLowerCase()),
	);

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
					<Input
						placeholder="Search styles..."
						className="pl-10 w-64 bg-card"
						value={searchQuery}
						onChange={(e) => onSearchChangeAction(e.target.value)}
					/>
				</div>
			</div>

			<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
				{filteredStyles.map(([name,style]) => (
					<Card key={name} className="hover:shadow-lg transition-shadow">
						<CardHeader>
							<div className="flex items-center justify-between">
								<div className="flex items-center space-x-2">
									<Brush className="w-5 h-5 text-primary" />
									<CardTitle className="text-lg">{name}</CardTitle>
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

			{filteredStyles.length === 0 && searchQuery && (
				<div className="text-center py-12">
					<p className="text-muted-foreground">
						No styles found matching "{searchQuery}"
					</p>
				</div>
			)}
		</div>
	);
}
