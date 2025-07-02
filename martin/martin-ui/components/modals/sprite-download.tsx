"use client";

import { Copy, CopyCheck } from "lucide-react";
import type React from "react";
import { useState } from "react";
import type { SpriteCollection } from "@/components/catalogs/sprite";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader } from "@/components/ui/dialog";
import { useToast } from "@/hooks/use-toast";

interface SpriteDownloadModalProps {
	sprite: SpriteCollection;
	onCloseAction: () => void;
}

interface SpriteFormat {
	label: string;
	format: "png" | "sdf";
	type: "spritesheet" | "json";
	url: string;
	description: string;
}

export function SpriteDownloadModal({
	sprite,
	onCloseAction,
}: SpriteDownloadModalProps) {
	const open = !!sprite;
	const [copiedUrl, setCopiedUrl] = useState<string | null>(null);
	const { toast } = useToast();
	if (!open) return null;

	// Generate sprite format URLs (these would be real URLs in production)
	const spriteFormats: SpriteFormat[] = sprite
		? [
				{
					label: "PNG Spritesheet",
					format: "png",
					type: "spritesheet",
					url: `/sprites/${sprite.name.toLowerCase().replace(/\s+/g, "-")}.png`,
					description: "Combined image file with all sprites",
				},
				{
					label: "PNG JSON",
					format: "png",
					type: "json",
					url: `/sprites/${sprite.name.toLowerCase().replace(/\s+/g, "-")}.json`,
					description: "Metadata with sprite coordinates and properties",
				},
				{
					label: "SDF Spritesheet",
					format: "sdf",
					type: "spritesheet",
					url: `/sprites/${sprite.name.toLowerCase().replace(/\s+/g, "-")}-sdf.png`,
					description: "SDF-encoded image for scalable rendering",
				},
				{
					label: "SDF JSON",
					format: "sdf",
					type: "json",
					url: `/sprites/${sprite.name.toLowerCase().replace(/\s+/g, "-")}-sdf.json`,
					description: "SDF metadata with rendering parameters",
				},
			]
		: [];

	const handleCopyUrl = async (url: string, label: string) => {
		try {
			// In a real application, this would be the full URL
			const fullUrl = `${window.location.origin}${url}`;
			await navigator.clipboard.writeText(fullUrl);

			setCopiedUrl(url);
			toast({
				title: "URL Copied",
				description: `${label} URL copied to clipboard`,
			});

			// Reset copied state after 2 seconds
			setTimeout(() => {
				setCopiedUrl(null);
			}, 2000);
		} catch {
			toast({
				variant: "destructive",
				title: "Copy Failed",
				description: "Failed to copy URL to clipboard",
			});
		}
	};

	const pngFormats = spriteFormats.filter((f) => f.format === "png");
	const sdfFormats = spriteFormats.filter((f) => f.format === "sdf");

	return (
		<Dialog open={open} onOpenChange={(v: boolean) => !v && onCloseAction()}>
			<DialogHeader>
				<h3 className="text-2xl font-bold">Download {sprite.name}</h3>
				<p className="text-muted-foreground">
					Choose your preferred sprite format
				</p>
			</DialogHeader>
			<DialogContent className="max-w-2xl w-full max-h-[80vh] overflow-auto p-0">
				<div className="mb-8 space-y-6">
					<div className="grid grid-cols-1 md:grid-cols-2 gap-6">
						{/* PNG Format */}
						<div className="p-4 border rounded-lg bg-blue-50 border-blue-200">
							<div className="flex items-center mb-3">
								<Badge
									variant="secondary"
									className="bg-blue-100 text-blue-800 mr-2"
								>
									PNG
								</Badge>
								<h4 className="font-semibold text-blue-900">Standard Format</h4>
							</div>
							<p className="text-sm text-blue-800 mb-4">
								Traditional raster sprites with full color support. Best for
								detailed icons and complex graphics.
							</p>
							<ul className="text-xs text-blue-700 space-y-1">
								<li>• Full color and transparency support</li>
								<li>• Works with all mapping libraries</li>
								<li>• Fixed resolution (may blur when scaled)</li>
								<li>• Larger file sizes</li>
							</ul>
						</div>

						{/* SDF Format */}
						<div className="p-4 border rounded-lg bg-purple-50 border-purple-200">
							<div className="flex items-center mb-3">
								<Badge
									variant="secondary"
									className="bg-purple-100 text-purple-800 mr-2"
								>
									SDF
								</Badge>
								<h4 className="font-semibold text-purple-900">
									Signed Distance Field
								</h4>
							</div>
							<p className="text-sm text-purple-800 mb-4">
								Advanced format for scalable, high-quality rendering at any zoom
								level. Perfect for modern mapping applications.
							</p>
							<ul className="text-xs text-purple-700 space-y-1">
								<li>• Infinite scalability without blur</li>
								<li>• Smaller file sizes</li>
								<li>• Runtime color customization</li>
								<li>• Requires SDF-compatible renderer</li>
							</ul>
						</div>
					</div>

					{/* Download Options */}
					<div className="space-y-6">
						{/* PNG Downloads */}
						<div>
							<h4 className="font-semibold mb-3 text-blue-900 flex items-center">
								<Badge
									variant="secondary"
									className="bg-blue-100 text-blue-800 mr-2"
								>
									PNG
								</Badge>
								Standard Format Downloads
							</h4>
							<div className="space-y-3">
								{pngFormats.map((format) => (
									<div
										key={format.url}
										className="flex items-center justify-between p-3 border rounded-lg hover:bg-gray-50"
									>
										<div className="flex-1">
											<div className="flex items-center mb-1">
												<span className="font-medium">{format.label}</span>
												<Badge variant="outline" className="ml-2 text-xs">
													{format.type}
												</Badge>
											</div>
											<p className="text-sm text-muted-foreground">
												{format.description}
											</p>
										</div>
										<Button
											variant="outline"
											size="sm"
											onClick={() => handleCopyUrl(format.url, format.label)}
											className="ml-4"
										>
											{copiedUrl === format.url ? (
												<>
													<CopyCheck className="h-4 w-4 mr-2 text-green-600" />
													Copied
												</>
											) : (
												<>
													<Copy className="h-4 w-4 mr-2" />
													Copy URL
												</>
											)}
										</Button>
									</div>
								))}
							</div>
						</div>

						{/* SDF Downloads */}
						<div>
							<h4 className="font-semibold mb-3 text-purple-900 flex items-center">
								<Badge
									variant="secondary"
									className="bg-purple-100 text-purple-800 mr-2"
								>
									SDF
								</Badge>
								Signed Distance Field Downloads
							</h4>
							<div className="space-y-3">
								{sdfFormats.map((format) => (
									<div
										key={format.url}
										className="flex items-center justify-between p-3 border rounded-lg hover:bg-gray-50"
									>
										<div className="flex-1">
											<div className="flex items-center mb-1">
												<span className="font-medium">{format.label}</span>
												<Badge variant="outline" className="ml-2 text-xs">
													{format.type}
												</Badge>
											</div>
											<p className="text-sm text-muted-foreground">
												{format.description}
											</p>
										</div>
										<Button
											variant="outline"
											size="sm"
											onClick={() => handleCopyUrl(format.url, format.label)}
											className="ml-4"
										>
											{copiedUrl === format.url ? (
												<>
													<CopyCheck className="h-4 w-4 mr-2 text-green-600" />
													Copied
												</>
											) : (
												<>
													<Copy className="h-4 w-4 mr-2" />
													Copy URL
												</>
											)}
										</Button>
									</div>
								))}
							</div>
						</div>
					</div>

					{/* Footer */}
					<div className="mt-8 pt-4 border-t">
						<div className="flex items-center justify-between">
							<p className="text-sm text-muted-foreground">
								URLs are copied to your clipboard and ready to use in your
								mapping application.
							</p>
							<Button onClick={onCloseAction}>Done</Button>
						</div>
					</div>
				</div>
			</DialogContent>
		</Dialog>
	);
}
