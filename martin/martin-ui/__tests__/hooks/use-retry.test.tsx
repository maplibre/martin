import { act, renderHook } from "@testing-library/react";
import { useRetry } from "../../hooks/use-retry";

// Mock useToast to avoid side effects and track calls
jest.mock("../../hooks/use-toast", () => ({
  useToast: () => ({
    toast: jest.fn(),
  }),
}));

describe("useRetry", () => {
  it("should succeed on first attempt", async () => {
    const asyncFn = jest.fn().mockResolvedValue("success");
    const onSuccess = jest.fn();
    const { result } = renderHook(() => useRetry(asyncFn, { maxAttempts: 3, onSuccess }));

    let value: string | null = null;
    await act(async () => {
      value = await result.current.retry();
    });

    expect(asyncFn).toHaveBeenCalledTimes(1);
    expect(onSuccess).toHaveBeenCalled();
    expect(value).toBe("success");
    expect(result.current.isRetrying).toBe(false);
    expect(result.current.lastError).toBeNull();
  });

  it("should retry and eventually succeed", async () => {
    const asyncFn = jest
      .fn()
      .mockRejectedValueOnce(new Error("fail 1"))
      .mockResolvedValueOnce("final success");
    const onError = jest.fn();
    const onSuccess = jest.fn();
    const { result } = renderHook(() => useRetry(asyncFn, { maxAttempts: 2, onError, onSuccess }));

    let value: string | null = null;
    await act(async () => {
      value = await result.current.retry();
    });

    expect(asyncFn).toHaveBeenCalledTimes(2);
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onSuccess).toHaveBeenCalled();
    expect(value).toBe("final success");
    expect(result.current.isRetrying).toBe(false);
    expect(result.current.lastError).toBeNull();
  });

  it("should call onMaxAttemptsReached and return null after max attempts", async () => {
    const asyncFn = jest.fn().mockRejectedValue(new Error("fail always"));
    const onError = jest.fn();
    const onMaxAttemptsReached = jest.fn();
    const { result } = renderHook(() =>
      useRetry(asyncFn, { maxAttempts: 2, onError, onMaxAttemptsReached }),
    );

    let value: string | null = null;
    await act(async () => {
      value = await result.current.retry();
    });

    expect(asyncFn).toHaveBeenCalledTimes(2);
    expect(onError).toHaveBeenCalledTimes(2);
    expect(onMaxAttemptsReached).toHaveBeenCalledTimes(1);
    expect(value).toBeNull();
    expect(result.current.isRetrying).toBe(false);
    expect(result.current.lastError).toBeInstanceOf(Error);
  });

  it("should reset state when reset is called", async () => {
    const asyncFn = jest.fn().mockRejectedValue(new Error("fail always"));
    const { result } = renderHook(() => useRetry(asyncFn, { maxAttempts: 1 }));

    await act(async () => {
      await result.current.retry();
    });

    act(() => {
      result.current.reset();
    });

    expect(result.current.isRetrying).toBe(false);
    expect(result.current.attempt).toBe(0);
    expect(result.current.lastError).toBeNull();
  });
});
