"use client";

import { Copy, CopyCheck } from "lucide-react";
import Link from "next/link";
import type React from "react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { useToast } from "@/hooks/use-toast";
import type { SpriteCollection } from "@/lib/types";

interface SpriteDownloadDialogProps {
	name: string;
	sprite: SpriteCollection;
	onCloseAction: () => void;
}

interface SpriteFormat {
	label: string;
	url: string;
	description: string;
}

export function SpriteDownloadDialog({
	name,
	sprite,
	onCloseAction,
}: SpriteDownloadDialogProps) {
	const [copiedUrl, setCopiedUrl] = useState<string | null>(null);
	const { toast } = useToast();
	if (!sprite) return null;

	// Generate sprite format URLs
	const pngFormats: SpriteFormat[] = [
		{
			label: "PNG JSON",
			url: `/sprites/${name}.json`,
			description: "Sprite coordinates and metadata",
		},
		{
			label: "PNG Spritesheet",
			url: `/sprite/${name}.png`,
			description: "Standard sprite format with full color support",
		},
		{
			label: "High DPI PNG Spritesheet",
			url: `/sprite/${name}@2x.png`,
			description: "High resolution sprites for retina displays",
		},
	];

	const sdfFormats: SpriteFormat[] = [
		{
			label: "SDF Spritesheet",
			url: `/sdf_sprite/${name}.png`,
			description: "For runtime coloring with single color",
		},
		{
			label: "SDF JSON",
			url: `/sdf_sprite/${name}.json`,
			description: "SDF sprite coordinates and metadata",
		},
		{
			label: "High DPI SDF Spritesheet",
			url: `/sdf_sprite/${name}@2x.png`,
			description: "High resolution sprites for retina displays",
		},
	];

	const handleCopyUrl = async (url: string, label: string) => {
		try {
			// In a real application, this would be the full URL
			const fullUrl = `${window.location.origin}${url}`;
			await navigator.clipboard.writeText(fullUrl);

			setCopiedUrl(url);
			toast({
				title: "URL Copied",
				description: `URL of ${label} copied to clipboard`,
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

	return (
		<Dialog
			open={!!sprite}
			onOpenChange={(v: boolean) => !v && onCloseAction()}
		>
			<DialogContent className="max-w-2xl w-full max-h-[90vh] overflow-auto">
				<DialogHeader>
					<DialogTitle className="text-2xl">
						Download <code className="font-mono">{name}</code>
					</DialogTitle>
				</DialogHeader>
				<div className="space-y-6">
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
								Standard sprite format with multiple colors and transparency.
							</p>
							<ul className="text-xs text-blue-700 my-6 ml-6 list-disc [&>li]:mt-2">
								<li>Full color support</li>
								<li>No runtime recoloring</li>
								<li>Compatible with all mapping libraries</li>
								<li>Fixed resolution</li>
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
								For dynamic coloring at runtime.
							</p>
							<ul className="text-xs text-purple-700  my-6 ml-6 list-disc [&>li]:mt-2">
								<li>
									Single color per sprite - Layer multiple SDFs for multi-color icons
								</li>
								<li>
									Customizable color via{" "}
									<code className="bg-purple-200 font-semibold font-monospace text-purple-950 p-0.5 rounded-sm">
										icon-color
									</code>{" "}
									property
								</li>
								<li>Supported by MapLibre and Mapbox</li>
								<li>
									<Link
										href="https://steamcdn-a.akamaihd.net/apps/valve/2007/SIGGRAPH2007_AlphaTestedMagnification.pdf"
										className="text-purple-950 hover:underline"
									>
										SVG-Like
									</Link>{" "}
									zooming
								</li>
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
								Standard Sprites
							</h4>
							<div className="space-y-3">
								{pngFormats.map((format) => (
									<div
										key={format.url}
										className="flex items-center justify-between p-3 border rounded-lg hover:bg-gray-50"
									>
										<div className="flex-1">
											<div className="flex items-center mb-1 font-medium">
												{format.label}
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
								Runtime Colorable Sprites
							</h4>
							<div className="space-y-3">
								{sdfFormats.map((format) => (
									<div
										key={format.url}
										className="flex items-center justify-between p-3 border rounded-lg hover:bg-gray-50"
									>
										<div className="flex-1">
											<div className="flex items-center mb-1 font-medium">
												{format.label}
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
				</div>
			</DialogContent>
		</Dialog>
	);
}
