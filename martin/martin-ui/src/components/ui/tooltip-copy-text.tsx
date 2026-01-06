import { Copy } from 'lucide-react';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { Tooltip, TooltipContent, TooltipTrigger } from './tooltip';

export interface TooltipCopyTextProps {
  text: string;
}

export function TooltipCopyText({ text, ...props }: TooltipCopyTextProps) {
  // no copied and switching the icon since clicking immediately closes the tooltip
  const { copy } = useCopyToClipboard({ successMessage: `"${text}"` });

  return (
    <Tooltip>
      <TooltipTrigger
        className="text-lg font-mono cursor-pointer truncate w-full"
        onClick={() => copy(text)}
        type="button"
      >
        <code>{text}</code>
      </TooltipTrigger>
      <TooltipContent {...props}>
        <div className="flex flex-col justify-center items-center p-1">
          <div className="text-sm font-mono">{text}</div>
          <div className="text-xs pt-3 flex flex-row gap-1 text-slate-400 p-0.5">
            <Copy className="h-3 w-3 mb-0.5" /> Click to copy
          </div>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
