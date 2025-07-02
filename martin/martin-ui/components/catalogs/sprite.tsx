"use client";

import { Download, Eye, ImageIcon, Search } from "lucide-react";
import { ErrorState } from "@/components/error/error-state";
import { CatalogSkeleton } from "@/components/loading/catalog-skeleton";
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
import { useState } from "react";
import type { SpriteCollection } from "@/lib/types";

interface SpriteCatalogProps {
	spriteCollections: SpriteCollection[] | null;
	searchQuery?: string;
	onSearchChangeAction?: (query: string) => void;
	isLoading?: boolean;
	isLoadingSprites?: boolean; // Only used for preview, not for searching
	error?: string | Error | null;
	onRetry?: () => void;
	isRetrying?: boolean;
}


export function SpriteCatalog({
	spriteCollections,
	searchQuery = "",
	onSearchChangeAction = () => {},
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

	const filteredSpriteCollections = (spriteCollections||[]).filter(
		(sprite) =>
			sprite.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
			sprite.description.toLowerCase().includes(searchQuery.toLowerCase())
	);

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
						<Input
							placeholder="Search sprites..."
							className="pl-10 w-64 bg-card"
							value={searchQuery}
							onChange={(e) => onSearchChangeAction(e.target.value)}
						/>
					</div>
				</div>

				<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
					{filteredSpriteCollections.map((sprite: SpriteCollection, index: number) => (
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
											<Eye className="w-4 h-4 mr-2" />
											Preview
										</Button>
									</div>
								</div>
							</CardContent>
						</Card>
					))}
				</div>

				{filteredSpriteCollections.length === 0 && searchQuery && (
					<div className="text-center py-12">
						<p className="text-muted-foreground">
							No sprite collections found matching "{searchQuery}"
						</p>
					</div>
				)}
			</div>

			<SpritePreviewModal
				sprite={selectedSprite}
				onCloseAction={handleSpriteClose}
				onDownloadAction={handleSpriteDownload}
				isLoading={isLoadingSprites}
			/>

			{downloadSprite&&<SpriteDownloadModal sprite={downloadSprite} onCloseAction={handleDownloadClose} />}
		</>
	);
}
