import { BookOpen, Github, Info } from 'lucide-react';
import Logo from '@/components/logo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { ThemeSwitcher } from './theme-switcher';

export function Header() {
  return (
    <header className="border-b bg-navbar backdrop-blur-md backdrop-brightness-90 sticky top-0 z-50">
      <div className="container mx-auto px-6">
        <div className="flex h-20 items-center justify-between">
          <div className="flex items-center space-x-4">
            <a className="items-center flex" href="/">
              <Logo className="-mt-5 -rotate-6 md:block hidden" />
              <h1 className="text-3xl md:block hidden -ms-2 font-bold leading-relaxed text-foreground select-none">
                MARTIN
              </h1>
            </a>
            
              <Badge asChild className="hover:bg-purple-700 hidden md:block" variant="default">
                {import.meta.env.VITE_MARTIN_VERSION ? (<a
                  className="p-1"
                  href={`https://github.com/maplibre/martin/releases/tag/${import.meta.env.VITE_MARTIN_VERSION}`}
                >
                  {import.meta.env.VITE_MARTIN_VERSION}
                  </a>
                ) : "dev"}
              </Badge>
          </div>
          <div className="flex items-center space-x-4">
            <HoverCard>
              <HoverCardTrigger asChild>
                <Button asChild size="lg" variant="outline">
                  <a href="https://maplibre.org/martin/" rel="noopener noreferrer" target="_blank">
                    <BookOpen size={18} />
                    <span>Documentation</span>
                  </a>
                </Button>
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
                <Button asChild size="lg" variant="outline">
                  <a href="https://maplibre.org" rel="noopener noreferrer" target="_blank">
                    <Info size={18} />
                    <span>About us</span>
                  </a>
                </Button>
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
            <HoverCard>
              <HoverCardTrigger asChild>
                <Button asChild size="lg-icon" variant="outline">
                  <a
                    aria-label="View source on GitHub"
                    href="https://github.com/maplibre/martin"
                    rel="noopener noreferrer"
                    target="_blank"
                  >
                    <Github size={20} />
                  </a>
                </Button>
              </HoverCardTrigger>
              <HoverCardContent className="w-60">
                <div className="space-y-2">
                  <h4 className="text-sm font-semibold">GitHub Repository</h4>
                  <p className="text-sm">
                    View the source code, discuss issues and contribute to Martin.
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
