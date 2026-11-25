import { Copy } from 'lucide-react';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { cn } from '@/lib/utils';
import { Tooltip, TooltipContent, TooltipTrigger } from './tooltip';

export interface TooltipCopyTextProps {
  text: string;
  className?: string;
}

/** Truncated identifier that shows the full value in a tooltip and copies it on click. */
export function TooltipCopyText({ text, className, ...props }: TooltipCopyTextProps) {
  // no copied and switching the icon since clicking immediately closes the tooltip
  const { copy } = useCopyToClipboard({ successMessage: `"${text}"` });

  return (
    <Tooltip>
      <TooltipTrigger
        aria-label={`Copy ${text}`}
        className={cn(
          'block w-full min-w-0 cursor-copy line-clamp-2 rounded-sm [overflow-wrap:anywhere] text-left font-mono text-sm font-medium',
          className,
        )}
        onClick={() => copy(text)}
        type="button"
      >
        <code>{text}</code>
      </TooltipTrigger>
      <TooltipContent className="max-w-[min(24rem,90vw)]" {...props}>
        <div className="flex flex-col gap-2 p-0.5">
          <div className="font-mono text-sm [overflow-wrap:anywhere]">{text}</div>
          <div className="flex items-center gap-1 text-xs text-muted-foreground">
            <Copy className="size-3" /> Click to copy
          </div>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
