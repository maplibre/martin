import { Download, Eye, Search, Type } from "lucide-react";
import { ErrorState } from "@/components/error/error-state";
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
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";

interface Font {
	name: string;
	family: string;
	weight: string;
	format: string;
	size: string;
	usage: string;
	preview: string;
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
		weight: "400",
		format: "TTF",
		size: "156 KB",
		usage: "12,450 requests/day",
		preview: "The quick brown fox jumps over the lazy dog",
	},
	{
		name: "Roboto Bold",
		family: "Roboto",
		weight: "700",
		format: "TTF",
		size: "164 KB",
		usage: "8,230 requests/day",
		preview: "The quick brown fox jumps over the lazy dog",
	},
	{
		name: "Open Sans Regular",
		family: "Open Sans",
		weight: "400",
		format: "WOFF2",
		size: "142 KB",
		usage: "15,680 requests/day",
		preview: "The quick brown fox jumps over the lazy dog",
	},
	{
		name: "Noto Sans CJK",
		family: "Noto Sans",
		weight: "400",
		format: "OTF",
		size: "2.1 MB",
		usage: "3,420 requests/day",
		preview: "漢字 ひらがな カタカナ 한글 中文",
	},
	{
		name: "Source Code Pro",
		family: "Source Code Pro",
		weight: "400",
		format: "TTF",
		size: "198 KB",
		usage: "1,890 requests/day",
		preview: "function() { return 'Hello World'; }",
	},
	{
		name: "Inter Medium",
		family: "Inter",
		weight: "500",
		format: "WOFF2",
		size: "178 KB",
		usage: "9,340 requests/day",
		preview: "The quick brown fox jumps over the lazy dog",
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
					<h2 className="text-2xl font-bold text-gray-900">Font Catalog</h2>
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
									<Type className="w-5 h-5 text-purple-600" />
									<CardTitle className="text-lg">{font.name}</CardTitle>
								</div>
								<Badge variant="secondary">{font.format}</Badge>
							</div>
							<CardDescription>
								Family: {font.family} • Weight: {font.weight}
							</CardDescription>
						</CardHeader>
						<CardContent>
							<div className="space-y-4">
								<div className="p-3 bg-gray-50 rounded-lg">
									<p className="text-sm font-medium mb-2">Preview:</p>
									<p
										className="text-base"
										style={{ fontFamily: font.family, fontWeight: font.weight }}
									>
										{font.preview}
									</p>
								</div>
								<div className="space-y-2 text-sm text-muted-foreground">
									<div className="flex justify-between">
										<span>File Size:</span>
										<span>{font.size}</span>
									</div>
									<div className="flex justify-between">
										<span>Usage:</span>
										<span>{font.usage}</span>
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
									<Button
										size="sm"
										className="flex-1 bg-purple-600 hover:bg-purple-700"
									>
										<Eye className="w-4 h-4 mr-2" />
										Details
									</Button>
								</div>
							</div>
						</CardContent>
					</Card>
				))}
			</div>
		</div>
	);
}
