import type { LucideIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface CatalogEmptyProps {
  icon: LucideIcon;
  message: string;
  linkHref: string;
  linkLabel: string;
}

export function CatalogEmpty({ icon: Icon, message, linkHref, linkLabel }: CatalogEmptyProps) {
  return (
    <div className="tile-grid flex flex-col items-center gap-2 rounded-xl border border-dashed px-6 py-14 text-center">
      <div className="flex size-11 items-center justify-center rounded-full bg-card text-brand shadow-card">
        <Icon aria-hidden="true" className="size-5" />
      </div>
      <p className="text-sm font-medium text-foreground">{message}</p>
      <Button asChild size="sm" variant="link">
        <a href={linkHref} rel="noopener noreferrer" target="_blank">
          {linkLabel}
        </a>
      </Button>
    </div>
  );
}
