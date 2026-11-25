import type * as React from 'react';

import { cn } from '@/lib/utils';

const Card = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div
    className={cn('rounded-xl border bg-card text-card-foreground shadow-card', className)}
    ref={ref}
    {...props}
  />
);
Card.displayName = 'Card';

const CardHeader = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div className={cn('flex flex-col gap-1.5 p-5', className)} ref={ref} {...props} />
);
CardHeader.displayName = 'CardHeader';

const CardTitle = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div
    className={cn('text-base font-semibold leading-snug tracking-tight', className)}
    ref={ref}
    {...props}
  />
);
CardTitle.displayName = 'CardTitle';

const CardDescription = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div className={cn('text-sm text-muted-foreground', className)} ref={ref} {...props} />
);
CardDescription.displayName = 'CardDescription';

const CardContent = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div className={cn('p-5 pt-0', className)} ref={ref} {...props} />
);
CardContent.displayName = 'CardContent';

const CardFooter = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div className={cn('flex items-center p-5 pt-0', className)} ref={ref} {...props} />
);
CardFooter.displayName = 'CardFooter';

export { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle };
