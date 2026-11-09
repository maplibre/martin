import { Search, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

interface CatalogHeaderProps {
  title: string;
  description: string;
  searchPlaceholder: string;
  searchQuery: string;
  onSearchChange: (query: string) => void;
  /** Number of entries after filtering, shown next to the description. */
  count?: number;
}

export function CatalogHeader({
  title,
  description,
  searchPlaceholder,
  searchQuery,
  onSearchChange,
  count,
}: CatalogHeaderProps) {
  return (
    <div className="flex flex-col items-stretch justify-between gap-4 md:flex-row md:items-end">
      <div className="min-w-0">
        <h2 className="text-2xl font-semibold tracking-tight text-foreground">{title}</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {description}
          {count === undefined ? null : (
            <span className="ml-2 rounded-full bg-muted px-2 py-0.5 text-xs font-medium tabular-nums text-foreground">
              {count}
            </span>
          )}
        </p>
      </div>
      <div className="relative w-full md:w-72">
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <Input
          aria-label={searchPlaceholder.replace(/\.\.\.$/, '')}
          className="px-9 [&::-webkit-search-cancel-button]:appearance-none"
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder={searchPlaceholder}
          type="search"
          value={searchQuery}
        />
        {searchQuery ? (
          <Button
            aria-label="Clear search"
            className="absolute right-0.5 top-1/2 size-10 -translate-y-1/2 sm:size-9"
            onClick={() => onSearchChange('')}
            size="icon"
            variant="ghost"
          >
            <X />
          </Button>
        ) : null}
      </div>
    </div>
  );
}
