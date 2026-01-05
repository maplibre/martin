import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mockToast = vi.fn();
vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({ toast: mockToast }),
}));

const mockCopyToClipboard = vi.fn();
vi.mock('@/lib/utils', () => ({
  copyToClipboard: (text: string) => mockCopyToClipboard(text),
}));

const { useCopyToClipboard } = await import('@/hooks/use-copy-to-clipboard');

describe('useCopyToClipboard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockCopyToClipboard.mockResolvedValue(undefined);
  });

  it('copies text and shows toast', async () => {
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

  it('uses custom success message from options', async () => {
    const { result } = renderHook(() => useCopyToClipboard({ successMessage: 'URL copied!' }));

    await act(async () => {
      await result.current.copy('http://example.com');
    });

    expect(mockToast).toHaveBeenCalledWith({
      description: 'URL copied!',
      title: 'Copied!',
    });
  });

  it('handles errors and shows error toast', async () => {
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
});
