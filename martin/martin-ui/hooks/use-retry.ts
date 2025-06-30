"use client";

import { useCallback, useState } from "react";
import { useToast } from "@/hooks/use-toast";

interface RetryOptions {
	maxAttempts?: number;
	baseDelay?: number;
	maxDelay?: number;
	backoffFactor?: number;
	onError?: (error: Error, attempt: number) => void;
	onSuccess?: () => void;
	onMaxAttemptsReached?: (error: Error) => void;
}

interface RetryState {
	isRetrying: boolean;
	attempt: number;
	lastError: Error | null;
}

export function useRetry<T>(
	asyncFunction: () => Promise<T>,
	options: RetryOptions = {},
) {
	const {
		maxAttempts = 3,
		baseDelay = 1000,
		maxDelay = 10000,
		backoffFactor = 2,
		onError,
		onSuccess,
		onMaxAttemptsReached,
	} = options;

	const { toast } = useToast();
	const [state, setState] = useState<RetryState>({
		isRetrying: false,
		attempt: 0,
		lastError: null,
	});

	const calculateDelay = useCallback(
		(attempt: number) => {
			const delay = Math.min(baseDelay * backoffFactor ** attempt, maxDelay);
			// Add jitter to prevent thundering herd
			return delay + Math.random() * 1000;
		},
		[baseDelay, backoffFactor, maxDelay],
	);

	const retry = useCallback(async (): Promise<T | null> => {
		setState((prev) => ({ ...prev, isRetrying: true, attempt: 0 }));

		for (let attempt = 0; attempt < maxAttempts; attempt++) {
			try {
				setState((prev) => ({ ...prev, attempt: attempt + 1 }));
				const result = await asyncFunction();

				setState({
					isRetrying: false,
					attempt: 0,
					lastError: null,
				});

				onSuccess?.();
				return result;
			} catch (error) {
				const err = error instanceof Error ? error : new Error(String(error));
				setState((prev) => ({ ...prev, lastError: err }));

				onError?.(err, attempt + 1);

				if (attempt === maxAttempts - 1) {
					setState((prev) => ({ ...prev, isRetrying: false }));
					onMaxAttemptsReached?.(err);

					toast({
						variant: "destructive",
						title: "Operation Failed",
						description: `Failed after ${maxAttempts} attempts: ${err.message}`,
					});

					return null;
				}

				// Wait before next attempt
				const delay = calculateDelay(attempt);
				await new Promise((resolve) => setTimeout(resolve, delay));
			}
		}

		return null;
	}, [
		asyncFunction,
		maxAttempts,
		calculateDelay,
		onError,
		onSuccess,
		onMaxAttemptsReached,
		toast,
	]);

	const reset = useCallback(() => {
		setState({
			isRetrying: false,
			attempt: 0,
			lastError: null,
		});
	}, []);

	return {
		...state,
		retry,
		reset,
	};
}
