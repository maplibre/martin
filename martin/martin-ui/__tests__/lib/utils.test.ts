import { describe, expect, it } from 'vitest';
import { formatFileSize } from '@/lib/utils';

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
