import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import type * as React from 'react';

import { cn } from '@/lib/utils';

const badgeVariants = cva(
  'inline-flex items-center justify-center rounded-md border px-2 py-0.5 text-xs font-medium w-fit max-w-full whitespace-nowrap shrink-0 [&>svg]:size-3 gap-1 [&>svg]:pointer-events-none transition-colors overflow-hidden',
  {
    defaultVariants: {
      variant: 'default',
    },
    variants: {
      variant: {
        default: 'border-transparent bg-brand-soft text-brand-soft-foreground [a&]:hover:bg-accent',
        destructive: 'border-transparent bg-destructive text-destructive-foreground',
        outline: 'text-muted-foreground [a&]:hover:bg-accent [a&]:hover:text-accent-foreground',
        secondary: 'border-transparent bg-secondary text-secondary-foreground [a&]:hover:bg-accent',
      },
    },
  },
);

export function Badge({
  className,
  variant,
  asChild = false,
  ...props
}: React.ComponentProps<'span'> & VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : 'span';

  return (
    <Comp className={cn(badgeVariants({ variant }), className)} data-slot="badge" {...props} />
  );
}
