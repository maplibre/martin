"use client";

import { BookOpen, Info } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";

export function Header() {
	return (
		<header className="border-b bg-[#1b1c30] backdrop-blur-sm sticky top-0 z-50">
			<div className="container mx-auto px-6">
				<div className="flex items-center justify-between">
					<div className="flex items-center space-x-4">
						<div className="flex items-center">
							<Image
								src="/icon.png"
								alt="Logo of the Martin Tileserver"
								title="Martin Tile Server"
								height={9 * 10}
								width={32 * 10}
							/>
						</div>
						<Badge
							variant="secondary"
							className="bg-purple-100 text-purple-700"
						>
							v0.18.0
						</Badge>
					</div>
					<div className="flex items-center space-x-6">
						<Link
							href="https://maplibre.org/martin/"
							className="flex items-center gap-2 text-gray-200 hover:text-white hover:bg-gray-800 px-3 py-2 rounded-md transition-all"
							target="_blank"
							rel="noopener noreferrer"
						>
							<BookOpen size={18} />
							<span>Documentation</span>
						</Link>
						<Link
							href="https://maplibre.org"
							className="flex items-center gap-2 text-gray-200 hover:text-white hover:bg-gray-800 px-3 py-2 rounded-md transition-all"
							target="_blank"
							rel="noopener noreferrer"
						>
							<Info size={18} />
							<span>About us</span>
						</Link>
					</div>
				</div>
			</div>
		</header>
	);
}
