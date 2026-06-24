import type * as React from 'react';

import { cn } from '@/lib/utils';

const Card = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div
    className={cn('rounded-lg border bg-card text-card-foreground shadow-xs', className)}
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
  <div className={cn('flex flex-col space-y-1.5 p-6', className)} ref={ref} {...props} />
);
CardHeader.displayName = 'CardHeader';

const CardTitle = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div
    className={cn('text-2xl font-semibold leading-none tracking-tight', className)}
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
  <div className={cn('p-6 pt-0', className)} ref={ref} {...props} />
);
CardContent.displayName = 'CardContent';

const CardFooter = ({
  className,
  ref,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { ref?: React.RefObject<HTMLDivElement | null> }) => (
  <div className={cn('flex items-center p-6 pt-0', className)} ref={ref} {...props} />
);
CardFooter.displayName = 'CardFooter';

export { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle };
