"use client";

import { BookOpen, Info } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { Badge } from "@/components/ui/badge";
import { ThemeSwitcher } from "./theme-switcher";

export function Header() {
	return (
		<header className="border-b bg-navbar backdrop-blur-md backdrop-brightness-90 sticky top-0 z-50">
			<div className="container mx-auto px-6">
				<div className="flex h-20 items-center justify-between">
					<div className="flex items-center space-x-4">
						<div className="items-center flex">
							<Link href="/" className="hidden lg:flex" >
								<Image
									src="/icon.png"
									alt="Logo of the Martin Tileserver"
									title="Martin Tile Server"
									height={9 * 10 - 20}
									width={32 * 10 - 30}
									priority
								/>
							</Link>
							<h1 className="text-3xl font-bold leading-relaxed md:block lg:hidden hidden">MARTIN</h1>
						</div>
							<Badge
  							variant="default"
                className="hover:bg-purple-700 hidden md:block"
                asChild
  						>
                <Link href={`https://github.com/maplibre/martin/releases/tag/${process.env.NEXT_PUBLIC_VERSION}`} className="p-1" >

       							{process.env.NEXT_PUBLIC_VERSION}
                </Link>
  						</Badge>
					</div>
					<div className="flex items-center space-x-6">
						<Link
							href="https://maplibre.org/martin/"
							className="flex items-center gap-2 text-foreground hover:bg-accent hover:text-accent-foreground px-3 py-2 rounded-md transition-all"
							target="_blank"
							rel="noopener noreferrer"
						>
							<BookOpen size={18} />
							<span>Documentation</span>
						</Link>
						<Link
							href="https://maplibre.org"
							className="md:flex hidden items-center gap-2 text-foreground hover:bg-accent hover:text-accent-foreground px-3 py-2 rounded-md transition-all"
							target="_blank"
							rel="noopener noreferrer"
						>
							<Info size={18} />
							<span>About us</span>
						</Link>
						<ThemeSwitcher />
					</div>
				</div>
			</div>
		</header>
	);
}
