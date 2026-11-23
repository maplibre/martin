import { describe, expect, it } from 'vitest';
import { spriteFileUrl } from '@/components/style-editor/value-editors/pickers/icon/sprite-sheets';

describe('spriteFileUrl', () => {
  it('puts the suffix on the path and keeps an access key in the query', () => {
    expect(spriteFileUrl('https://tiles.example.com/sprite?key=abc', '@2x.json')).toBe(
      'https://tiles.example.com/sprite@2x.json?key=abc',
    );
  });

  it('resolves a relative sprite against the page', () => {
    expect(spriteFileUrl('/sprite/basic', '.png')).toBe(
      `${window.location.origin}/sprite/basic.png`,
    );
  });
});
