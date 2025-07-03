import { act, renderHook } from "@testing-library/react";
import { useAsyncOperation } from "@/hooks/use-async-operation";
import { useToast } from "@/hooks/use-toast";

// Mock the useToast hook to spy on toast notifications
jest.mock("@/hooks/use-toast", () => ({
  useToast: jest.fn(() => ({
    toast: jest.fn(),
  })),
}));

jest.useFakeTimers();

describe("useAsyncOperation", () => {
  const mockToast = jest.fn();

  beforeEach(() => {
    // Clear mock history before each test
    mockToast.mockClear();
    (useToast as jest.Mock).mockClear();
    (useToast as jest.Mock).mockReturnValue({ toast: mockToast });
  });

  it("should handle successful operation on the first attempt", async () => {
    const mockAsyncFunction = jest.fn().mockResolvedValue("Success Data");
    const onSuccess = jest.fn();

    const { result } = renderHook(() =>
      useAsyncOperation<string>(mockAsyncFunction, { onSuccess }),
    );

    expect(result.current.isLoading).toBe(false);
    expect(result.current.data).toBeUndefined();
    expect(result.current.error).toBeNull();

    let promise: Promise<string | void> | undefined;
    act(() => {
      promise = result.current.execute();
    });

    expect(result.current.isLoading).toBe(true);

    await act(async () => {
      await promise!;
    });

    expect(result.current.isLoading).toBe(false);
    expect(result.current.data).toBe("Success Data");
    expect(result.current.error).toBeNull();
    expect(mockAsyncFunction).toHaveBeenCalledTimes(1);
    expect(onSuccess).toHaveBeenCalledWith("Success Data");
  });

  it("should handle operation failure after all retries", async () => {
    const error = new Error("Failed");
    const mockAsyncFunction = jest.fn().mockRejectedValue(error);
    const onError = jest.fn();

    const { result } = renderHook(() =>
      useAsyncOperation<unknown>(mockAsyncFunction, { onError, maxRetries: 3 }),
    );

    let executePromise: Promise<unknown> | undefined;
    act(() => {
      executePromise = result.current.execute();
    });

    await act(async () => {
      // Advance timers to simulate backoff delays
      // First retry after ~500ms
      await jest.advanceTimersByTimeAsync(501);
      // Second retry after ~1000ms
      await jest.advanceTimersByTimeAsync(1001);
      // Wait for the promise to reject
      await expect(executePromise!).rejects.toThrow("Failed");
    });

    expect(result.current.isLoading).toBe(false);
    expect(result.current.data).toBeUndefined();
    expect(result.current.error).toEqual(error);
    expect(mockAsyncFunction).toHaveBeenCalledTimes(3);
    expect(onError).toHaveBeenCalledTimes(3);
    expect(onError).toHaveBeenCalledWith(error, 1);
    expect(onError).toHaveBeenCalledWith(error, 2);
    expect(onError).toHaveBeenCalledWith(error, 3);
  });

  it("should succeed on the second attempt", async () => {
    const successData = "Success on second try";
    const mockAsyncFunction = jest
      .fn()
      .mockRejectedValueOnce(new Error("Failed first time"))
      .mockResolvedValueOnce(successData);
    const onSuccess = jest.fn();
    const onError = jest.fn();

    const { result } = renderHook(() =>
      useAsyncOperation<string>(mockAsyncFunction, { onSuccess, onError }),
    );

    let promise: Promise<string | void> | undefined;
    act(() => {
      promise = result.current.execute();
    });

    await act(async () => {
      // Wait for the first backoff period
      await jest.advanceTimersByTimeAsync(501);
      await promise!;
    });

    expect(result.current.isLoading).toBe(false);
    expect(result.current.data).toBe(successData);
    expect(result.current.error).toBeNull();
    expect(mockAsyncFunction).toHaveBeenCalledTimes(2);
    expect(onSuccess).toHaveBeenCalledWith(successData);
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("should show success and error toasts when configured", async () => {
    // Test error toast
    const error = new Error("Toast Test Failed");
    const failingMock = jest.fn().mockRejectedValue(error);
    const { result: errorResult } = renderHook(() =>
      useAsyncOperation<unknown>(failingMock, {
        maxRetries: 1,
        showErrorToast: true,
      }),
    );

    await act(async () => {
      await expect(errorResult.current.execute()).rejects.toThrow(
        "Toast Test Failed",
      );
    });

    expect(mockToast).toHaveBeenCalledWith({
      title: "Error",
      description: "Operation failed after 1 attempts: Toast Test Failed",
      variant: "destructive",
    });

    mockToast.mockClear();

    // Test success toast
    const successData = "Toast Test Success";
    const succeedingMock = jest.fn().mockResolvedValue(successData);
    const { result: successResult } = renderHook(() =>
      useAsyncOperation<string>(succeedingMock, {
        showSuccessToast: true,
        successMessage: "It worked!",
      }),
    );

    await act(async () => {
      await successResult.current.execute();
    });

    expect(mockToast).toHaveBeenCalledWith({
      title: "Success",
      description: "It worked!",
    });
  });

  it("should reset the state", async () => {
    const mockAsyncFunction = jest.fn().mockResolvedValue("Some data");
    const { result } = renderHook(() =>
      useAsyncOperation<string>(mockAsyncFunction),
    );

    // Execute to change state
    await act(async () => {
      await result.current.execute();
    });

    expect(result.current.data).toBe("Some data");
    expect(result.current.isLoading).toBe(false);

    // Reset the state
    act(() => {
      result.current.reset();
    });

    expect(result.current.data).toBeUndefined();
    expect(result.current.error).toBeNull();
    expect(result.current.isLoading).toBe(false);
  });
});
