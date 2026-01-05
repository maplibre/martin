import { useCallback, useEffect, useRef, useState } from 'react';
import { useToast } from '@/hooks/use-toast';
import { copyToClipboard } from '@/lib/utils';

const RESET_DELAY = 2000;
const ERROR_MESSAGE = 'Failed to copy to clipboard';

interface UseCopyToClipboardOptions {
  successMessage?: string;
}

interface UseCopyToClipboardReturn {
  copied: boolean;
  copiedText: string | null;
  copy: (text: string, customSuccessMessage?: string) => Promise<boolean>;
}

/** Hook for clipboard operations with toast notifications and auto-reset state. */
export function useCopyToClipboard(
  options: UseCopyToClipboardOptions = {},
): UseCopyToClipboardReturn {
  const { successMessage = 'Copied!' } = options;
  const { toast } = useToast();
  const [copied, setCopied] = useState(false);
  const [copiedText, setCopiedText] = useState<string | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, []);

  const copy = useCallback(
    async (text: string, customSuccessMessage?: string): Promise<boolean> => {
      try {
        await copyToClipboard(text);
        setCopied(true);
        setCopiedText(text);

        toast({
          description: customSuccessMessage ?? successMessage,
          title: 'Copied!',
        });

        if (timeoutRef.current) clearTimeout(timeoutRef.current);
        timeoutRef.current = setTimeout(() => {
          setCopied(false);
          setCopiedText(null);
          timeoutRef.current = null;
        }, RESET_DELAY);

        return true;
      } catch (err) {
        console.error('Failed to copy to clipboard:', err);
        toast({
          description: ERROR_MESSAGE,
          title: 'Error',
          variant: 'destructive',
        });
        return false;
      }
    },
    [successMessage, toast],
  );

  return { copied, copiedText, copy };
}
