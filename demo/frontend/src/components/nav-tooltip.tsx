import type { ReactNode } from 'react';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';

interface NavTooltipProps {
  label: string;
  description: string;
  children: ReactNode;
}

export default function NavTooltip({ label, description, children }: NavTooltipProps) {
  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>{children}</TooltipTrigger>
        <TooltipContent className="max-w-[220px] text-center" side="bottom">
          <p className="font-semibold text-xs mb-0.5">{label}</p>
          <p className="text-xs text-muted-foreground leading-snug">{description}</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
