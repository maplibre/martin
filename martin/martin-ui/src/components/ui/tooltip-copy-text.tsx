import { useToast } from '@/hooks/use-toast';
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
  const { toast } = useToast();

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      toast({
        description: `"${text}"`,
        title: 'Copied!',
      });
    } catch (err) {
      console.error(`Failed to copy "${text}"`, err);
      toast({
        description: 'Failed to copy to clipboard',
        title: 'Error',
        variant: 'destructive',
      });
    }
  };

  return (
    <Tooltip>
      <TooltipTrigger
        className="text-lg font-mono cursor-pointer truncate w-full"
        onClick={handleCopy}
        type="button"
      >
        <code>{text}</code>
      </TooltipTrigger>
      <TooltipContent {...props}>
        <div className="flex flex-col justify-center items-center">
          <div className="text-xs">{text}</div>
          <br />
          <div className="text-sm">⧉ Click to copy</div>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
