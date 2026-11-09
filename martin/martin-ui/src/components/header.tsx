import { BookOpen, Code, Info, Menu, X } from 'lucide-react';
import { useState } from 'react';
import Logo from '@/components/logo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { ThemeSwitcher } from './theme-switcher';

export function Header() {
  const version = import.meta.env.VITE_MARTIN_VERSION;
  const [menuOpen, setMenuOpen] = useState(false);
  const links = [
    { href: 'https://maplibre.org/martin/', icon: BookOpen, label: 'Documentation' },
    { href: 'https://maplibre.org', icon: Info, label: 'About MapLibre' },
    { href: 'https://github.com/maplibre/martin', icon: Code, label: 'Source on GitHub' },
  ];
  return (
    <header className="sticky top-0 z-40 border-b bg-navbar backdrop-blur-md">
      <div className="container">
        <div className="flex h-16 items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2 sm:gap-3">
            <a
              aria-label="Martin tile server home"
              className="flex items-center gap-1.5 rounded-md"
              href="/"
            >
              <Logo className="shrink-0" height={34} width={50} />
              <h1 className="select-none text-xl font-extrabold tracking-[0.08em] text-foreground">
                MARTIN
              </h1>
            </a>

            <Badge asChild className="hidden font-mono sm:inline-flex" variant="outline">
              {version ? (
                <a
                  href={`https://github.com/maplibre/martin/releases/tag/martin-v${version}`}
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  {version}
                </a>
              ) : (
                <span>dev</span>
              )}
            </Badge>
          </div>
          <nav aria-label="Project links" className="flex items-center gap-1.5 sm:gap-2">
            <Button
              aria-controls="mobile-menu"
              aria-expanded={menuOpen}
              className="sm:hidden"
              onClick={() => setMenuOpen((open) => !open)}
              size="default"
              variant="ghost"
            >
              {menuOpen ? (
                <X aria-hidden="true" size={16} />
              ) : (
                <Menu aria-hidden="true" size={16} />
              )}
              Menu
            </Button>
            <HoverCard>
              <HoverCardTrigger asChild>
                <Button asChild className="max-sm:hidden" size="default" variant="ghost">
                  <a
                    aria-label="Documentation"
                    href="https://maplibre.org/martin/"
                    rel="noopener noreferrer"
                    target="_blank"
                  >
                    <BookOpen size={16} />
                    <span className="max-sm:sr-only">Documentation</span>
                  </a>
                </Button>
              </HoverCardTrigger>
              <HoverCardContent className="w-80">
                <div className="space-y-1">
                  <h4 className="text-sm font-semibold">Martin Documentation</h4>
                  <p className="text-sm text-muted-foreground">
                    Access comprehensive guides and documentation for the Martin tile server.
                  </p>
                </div>
              </HoverCardContent>
            </HoverCard>
            <HoverCard>
              <HoverCardTrigger asChild>
                <Button
                  asChild
                  className="max-lg:size-11 max-lg:px-0 max-sm:hidden"
                  size="default"
                  variant="ghost"
                >
                  <a
                    aria-label="About MapLibre"
                    href="https://maplibre.org"
                    rel="noopener noreferrer"
                    target="_blank"
                  >
                    <Info size={16} />
                    <span className="max-lg:sr-only">About MapLibre</span>
                  </a>
                </Button>
              </HoverCardTrigger>
              <HoverCardContent className="w-80">
                <div className="space-y-1">
                  <h4 className="text-sm font-semibold">About MapLibre</h4>
                  <p className="text-sm text-muted-foreground">
                    Learn about MapLibre, the open-source collective behind this project.
                  </p>
                </div>
              </HoverCardContent>
            </HoverCard>
            <HoverCard>
              <HoverCardTrigger asChild>
                <Button asChild className="max-sm:hidden" size="lg-icon" variant="ghost">
                  <a
                    aria-label="View source on GitHub"
                    href="https://github.com/maplibre/martin"
                    rel="noopener noreferrer"
                    target="_blank"
                  >
                    <Code size={18} />
                  </a>
                </Button>
              </HoverCardTrigger>
              <HoverCardContent className="w-60">
                <div className="space-y-1">
                  <h4 className="text-sm font-semibold">GitHub Repository</h4>
                  <p className="text-sm text-muted-foreground">
                    View the source code, discuss issues and contribute to Martin.
                  </p>
                </div>
              </HoverCardContent>
            </HoverCard>
            <ThemeSwitcher />
          </nav>
        </div>
        {menuOpen ? (
          <ul className="grid gap-1 border-t py-2 sm:hidden" id="mobile-menu">
            {links.map(({ href, icon: Icon, label }) => (
              <li key={href}>
                <a
                  className="flex min-h-11 items-center gap-3 rounded-md px-2 text-sm font-medium hover:bg-accent"
                  href={href}
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <Icon aria-hidden="true" className="size-4 text-brand" />
                  {label}
                </a>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    </header>
  );
}
