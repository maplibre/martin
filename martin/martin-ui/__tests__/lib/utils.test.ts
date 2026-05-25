import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { copyToClipboard, formatFileSize } from '@/lib/utils';

describe('formatFileSize', () => {
  it("returns '0 Bytes' for 0", () => {
    expect(formatFileSize(0)).toBe('0 Bytes');
  });

  it("returns 'Unknown size' for undefined, null, NaN, or negative", () => {
    expect(formatFileSize(undefined as unknown as number)).toBe('Unknown size');
    expect(formatFileSize(null as unknown as number)).toBe('Unknown size');
    expect(formatFileSize(NaN)).toBe('Unknown size');
    expect(formatFileSize(-1)).toBe('Unknown size');
  });

  it('formats bytes correctly for typical values', () => {
    expect(formatFileSize(0)).toBe('0 Bytes');
    expect(formatFileSize(1)).toBe('1 Byte');
    expect(formatFileSize(2)).toBe('2 Bytes');
    expect(formatFileSize(512)).toBe('512 Bytes');
    expect(formatFileSize(1000)).toBe('1 KB');
    expect(formatFileSize(1500)).toBe('1.5 KB');
    expect(formatFileSize(1_000_000)).toBe('1 MB');
    expect(formatFileSize(1_000_000_000)).toBe('1 GB');
    expect(formatFileSize(1_000_000_000_000)).toBe('1 TB');
  });

  it('formats bytes correctly if passed nonsensical floats', () => {
    expect(formatFileSize(1.2345)).toBe('1 Bytes');
    expect(formatFileSize(1512.345)).toBe('1.51 KB');
  });

  it('formats with two decimal places for non-integer values', () => {
    expect(formatFileSize(1234)).toBe('1.23 KB');
    expect(formatFileSize(10_000_000)).toBe('10 MB');
    expect(formatFileSize(10_000_000_000)).toBe('10 GB');
    expect(formatFileSize(1_000_000_000_000)).toBe('1 TB');
  });

  it("returns 'File too large' for values exceeding TB", () => {
    expect(formatFileSize(1125899906842624)).toBe('File too large');
    expect(formatFileSize(Number.MAX_SAFE_INTEGER)).toBe('File too large');
  });
});

describe('copyToClipboard', () => {
  const execCommandMock = vi.fn().mockReturnValue(true);
  // Store reference to original execCommand if it exists
  // biome-ignore lint/suspicious/noExplicitAny: jsdom doesn't define execCommand, we need to add it
  const originalDocument = globalThis.document as any;

  beforeEach(() => {
    vi.clearAllMocks();
    execCommandMock.mockReturnValue(true);
    // Add execCommand to the document object (jsdom doesn't have this)
    originalDocument.execCommand = execCommandMock;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    // Clean up execCommand
    originalDocument.execCommand = undefined;
  });

  it('uses navigator.clipboard.writeText when available and succeeds', async () => {
    const writeTextMock = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', {
      clipboard: { writeText: writeTextMock },
    });

    await copyToClipboard('test text');
    expect(writeTextMock).toHaveBeenCalledWith('test text');
  });

  it('falls back to execCommand when navigator.clipboard is unavailable', async () => {
    vi.stubGlobal('navigator', { clipboard: undefined });

    await copyToClipboard('fallback text');
    expect(execCommandMock).toHaveBeenCalledWith('copy');
  });

  it('falls back to execCommand when navigator.clipboard.writeText fails', async () => {
    const writeTextMock = vi.fn().mockRejectedValue(new Error('Clipboard API error'));
    vi.stubGlobal('navigator', {
      clipboard: { writeText: writeTextMock },
    });

    await copyToClipboard('test text');
    expect(writeTextMock).toHaveBeenCalledWith('test text');
    expect(execCommandMock).toHaveBeenCalledWith('copy');
  });

  it('throws when both clipboard API and execCommand fail', async () => {
    vi.stubGlobal('navigator', { clipboard: undefined });
    execCommandMock.mockReturnValue(false);

    await expect(copyToClipboard('will fail')).rejects.toThrow('Copy command failed');
  });
});
