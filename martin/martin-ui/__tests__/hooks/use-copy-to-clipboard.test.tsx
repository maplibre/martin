import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// Mock the useToast hook
const mockToast = vi.fn();
vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({
    toast: mockToast,
  }),
}));

// Mock copyToClipboard
const mockCopyToClipboard = vi.fn();
vi.mock('@/lib/utils', () => ({
  copyToClipboard: (text: string) => mockCopyToClipboard(text),
}));

// Import after mocks are set up
const { useCopyToClipboard } = await import('@/hooks/use-copy-to-clipboard');

describe('useCopyToClipboard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockCopyToClipboard.mockResolvedValue(undefined);
  });

  it('should copy text successfully and show toast', async () => {
    const { result } = renderHook(() => useCopyToClipboard());

    expect(result.current.copied).toBe(false);
    expect(result.current.copiedText).toBeNull();

    let success: boolean | undefined;
    await act(async () => {
      success = await result.current.copy('test text');
    });

    expect(success).toBe(true);
    expect(result.current.copied).toBe(true);
    expect(result.current.copiedText).toBe('test text');
    expect(mockCopyToClipboard).toHaveBeenCalledWith('test text');
    expect(mockToast).toHaveBeenCalledWith({
      description: 'Copied!',
      title: 'Copied!',
    });
  });

  it('should use custom success message', async () => {
    const { result } = renderHook(() => useCopyToClipboard({ successMessage: 'URL copied!' }));

    await act(async () => {
      await result.current.copy('http://example.com');
    });

    expect(mockToast).toHaveBeenCalledWith({
      description: 'URL copied!',
      title: 'Copied!',
    });
  });

  it('should allow custom success message per copy call', async () => {
    const { result } = renderHook(() => useCopyToClipboard());

    await act(async () => {
      await result.current.copy('test', 'Custom message!');
    });

    expect(mockToast).toHaveBeenCalledWith({
      description: 'Custom message!',
      title: 'Copied!',
    });
  });

  it('should handle errors and show error toast', async () => {
    mockCopyToClipboard.mockRejectedValue(new Error('Copy failed'));

    const { result } = renderHook(() => useCopyToClipboard());

    let success: boolean | undefined;
    await act(async () => {
      success = await result.current.copy('test text');
    });

    expect(success).toBe(false);
    expect(result.current.copied).toBe(false);
    expect(result.current.copiedText).toBeNull();
    expect(mockToast).toHaveBeenCalledWith({
      description: 'Failed to copy to clipboard',
      title: 'Error',
      variant: 'destructive',
    });
  });

  it('should not show toasts when disabled', async () => {
    const { result } = renderHook(() =>
      useCopyToClipboard({
        showErrorToast: false,
        showSuccessToast: false,
      }),
    );

    await act(async () => {
      await result.current.copy('test');
    });

    expect(mockToast).not.toHaveBeenCalled();
  });

  it('should reset copied state manually', async () => {
    const { result } = renderHook(() => useCopyToClipboard());

    await act(async () => {
      await result.current.copy('test');
    });

    expect(result.current.copied).toBe(true);

    act(() => {
      result.current.reset();
    });

    expect(result.current.copied).toBe(false);
    expect(result.current.copiedText).toBeNull();
  });
});
