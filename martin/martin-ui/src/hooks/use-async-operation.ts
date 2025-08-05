'use client';

import { useCallback, useState } from 'react';
import { useToast } from '@/hooks/use-toast';

// A helper function to introduce a delay, which is useful for backoff strategies.
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface AsyncOperationState<T> {
  data?: T;
  isLoading: boolean;
  error: Error | null;
}

interface AsyncOperationOptions<T> {
  /** Callback function that is triggered on a successful operation. */
  onSuccess?: (data: T) => void;
  /** Callback function that is triggered on a failed attempt. */
  onError?: (error: Error, attempt: number) => void;
  /** Whether to show a toast notification on error. Defaults to `true`. */
  showErrorToast?: boolean;
  /** Whether to show a toast notification on success. Defaults to `false`. */
  showSuccessToast?: boolean;
  /** Custom message for the success toast. */
  successMessage?: string;
  /** The maximum number of attempts, including the initial one. Defaults to 3. */
  maxRetries?: number;
  /** The initial delay in milliseconds for exponential backoff. Defaults to 500. */
  initialBackoffDelay?: number;
}

/**
 * A custom hook to manage asynchronous operations with automatic retries and exponential backoff.
 *
 * @param asyncFunction The asynchronous function to execute.
 * @param options Configuration options for the operation.
 * @returns An object with the operation's state (`data`, `isLoading`, `error`) and control functions (`execute`, `reset`).
 */
export function useAsyncOperation<T>(
  asyncFunction: () => Promise<T>,
  options: AsyncOperationOptions<T> = {},
) {
  const {
    onSuccess,
    onError,
    showErrorToast = true,
    showSuccessToast = false,
    successMessage,
    maxRetries = 10,
    initialBackoffDelay = 500,
  } = options;

  const { toast } = useToast();
  const [state, setState] = useState<AsyncOperationState<T>>({
    data: undefined,
    error: null,
    isLoading: true,
  });

  const execute = useCallback(async () => {
    setState((prev) => ({ ...prev, error: null }));
    // we set it to loading after 2s, while preserving the intial (loaded, unloaded) state
    const timeout = setTimeout(() => setState((prev) => ({ ...prev, isLoading: true })), 2000);

    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      try {
        const result = await asyncFunction();

        clearTimeout(timeout);
        setState({
          data: result,
          error: null,
          isLoading: false,
        });

        onSuccess?.(result);

        if (showSuccessToast) {
          toast({
            description: successMessage || 'Operation completed successfully.',
            title: 'Success',
          });
        }

        return result;
      } catch (error) {
        const err = error instanceof Error ? error : new Error(String(error));

        onError?.(err, attempt);

        if (attempt >= maxRetries) {
          setState({ data: undefined, error: err, isLoading: false });

          if (showErrorToast) {
            toast({
              description: `Operation failed after ${maxRetries} attempts: ${err.message}`,
              title: 'Error',
              variant: 'destructive',
            });
          }

          throw err;
        }

        const backoffDelay = initialBackoffDelay ** (attempt - 1);
        const jitter = backoffDelay * 0.2 * Math.random();
        console.log(`retrying in ${backoffDelay}ms with jitter ${jitter}ms`);
        await sleep(backoffDelay + jitter);
      }
    }
  }, [
    asyncFunction,
    maxRetries,
    initialBackoffDelay,
    onSuccess,
    onError,
    showSuccessToast,
    successMessage,
    showErrorToast,
    toast,
  ]);

  const reset = useCallback(() => {
    setState({ data: undefined, error: null, isLoading: true });
  }, []);

  return { ...state, execute, reset };
}
