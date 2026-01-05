import { Copy } from 'lucide-react';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { Tooltip, TooltipContent, TooltipTrigger } from './tooltip';

/**
 * Props for TooltipCopyText
 * @param text The string to copy to clipboard (required)
 * @param ...props Any other TooltipContent props
 */
export interface TooltipCopyTextProps {
  text: string;
}

export function TooltipCopyText({ text, ...props }: TooltipCopyTextProps) {
  const { copy } = useCopyToClipboard({
    successMessage: `"${text}"`,
  });

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
          <div className="text-xs pt-3 flex flex-row text-slate-400 p-0.5">
            <Copy className="h-3 w-3 mr-2" /> Click to copy
          </div>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
