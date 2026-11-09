'use client';

import * as TabsPrimitive from '@radix-ui/react-tabs';
import type * as React from 'react';

import { cn } from '@/lib/utils';

const Tabs = TabsPrimitive.Root;

const TabsList = ({
  className,
  ref,
  ...props
}: React.ComponentPropsWithoutRef<typeof TabsPrimitive.List> & {
  ref?: React.RefObject<React.ElementRef<typeof TabsPrimitive.List> | null>;
}) => (
  <TabsPrimitive.List
    className={cn(
      'inline-flex h-11 items-center justify-center gap-1 rounded-lg bg-muted p-1 text-muted-foreground sm:h-10',
      className,
    )}
    ref={ref}
    {...props}
  />
);
TabsList.displayName = TabsPrimitive.List.displayName;

const TabsTrigger = ({
  className,
  ref,
  ...props
}: React.ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger> & {
  ref?: React.RefObject<React.ElementRef<typeof TabsPrimitive.Trigger> | null>;
}) => (
  <TabsPrimitive.Trigger
    className={cn(
      'inline-flex h-full min-w-0 flex-1 items-center justify-center whitespace-nowrap rounded-md px-3 text-sm font-medium transition-colors cursor-pointer hover:text-foreground disabled:pointer-events-none disabled:opacity-50 data-[state=active]:bg-card data-[state=active]:text-foreground data-[state=active]:shadow-sm',
      className,
    )}
    ref={ref}
    {...props}
  />
);
TabsTrigger.displayName = TabsPrimitive.Trigger.displayName;

const TabsContent = ({
  className,
  ref,
  ...props
}: React.ComponentPropsWithoutRef<typeof TabsPrimitive.Content> & {
  ref?: React.RefObject<React.ElementRef<typeof TabsPrimitive.Content> | null>;
}) => (
  <TabsPrimitive.Content
    className={cn(
      'mt-4',
      className,
    )}
    ref={ref}
    tabIndex={-1}
    {...props}
  />
);
TabsContent.displayName = TabsPrimitive.Content.displayName;

export { Tabs, TabsContent, TabsList, TabsTrigger };
