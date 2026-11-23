import { Search, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';

interface CatalogSearchProps {
  placeholder: string;
  value: string;
  onChange: (query: string) => void;
  className?: string;
}

/** Search box that filters the open catalog, with a button to clear it. */
export function CatalogSearch({ placeholder, value, onChange, className }: CatalogSearchProps) {
  return (
    <div className={cn('relative w-full sm:w-72', className)}>
      <Search
        aria-hidden="true"
        className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        aria-label={placeholder.replace(/\.\.\.$/, '')}
        className="px-9 [&::-webkit-search-cancel-button]:appearance-none"
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        type="search"
        value={value}
      />
      {value ? (
        <Button
          aria-label="Clear search"
          className="absolute right-0.5 top-1/2 size-10 -translate-y-1/2 sm:size-9"
          onClick={() => onChange('')}
          size="icon"
          variant="ghost"
        >
          <X />
        </Button>
      ) : null}
    </div>
  );
}
