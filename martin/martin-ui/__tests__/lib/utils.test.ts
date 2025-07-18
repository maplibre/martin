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
    expect(formatFileSize(1)).toBe('1 Bytes');
    expect(formatFileSize(512)).toBe('512 Bytes');
    expect(formatFileSize(1024)).toBe('1 KiB');
    expect(formatFileSize(1536)).toBe('1.5 KiB');
    expect(formatFileSize(1048576)).toBe('1 MiB');
    expect(formatFileSize(1073741824)).toBe('1 GiB');
    expect(formatFileSize(1099511627776)).toBe('1 TiB');
  });

  it('formats bytes correctly if passed nonsenical floats', () => {
    expect(formatFileSize(1.2345)).toBe('1 Bytes');
    expect(formatFileSize(1512.345)).toBe('1.48 KiB');
  });

  it('formats with two decimal places for non-integer values', () => {
    expect(formatFileSize(1234)).toBe('1.21 KiB');
    expect(formatFileSize(10485760)).toBe('10 MiB');
    expect(formatFileSize(10737418240)).toBe('10 GiB');
    expect(formatFileSize(1100585369600)).toBe('1 TiB');
  });

  it("returns 'File too large' for values exceeding TB", () => {
    // 1 PB (petabyte) = 1024 TB
    expect(formatFileSize(1125899906842624)).toBe('File too large');
    expect(formatFileSize(Number.MAX_SAFE_INTEGER)).toBe('File too large');
  });
});
