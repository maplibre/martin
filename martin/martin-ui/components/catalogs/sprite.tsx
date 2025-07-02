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

export interface SpriteCollection {
	name: string;
	description: string;
	sizeInBytes: number;
	requestsPerDay: number;
	sprites: string[];
}

interface SpriteCatalogProps {
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
		sizeInBytes: 23 * 1024 * 1024,
		requestsPerDay: 45230,
		sprites: [
			"restaurant-icon",
			"hotel-icon",
			"gas-station-icon",
			"hospital-icon",
			"bank-icon",
			"atm-icon",
			"pharmacy-icon",
			"school-icon",
			"library-icon",
			"post-office-icon",
			"police-icon",
			"fire-station-icon",
		],
	},
	{
		name: "Transportation",
		description: "Transit and transportation related icons",
		sizeInBytes: 18 * 1024 * 1024,
		requestsPerDay: 32180,
		sprites: [
			"bus-stop-icon",
			"train-station-icon",
			"airport-icon",
			"parking-icon",
			"subway-icon",
			"taxi-icon",
			"bicycle-icon",
			"car-rental-icon",
		],
	},
	{
		name: "Amenities",
		description: "Public amenities and services",
		sizeInBytes: 21 * 1024 * 1024,
		requestsPerDay: 28450,
		sprites: [
			"wifi-icon",
			"restroom-icon",
			"information-icon",
			"wheelchair-icon",
			"elevator-icon",
			"stairs-icon",
			"drinking-water-icon",
			"phone-icon",
		],
	},
	{
		name: "Recreation",
		description: "Parks, sports, and recreational facilities",
		sizeInBytes: 14 * 1024 * 1024,
		requestsPerDay: 18920,
		sprites: [
			"park-icon",
			"playground-icon",
			"stadium-icon",
			"beach-icon",
			"swimming-icon",
			"tennis-icon",
			"golf-icon",
			"hiking-icon",
		],
	},
	{
		name: "Shopping",
		description: "Retail and commercial establishments",
		sizeInBytes: 16 * 1024 * 1024,
		requestsPerDay: 22340,
		sprites: [
			"shopping-mall-icon",
			"grocery-store-icon",
			"clothing-store-icon",
			"electronics-icon",
			"bookstore-icon",
			"flower-shop-icon",
			"jewelry-icon",
			"bakery-icon",
		],
	},
	{
		name: "Custom Markers",
		description: "Custom branded location markers",
		sizeInBytes: 890 * 1024,
		requestsPerDay: 12670,
		sprites: [
			"brand-a-marker-icon",
			"brand-b-marker-icon",
			"special-event-icon",
			"promotion-icon",
			"new-location-icon",
			"featured-icon",
		],
	},
];

import { useState } from "react";

export function SpriteCatalog({
	isLoading = false,
	isLoadingSprites = false,
	error = null,
	onRetry,
	isRetrying = false,
}: SpriteCatalogProps) {
	const [selectedSprite, setSelectedSprite] = useState<SpriteCollection | null>(null);
	const [downloadSprite, setDownloadSprite] = useState<SpriteCollection | null>(null);

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
		setSelectedSprite(sprite);
	};

	const handleSpriteClose = () => {
		setSelectedSprite(null);
	};

	const handleSpriteDownload = (sprite: SpriteCollection) => {
		setDownloadSprite(sprite);
	};

	const handleDownloadClose = () => {
		setDownloadSprite(null);
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
									<div className="p-3 bg-gray-50 rounded-lg text-gray-900">
										<p className="text-sm font-medium mb-2">Icon Preview:</p>
										<div className="grid grid-cols-8 gap-2">
											{sprite.sprites.slice(0,16).map(
												(spriteID) => (
													<div
														key={spriteID}
														className="w-6 h-6 bg-purple-200 rounded flex items-center justify-center"
													>
														<div className="w-4 h-4 bg-primary rounded-sm"></div>
													</div>
												),
											)}
											{sprite.sprites.length > 16 && (
												<div className="w-6 h-6 bg-gray-200 rounded flex items-center justify-center text-xs">
													+{sprite.sprites.length - 16}
												</div>
											)}
										</div>
									</div>
									<div className="space-y-2 text-sm text-muted-foreground">
										<div className="flex justify-between">
											<span>Icons:</span>
											<span>{sprite.sprites.length}</span>
										</div>
										<div className="flex justify-between">
											<span>File Size:</span>
											<span>{sprite.sizeInBytes} bytes</span>
										</div>
										<div className="flex justify-between">
											<span>Usage:</span>
											<span>{sprite.requestsPerDay} requests/day</span>
										</div>
									</div>
									<div className="flex space-x-2">
										<Button
											variant="outline"
											size="sm"
											className="flex-1 bg-transparent"
											onClick={() => handleSpriteDownload(sprite)}
										>
											<Download className="w-4 h-4 mr-2" />
											Download
										</Button>
										<Button
											variant="default"
											size="sm"
											className="flex-1 bg-primary hover:bg-purple-700 text-primary-foreground"
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
				onCloseAction={handleSpriteClose}
				onDownloadAction={handleSpriteDownload}
				isLoading={isLoadingSprites}
			/>

			<SpriteDownloadModal sprite={downloadSprite} onCloseAction={handleDownloadClose} />
		</>
	);
}
