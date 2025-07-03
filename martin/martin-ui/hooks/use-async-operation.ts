"use client";

import { useCallback, useState } from "react";
import { useToast } from "@/hooks/use-toast";

interface AsyncOperationState<T> {
  data?: T;
  isLoading: boolean;
  error: Error | null;
  isRetrying: boolean;
  retryCount: number;
}

interface AsyncOperationOptions<T> {
  onSuccess?: (data: T) => void;
  onError?: (error: Error) => void;
  showErrorToast?: boolean;
  showSuccessToast?: boolean;
  successMessage?: string;
  maxRetries?: number;
}

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
    maxRetries = 3,
  } = options;

  const { toast } = useToast();
  const [state, setState] = useState<AsyncOperationState<T>>({
    data: undefined,
    error: null,
    isLoading: false,
    isRetrying: false,
    retryCount: 0,
  });

  const execute = useCallback(
    async (isRetry = false) => {
      setState((prev) => ({
        ...prev,
        error: null,
        isLoading: true,
        isRetrying: isRetry,
        retryCount: isRetry ? prev.retryCount + 1 : 0,
      }));

      try {
        const result = await asyncFunction();

        setState((prev) => ({
          ...prev,
          data: result,
          error: null,
          isLoading: false,
          isRetrying: false,
        }));

        onSuccess?.(result);

        if (showSuccessToast) {
          toast({
            description: successMessage || "Operation completed successfully",
            title: "Success",
          });
        }

        return result;
      } catch (error) {
        const err = error instanceof Error ? error : new Error(String(error));

        setState((prev) => ({
          ...prev,
          error: err,
          isLoading: false,
          isRetrying: false,
        }));

        onError?.(err);

        if (showErrorToast && !isRetry) {
          toast({
            description: err.message,
            title: "Error",
            variant: "destructive",
          });
        }

        throw err;
      }
    },
    [asyncFunction, onSuccess, onError, showErrorToast, showSuccessToast, successMessage, toast],
  );

  const retry = useCallback(async () => {
    if (state.retryCount >= maxRetries) {
      toast({
        description: `Failed after ${maxRetries} attempts`,
        title: "Max Retries Reached",
        variant: "destructive",
      });
      return;
    }

    try {
      await execute(true);
    } catch {
      // Error is already handled in execute
    }
  }, [execute, state.retryCount, maxRetries, toast]);

  const reset = useCallback(() => {
    setState({
      data: undefined,
      error: null,
      isLoading: false,
      isRetrying: false,
      retryCount: 0,
    });
  }, []);

  return {
    ...state,
    canRetry: state.retryCount < maxRetries,
    execute,
    reset,
    retry,
  };
}
