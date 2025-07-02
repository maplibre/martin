"use client";

import { Download } from "lucide-react";
import type React from "react";
import { LoadingSpinner } from "@/components/loading/loading-spinner";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";

import type { SpriteCollection } from "../catalogs/sprite";

interface SpritePreviewModalProps {
	sprite: SpriteCollection | null;
	onCloseAction: () => void;
	onDownloadAction: (sprite: SpriteCollection) => void;
	isLoading?: boolean;
}

export function SpritePreviewModal({
	sprite,
	onDownloadAction,
	onCloseAction,
	isLoading = false,
}: SpritePreviewModalProps) {
	const open = !!sprite;

	return (
		<Dialog open={open} onOpenChange={(v) => !v && onCloseAction()}>
			<DialogContent className="max-w-4xl w-full max-h-[80vh] overflow-auto p-0">
				{sprite && (
					<div className="p-6">
						<DialogHeader>
							<DialogTitle className="text-2xl">{sprite.name}</DialogTitle>
							<DialogDescription>{sprite.description}</DialogDescription>
						</DialogHeader>
						<div className="flex items-center justify-end mb-4 gap-2">
							<Button
								variant="outline"
								size="sm"
								onClick={() => onDownloadAction(sprite)}
								disabled={isLoading}
							>
								<Download className="h-4 w-4 mr-2" />
								Download
							</Button>
							<Button
								variant="outline"
								size="sm"
								onClick={onCloseAction}
								disabled={isLoading}
							>
								Close
							</Button>
						</div>
						{isLoading ? (
							<div className="space-y-4">
								<div className="flex items-center justify-center py-8">
									<div className="text-center">
										<LoadingSpinner size="lg" className="mx-auto mb-4" />
										<p className="text-muted-foreground">Loading sprites...</p>
									</div>
								</div>
								<div className="grid grid-cols-4 md:grid-cols-6 lg:grid-cols-8 gap-4">
									{Array.from({ length: 24 }).map((_, i) => (
										<div
											key={i}
											className="flex flex-col items-center p-3 border rounded-lg"
										>
											<Skeleton className="w-12 h-12 mb-2" />
											<Skeleton className="h-3 w-16" />
										</div>
									))}
								</div>
							</div>
						) : (
							<div>
								<h4 className="font-medium mb-4">
									Sprite Preview ({sprite.sprites.length} icons)
								</h4>
								<div className="grid grid-cols-4 md:grid-cols-6 lg:grid-cols-8 gap-4 max-h-96 overflow-y-auto">
									{sprite.sprites.map((spriteItem) => (
										<div
											key={spriteItem}
											className="flex flex-col items-center p-3 border rounded-lg hover:bg-gray-50 transition-colors"
										>
											<div className="w-12 h-12 bg-purple-200 rounded flex items-center justify-center mb-2">
												<div className="w-8 h-8 bg-primary rounded-sm"></div>
											</div>
											<span className="text-xs text-center font-medium break-words">
												{spriteItem}
											</span>
										</div>
									))}
								</div>
							</div>
						)}
					</div>
				)}
			</DialogContent>
		</Dialog>
	);
}
