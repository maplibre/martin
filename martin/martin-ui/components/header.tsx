"use client";

import { BookOpen, Info } from "lucide-react";
import Image from "next/image";
import Link from "next/link";
import { useTheme } from "next-themes";
import { Badge } from "@/components/ui/badge";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/ui/hover-card";
import { ThemeSwitcher } from "./theme-switcher";
import { Skeleton } from "./ui/skeleton";

export function Header() {
  const { theme } = useTheme();
  return (
    <header className="border-b bg-navbar backdrop-blur-md backdrop-brightness-90 sticky top-0 z-50">
      <div className="container mx-auto px-6">
        <div className="flex h-20 items-center justify-between">
          <div className="flex items-center space-x-4">
            <Link className="items-center flex" href="/">
              {theme ?
              <Image
                alt="Martin Logo"
                className="-mt-5 -rotate-6"
                height={48}
                priority
                src={`/logo_martin-${theme}.svg`}
                title="Martin Tile Server"
                width={96}
              />:<Skeleton className="h-12 w-24" />}
              <h1 className="text-3xl -ms-2 font-bold leading-relaxed text-foreground select-none">
                MARTIN
              </h1>
            </Link>
            <Badge asChild className="hover:bg-purple-700 hidden md:block" variant="default">
              <Link
                className="p-1"
                href={`https://github.com/maplibre/martin/releases/tag/${process.env.NEXT_PUBLIC_VERSION}`}
              >
                {process.env.NEXT_PUBLIC_VERSION}
              </Link>
            </Badge>
          </div>
          <div className="flex items-center space-x-6">
            <HoverCard>
              <HoverCardTrigger asChild>
                <Link
                  className="flex items-center gap-2 text-foreground hover:bg-accent hover:text-accent-foreground px-3 py-2 rounded-md transition-all"
                  href="https://maplibre.org/martin/"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <BookOpen size={18} />
                  <span>Documentation</span>
                </Link>
              </HoverCardTrigger>
              <HoverCardContent className="w-80">
                <div className="space-y-2">
                  <h4 className="text-sm font-semibold">Martin Documentation</h4>
                  <p className="text-sm">
                    Access comprehensive guides and documentation for the Martin tile server.
                  </p>
                </div>
              </HoverCardContent>
            </HoverCard>
            <HoverCard>
              <HoverCardTrigger asChild>
                <Link
                  className="md:flex hidden items-center gap-2 text-foreground hover:bg-accent hover:text-accent-foreground px-3 py-2 rounded-md transition-all"
                  href="https://maplibre.org"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <Info size={18} />
                  <span>About us</span>
                </Link>
              </HoverCardTrigger>
              <HoverCardContent className="w-80">
                <div className="space-y-2">
                  <h4 className="text-sm font-semibold">About MapLibre</h4>
                  <p className="text-sm">
                    Learn about MapLibre,
                    <br />
                    the open-source collective behind this project.
                  </p>
                </div>
              </HoverCardContent>
            </HoverCard>
            <ThemeSwitcher />
          </div>
        </div>
      </div>
    </header>
  );
}
