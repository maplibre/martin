import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// Mock the useToast hook manually
const mockToast = vi.fn();
const _mockUseToast = vi.fn(() => ({
  toast: mockToast,
}));

// Mock the toast hook
vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({
    toast: mockToast,
  }),
}));

// Import after the mock is set up
const { useAsyncOperation } = await import('@/hooks/use-async-operation');

vi.useFakeTimers();

describe('useAsyncOperation', () => {
  beforeEach(() => {
    // Clear mock history before each test
    vi.clearAllMocks();
  });

  it('should handle successful operation on the first attempt', async () => {
    const mockAsyncFunction = vi.fn().mockResolvedValue('Success Data');
    const onSuccess = vi.fn();

    const { result } = renderHook(() =>
      useAsyncOperation<string>(mockAsyncFunction, { onSuccess }),
    );

    expect(result.current.isLoading).toBe(true);
    expect(result.current.data).toBeUndefined();
    expect(result.current.error).toBeNull();

    let promise: Promise<string | undefined> | undefined;
    act(() => {
      promise = result.current.execute();
    });

    expect(result.current.isLoading).toBe(true);

    await act(async () => {
      if (promise) {
        await promise;
      }
    });

    expect(result.current.isLoading).toBe(false);
    expect(result.current.data).toBe('Success Data');
    expect(result.current.error).toBeNull();
    expect(mockAsyncFunction).toHaveBeenCalledTimes(1);
    expect(onSuccess).toHaveBeenCalledWith('Success Data');
  });

  it('should handle operation failure after all retries', async () => {
    const error = new Error('Failed');
    const mockAsyncFunction = vi.fn().mockRejectedValue(error);
    const onError = vi.fn();

    const { result } = renderHook(() =>
      useAsyncOperation<unknown>(mockAsyncFunction, { maxRetries: 3, onError }),
    );

    let executePromise: Promise<unknown> | undefined;
    act(() => {
      executePromise = result.current.execute();
    });

    // Properly handle the rejected promise to avoid unhandled rejection
    if (executePromise) {
      executePromise.catch(() => {
        // Expected error, do nothing
      });
    }

    await act(async () => {
      // Advance timers to simulate backoff delays
      // First retry after ~500ms
      await vi.advanceTimersByTimeAsync(501);
      // Second retry after ~1000ms
      await vi.advanceTimersByTimeAsync(1001);
      // Wait for the promise to reject
      if (executePromise) {
        await expect(executePromise).rejects.toThrow('Failed');
      }
    });

    expect(result.current.isLoading).toBe(false);
    expect(result.current.data).toBeUndefined();
    expect(result.current.error).toEqual(error);
    expect(mockAsyncFunction).toHaveBeenCalledTimes(3);
    expect(onError).toHaveBeenCalledTimes(3);
  });

  it('should succeed on the second attempt', async () => {
    const successData = 'Success on second try';
    const mockAsyncFunction = vi
      .fn()
      .mockRejectedValueOnce(new Error('Failed first time'))
      .mockResolvedValueOnce(successData);
    const onSuccess = vi.fn();
    const onError = vi.fn();

    const { result } = renderHook(() =>
      useAsyncOperation<string>(mockAsyncFunction, { onError, onSuccess }),
    );

    let promise: Promise<string | undefined> | undefined;
    act(() => {
      promise = result.current.execute();
    });

    await act(async () => {
      // Wait for the first backoff period
      await vi.advanceTimersByTimeAsync(501);
      if (promise) {
        await promise;
      }
    });

    expect(result.current.isLoading).toBe(false);
    expect(result.current.data).toBe(successData);
    expect(result.current.error).toBeNull();
    expect(mockAsyncFunction).toHaveBeenCalledTimes(2);
    expect(onSuccess).toHaveBeenCalledWith(successData);
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it('should show success and error toasts when configured', async () => {
    // Test error toast
    const error = new Error('Toast Test Failed');
    const failingMock = vi.fn().mockRejectedValue(error);

    const { result: errorResult } = renderHook(() =>
      useAsyncOperation<unknown>(failingMock, {
        maxRetries: 1,
        showErrorToast: true,
      }),
    );

    await act(async () => {
      try {
        await errorResult.current.execute();
      } catch (_e) {
        // Expected to throw
      }
    });

    expect(mockToast).toHaveBeenCalledWith({
      description: 'Operation failed after 1 attempts: Toast Test Failed',
      title: 'Error',
      variant: 'destructive',
    });

    mockToast.mockClear();

    // Test success toast
    const successData = 'Toast Test Success';
    const succeedingMock = vi.fn().mockResolvedValue(successData);
    const { result: successResult } = renderHook(() =>
      useAsyncOperation<string>(succeedingMock, {
        showSuccessToast: true,
        successMessage: 'It worked!',
      }),
    );

    await act(async () => {
      await successResult.current.execute();
    });

    expect(mockToast).toHaveBeenCalledWith({
      description: 'It worked!',
      title: 'Success',
    });
  });

  it('should reset the state', async () => {
    const mockAsyncFunction = vi.fn().mockResolvedValue('Some data');
    const { result } = renderHook(() => useAsyncOperation<string>(mockAsyncFunction));

    // Execute to change state
    await act(async () => {
      await result.current.execute();
    });

    expect(result.current.data).toBe('Some data');
    expect(result.current.isLoading).toBe(false);

    // Reset the state
    act(() => {
      result.current.reset();
    });

    expect(result.current.data).toBeUndefined();
    expect(result.current.error).toBeNull();
    expect(result.current.isLoading).toBe(true);
  });
});
