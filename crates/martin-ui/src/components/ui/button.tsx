/* eslint-disable react-refresh/only-export-components */
import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import type * as React from 'react';

import { cn } from '@/lib/utils';

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors cursor-pointer disabled:cursor-not-allowed disabled:pointer-events-none disabled:bg-muted disabled:text-muted-foreground disabled:shadow-none disabled:ring-1 disabled:ring-inset disabled:ring-border [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 aria-invalid:border-destructive",
  {
    defaultVariants: {
      size: 'default',
      variant: 'default',
    },
    variants: {
      size: {
        default: 'h-11 px-4 py-2 sm:h-9 has-[>svg]:px-3.5',
        icon: 'size-11 sm:size-9',
        lg: 'h-12 rounded-md px-6 sm:h-10 has-[>svg]:px-4',
        'lg-icon': 'size-11 sm:size-10',
        sm: 'h-11 rounded-md gap-1.5 px-3 sm:h-8 has-[>svg]:px-2.5',
      },
      variant: {
        default: 'bg-primary text-primary-foreground shadow-xs hover:bg-primary-hover',
        destructive: 'bg-destructive text-destructive-foreground shadow-xs hover:opacity-90',
        ghost: 'text-foreground hover:bg-accent hover:text-accent-foreground',
        link: 'text-brand underline-offset-4 hover:underline',
        outline:
          'border border-input bg-card text-foreground shadow-xs hover:bg-accent hover:text-accent-foreground',
        secondary: 'bg-secondary text-secondary-foreground hover:bg-accent',
      },
    },
  },
);
export type ButtonProps = React.ComponentProps<'button'> &
  VariantProps<typeof buttonVariants> & { asChild?: boolean };
function Button({ className, variant, size, asChild = false, ...props }: ButtonProps) {
  const Comp = asChild ? Slot : 'button';

  return (
    <Comp
      className={cn(buttonVariants({ className, size, variant }))}
      data-slot="button"
      {...props}
    />
  );
}

export { Button, buttonVariants };
