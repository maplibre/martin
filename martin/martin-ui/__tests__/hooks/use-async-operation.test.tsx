import { act, renderHook } from "@testing-library/react";
import { useAsyncOperation } from "../../hooks/use-async-operation";

// Mock useToast to avoid side effects and track calls
jest.mock("../../hooks/use-toast", () => ({
  useToast: () => ({
    toast: jest.fn(),
  }),
}));

describe("useAsyncOperation", () => {
  it("should execute and succeed", async () => {
    const asyncFn = jest.fn().mockResolvedValue("data");
    const onSuccess = jest.fn();
    const { result } = renderHook(() => useAsyncOperation(asyncFn, { maxRetries: 2, onSuccess }));

    let value: string | undefined;
    await act(async () => {
      value = await result.current.execute();
    });

    expect(asyncFn).toHaveBeenCalledTimes(1);
    expect(onSuccess).toHaveBeenCalledWith("data");
    expect(result.current.data).toBe("data");
    expect(result.current.isLoading).toBe(false);
    expect(result.current.error).toBeNull();
    expect(result.current.retryCount).toBe(0);
    expect(result.current.isRetrying).toBe(false);
  });

  it("should handle error and call onError", async () => {
    const asyncFn = jest.fn().mockRejectedValue(new Error("fail"));
    const onError = jest.fn();
    const { result } = renderHook(() => useAsyncOperation(asyncFn, { maxRetries: 2, onError }));

    await act(async () => {
      try {
        await result.current.execute();
      } catch {}
    });

    expect(asyncFn).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalled();
    expect(result.current.data).toBeUndefined();
    expect(result.current.isLoading).toBe(false);
    expect(result.current.error).toBeInstanceOf(Error);
    expect(result.current.retryCount).toBe(0);
    expect(result.current.isRetrying).toBe(false);
  });

  it("should retry and succeed", async () => {
    const asyncFn = jest
      .fn()
      .mockRejectedValueOnce(new Error("fail"))
      .mockResolvedValueOnce("success");
    const onSuccess = jest.fn();
    const onError = jest.fn();
    const { result } = renderHook(() =>
      useAsyncOperation(asyncFn, { maxRetries: 2, onError, onSuccess }),
    );

    // First call fails
    await act(async () => {
      try {
        await result.current.execute();
      } catch {}
    });

    // Retry should succeed
    let value: string | undefined;
    await act(async () => {
      value = await result.current.retry();
    });

    expect(asyncFn).toHaveBeenCalledTimes(2);
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onSuccess).toHaveBeenCalledWith("success");
    expect(result.current.data).toBe("success");
    expect(result.current.retryCount).toBe(1);
    expect(result.current.isRetrying).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("should not retry more than maxRetries", async () => {
    const asyncFn = jest.fn().mockRejectedValue(new Error("fail"));
    const { result } = renderHook(() => useAsyncOperation(asyncFn, { maxRetries: 2 }));

    // First call fails
    await act(async () => {
      try {
        await result.current.execute();
      } catch {}
    });

    // Retry 1
    await act(async () => {
      try {
        await result.current.retry();
      } catch {}
    });

    // Retry 2 (should hit max retries)
    await act(async () => {
      try {
        await result.current.retry();
      } catch {}
    });

    expect(result.current.retryCount).toBe(2);
    expect(result.current.canRetry).toBe(false);
  });

  it("should reset state", async () => {
    const asyncFn = jest.fn().mockRejectedValue(new Error("fail"));
    const { result } = renderHook(() => useAsyncOperation(asyncFn, { maxRetries: 1 }));

    await act(async () => {
      try {
        await result.current.execute();
      } catch {}
    });

    act(() => {
      result.current.reset();
    });

    expect(result.current.data).toBeUndefined();
    expect(result.current.isLoading).toBe(false);
    expect(result.current.error).toBeNull();
    expect(result.current.retryCount).toBe(0);
    expect(result.current.isRetrying).toBe(false);
  });
});
