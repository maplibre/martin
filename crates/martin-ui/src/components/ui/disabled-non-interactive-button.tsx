import { Slot } from '@radix-ui/react-slot';
import type { VariantProps } from 'class-variance-authority';
import type * as React from 'react';

import { cn } from '@/lib/utils';
import { buttonVariants } from './button';

/** Looks clearly unavailable (muted, hatched ring) and is exposed as aria-disabled. */
export function DisabledNonInteractiveButton({
  className,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<'button'> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  }) {
  const Comp = asChild ? Slot : 'span';

  return (
    <Comp
      aria-disabled="true"
      className={cn(
        buttonVariants({ size, variant: 'secondary' }),
        'cursor-not-allowed bg-muted text-muted-foreground ring-1 ring-inset ring-border hover:bg-muted',
        className,
      )}
      {...props}
    />
  );
}
