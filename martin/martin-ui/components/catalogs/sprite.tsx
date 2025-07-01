"use client";

import { Download, Eye, ImageIcon, Search } from "lucide-react";
import { ErrorState } from "@/components/error/error-state";
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";
import { LoadingSpinner } from "@/components/loading/loading-spinner";
import { SpriteDownloadModal } from "@/components/modals/sprite-download";
import { SpritePreviewModal } from "@/components/modals/sprite-preview";
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

interface Sprite {
	name: string;
	id: string;
}

export interface SpriteCollection {
	name: string;
	description: string;
	icons: number;
	sizeInBytes: number;
	usage: string;
	sprites: Sprite[];
}

interface SpriteCatalogProps {
	selectedSprite: SpriteCollection | null;
	onSpriteSelectAction: (sprite: SpriteCollection) => void;
	onSpriteCloseAction: () => void;
	downloadSprite: SpriteCollection | null;
	onDownloadCloseAction: () => void;
	isLoading?: boolean;
	isLoadingSprites?: boolean;
	error?: string | Error | null;
	onRetry?: () => void;
	isRetrying?: boolean;
}

const spriteCollections: SpriteCollection[] = [
	{
		name: "POI Icons",
		description: "Point of interest markers and symbols",
		icons: 156,
		sizeInBytes: 2.3 * 1024 * 1024,
		usage: "45,230 requests/day",
		sprites: [
			{ name: "restaurant", id: "restaurant-icon" },
			{ name: "hotel", id: "hotel-icon" },
			{ name: "gas-station", id: "gas-station-icon" },
			{ name: "hospital", id: "hospital-icon" },
			{ name: "bank", id: "bank-icon" },
			{ name: "atm", id: "atm-icon" },
			{ name: "pharmacy", id: "pharmacy-icon" },
			{ name: "school", id: "school-icon" },
			{ name: "library", id: "library-icon" },
			{ name: "post-office", id: "post-office-icon" },
			{ name: "police", id: "police-icon" },
			{ name: "fire-station", id: "fire-station-icon" },
		],
	},
	{
		name: "Transportation",
		description: "Transit and transportation related icons",
		icons: 89,
		sizeInBytes: 1.8 * 1024 * 1024,
		usage: "32,180 requests/day",
		sprites: [
			{ name: "bus-stop", id: "bus-stop-icon" },
			{ name: "train-station", id: "train-station-icon" },
			{ name: "airport", id: "airport-icon" },
			{ name: "parking", id: "parking-icon" },
			{ name: "subway", id: "subway-icon" },
			{ name: "taxi", id: "taxi-icon" },
			{ name: "bicycle", id: "bicycle-icon" },
			{ name: "car-rental", id: "car-rental-icon" },
		],
	},
	{
		name: "Amenities",
		description: "Public amenities and services",
		icons: 124,
		sizeInBytes: 2.1 * 1024 * 1024,
		usage: "28,450 requests/day",
		sprites: [
			{ name: "wifi", id: "wifi-icon" },
			{ name: "restroom", id: "restroom-icon" },
			{ name: "information", id: "information-icon" },
			{ name: "wheelchair", id: "wheelchair-icon" },
			{ name: "elevator", id: "elevator-icon" },
			{ name: "stairs", id: "stairs-icon" },
			{ name: "drinking-water", id: "drinking-water-icon" },
			{ name: "phone", id: "phone-icon" },
		],
	},
	{
		name: "Recreation",
		description: "Parks, sports, and recreational facilities",
		icons: 67,
		sizeInBytes: 1.4 * 1024 * 1024,
		usage: "18,920 requests/day",
		sprites: [
			{ name: "park", id: "park-icon" },
			{ name: "playground", id: "playground-icon" },
			{ name: "stadium", id: "stadium-icon" },
			{ name: "beach", id: "beach-icon" },
			{ name: "swimming", id: "swimming-icon" },
			{ name: "tennis", id: "tennis-icon" },
			{ name: "golf", id: "golf-icon" },
			{ name: "hiking", id: "hiking-icon" },
		],
	},
	{
		name: "Shopping",
		description: "Retail and commercial establishments",
		icons: 78,
		sizeInBytes: 1.6 * 1024 * 1024,
		usage: "22,340 requests/day",
		sprites: [
			{ name: "shopping-mall", id: "shopping-mall-icon" },
			{ name: "grocery-store", id: "grocery-store-icon" },
			{ name: "clothing-store", id: "clothing-store-icon" },
			{ name: "electronics", id: "electronics-icon" },
			{ name: "bookstore", id: "bookstore-icon" },
			{ name: "flower-shop", id: "flower-shop-icon" },
			{ name: "jewelry", id: "jewelry-icon" },
			{ name: "bakery", id: "bakery-icon" },
		],
	},
	{
		name: "Custom Markers",
		description: "Custom branded location markers",
		icons: 24,
		sizeInBytes: 890 * 1024,
		usage: "12,670 requests/day",
		sprites: [
			{ name: "brand-a-marker", id: "brand-a-marker-icon" },
			{ name: "brand-b-marker", id: "brand-b-marker-icon" },
			{ name: "special-event", id: "special-event-icon" },
			{ name: "promotion", id: "promotion-icon" },
			{ name: "new-location", id: "new-location-icon" },
			{ name: "featured", id: "featured-icon" },
		],
	},
];

export function SpriteCatalog({
	selectedSprite,
	onSpriteSelectAction,
	onSpriteCloseAction,
	downloadSprite,
	onDownloadCloseAction,
	isLoading = false,
	isLoadingSprites = false,
	error = null,
	onRetry,
	isRetrying = false,
}: SpriteCatalogProps) {
	if (isLoading) {
		return (
			<CatalogSkeleton
				title="Sprite Catalog"
				description="Manage and preview all available sprite sheets and icons"
			/>
		);
	}

	if (error) {
		return (
			<ErrorState
				title="Failed to Load Sprites"
				description="Unable to fetch sprite catalog from the server"
				error={error}
				onRetry={onRetry}
				isRetrying={isRetrying}
				variant="server"
				showDetails={true}
			/>
		);
	}

	const handleSpriteSelect = (sprite: SpriteCollection) => {
		onSpriteSelectAction(sprite);
	};

	return (
		<>
			<div className="space-y-6">
				<div className="flex items-center justify-between">
					<div>
						<h2 className="text-2xl font-bold text-foreground">Sprite Catalog</h2>
						<p className="text-muted-foreground">
							Manage and preview all available sprite sheets and icons
						</p>
					</div>
					<div className="relative">
						<Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400 w-4 h-4" />
						<Input placeholder="Search sprites..." className="pl-10 w-64" />
					</div>
				</div>

				<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
					{spriteCollections.map((sprite: SpriteCollection, index: number) => (
						<Card key={index} className="hover:shadow-lg transition-shadow">
							<CardHeader>
								<div className="flex items-center justify-between">
									<div className="flex items-center space-x-2">
										<ImageIcon className="w-5 h-5 text-primary" />
										<CardTitle className="text-lg">{sprite.name}</CardTitle>
									</div>
									<Badge variant="secondary">1x, 2x</Badge>
								</div>
								<CardDescription>{sprite.description}</CardDescription>
							</CardHeader>
							<CardContent>
								<div className="space-y-4">
									<div className="p-3 bg-gray-50 rounded-lg">
										<p className="text-sm font-medium mb-2">Icon Preview:</p>
										<div className="grid grid-cols-8 gap-2">
											{Array.from({ length: Math.min(16, sprite.icons) }).map(
												(_: unknown, i: number) => (
													<div
														key={i}
														className="w-6 h-6 bg-purple-200 rounded flex items-center justify-center"
													>
														<div className="w-4 h-4 bg-primary rounded-sm"></div>
													</div>
												),
											)}
											{sprite.icons > 16 && (
												<div className="w-6 h-6 bg-gray-200 rounded flex items-center justify-center text-xs">
													+{sprite.icons - 16}
												</div>
											)}
										</div>
									</div>
									<div className="space-y-2 text-sm text-muted-foreground">
										<div className="flex justify-between">
											<span>Icons:</span>
											<span>{sprite.icons}</span>
										</div>
										<div className="flex justify-between">
											<span>File Size:</span>
											<span>{sprite.sizeInBytes} bytes</span>
										</div>
										<div className="flex justify-between">
											<span>Usage:</span>
											<span>{sprite.usage}</span>
										</div>
									</div>
									<div className="flex space-x-2">
										<Button
											variant="outline"
											size="sm"
											className="flex-1 bg-transparent"
										>
											<Download className="w-4 h-4 mr-2" />
											Download
										</Button>
										<Button
											variant="default"
											size="sm"
											className="flex-1 bg-primary hover:bg-purple-700"
											onClick={() => handleSpriteSelect(sprite)}
											disabled={isLoadingSprites}
										>
											{isLoadingSprites ? (
												<LoadingSpinner size="sm" className="mr-2" />
											) : (
												<Eye className="w-4 h-4 mr-2" />
											)}
											Preview
										</Button>
									</div>
								</div>
							</CardContent>
						</Card>
					))}
				</div>
			</div>

			<SpritePreviewModal
				sprite={selectedSprite}
				onCloseAction={onSpriteCloseAction}
				onDownloadAction={onSpriteSelectAction}
				isLoading={isLoadingSprites}
			/>

			<SpriteDownloadModal sprite={downloadSprite} onCloseAction={onDownloadCloseAction} />
		</>
	);
}
