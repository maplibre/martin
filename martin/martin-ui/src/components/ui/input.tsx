import type * as React from 'react';

import { cn } from '@/lib/utils';

const Input = ({
  className,
  type,
  ref,
  ...props
}: React.ComponentProps<'input'> & { ref?: React.RefObject<HTMLInputElement | null> }) => {
  return (
    <input
      className={cn(
        'flex h-11 w-full rounded-md border border-input bg-card px-3 py-2 text-base shadow-xs transition-colors file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground hover:border-ring/60 disabled:cursor-not-allowed disabled:bg-muted disabled:opacity-60 sm:h-10 sm:text-sm',
        className,
      )}
      ref={ref}
      type={type}
      {...props}
    />
  );
};
Input.displayName = 'Input';

export { Input };
