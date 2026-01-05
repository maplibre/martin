import { useCallback, useRef, useState } from 'react';
import { useToast } from '@/hooks/use-toast';
import { copyToClipboard } from '@/lib/utils';

export interface UseCopyToClipboardOptions {
  /** Success message to show in toast (defaults to 'Copied!') */
  successMessage?: string;
  /** Error message to show in toast (defaults to 'Failed to copy to clipboard') */
  errorMessage?: string;
  /** Time in ms before copied state resets (defaults to 2000) */
  resetDelay?: number;
  /** Whether to show success toast (defaults to true) */
  showSuccessToast?: boolean;
  /** Whether to show error toast (defaults to true) */
  showErrorToast?: boolean;
}

export interface UseCopyToClipboardReturn {
  /** Whether text was recently copied */
  copied: boolean;
  /** The last successfully copied text */
  copiedText: string | null;
  /** Function to copy text to clipboard */
  copy: (text: string, customSuccessMessage?: string) => Promise<boolean>;
  /** Reset the copied state */
  reset: () => void;
}

/**
 * A hook that provides clipboard functionality with toast notifications.
 *
 * This hook encapsulates the clipboard copy logic with automatic toast notifications
 * and state management, reducing boilerplate code across components.
 *
 * @example
 * ```tsx
 * const { copy, copied } = useCopyToClipboard();
 *
 * return (
 *   <button onClick={() => copy('text to copy')}>
 *     {copied ? 'Copied!' : 'Copy'}
 *   </button>
 * );
 * ```
 *
 * @example
 * ```tsx
 * // With custom messages
 * const { copy, copied } = useCopyToClipboard({
 *   successMessage: 'URL copied!',
 *   errorMessage: 'Could not copy URL',
 *   resetDelay: 3000,
 * });
 * ```
 */
export function useCopyToClipboard(
  options: UseCopyToClipboardOptions = {},
): UseCopyToClipboardReturn {
  const {
    successMessage = 'Copied!',
    errorMessage = 'Failed to copy to clipboard',
    resetDelay = 2000,
    showSuccessToast = true,
    showErrorToast = true,
  } = options;

  const { toast } = useToast();
  const [copied, setCopied] = useState(false);
  const [copiedText, setCopiedText] = useState<string | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const reset = useCallback(() => {
    // Clear any pending timeout to prevent memory leaks
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
    setCopied(false);
    setCopiedText(null);
  }, []);

  const copy = useCallback(
    async (text: string, customSuccessMessage?: string): Promise<boolean> => {
      try {
        await copyToClipboard(text);
        setCopied(true);
        setCopiedText(text);

        if (showSuccessToast) {
          toast({
            description: customSuccessMessage ?? successMessage,
            title: 'Copied!',
          });
        }

        // Clear any existing timeout before setting a new one
        if (timeoutRef.current) {
          clearTimeout(timeoutRef.current);
        }

        // Auto-reset after delay
        timeoutRef.current = setTimeout(() => {
          setCopied(false);
          setCopiedText(null);
          timeoutRef.current = null;
        }, resetDelay);

        return true;
      } catch (err) {
        console.error('Failed to copy to clipboard:', err);

        if (showErrorToast) {
          toast({
            description: errorMessage,
            title: 'Error',
            variant: 'destructive',
          });
        }

        return false;
      }
    },
    [successMessage, errorMessage, resetDelay, showSuccessToast, showErrorToast, toast],
  );

  return { copied, copiedText, copy, reset };
}
