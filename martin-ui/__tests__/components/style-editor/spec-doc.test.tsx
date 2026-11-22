import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { SpecDoc, specDocText } from '@/components/style-editor/spec-doc';

describe('SpecDoc', () => {
  afterEach(cleanup);

  it('renders code, links and images with paths resolved against the spec site', () => {
    const { container } = render(
      <SpecDoc markdown="Unlike [`line-opacity`](layers/#line-opacity), `line-layer-opacity` is applied **once**. ![line-opacity vs. line-layer-opacity](assets/line-layer-opacity.svg)" />,
    );

    expect(screen.getByRole('link', { name: 'line-opacity' }).getAttribute('href')).toBe(
      'https://maplibre.org/maplibre-style-spec/layers/#line-opacity',
    );
    expect(
      screen.getByRole('img', { name: 'line-opacity vs. line-layer-opacity' }).getAttribute('src'),
    ).toBe('https://maplibre.org/maplibre-style-spec/assets/line-layer-opacity.svg');
    expect([...container.querySelectorAll('code')].map((c) => c.textContent)).toEqual([
      'line-opacity',
      'line-layer-opacity',
    ]);
    expect(container.querySelector('strong')?.textContent).toBe('once');
  });

  it('turns links to Markdown files into spec pages and keeps absolute links', () => {
    render(
      <SpecDoc markdown="See [layers](./layers.md#paint) and [URL syntax](https://en.wikipedia.org/wiki/URL#Syntax)." />,
    );

    expect(screen.getByRole('link', { name: 'layers' }).getAttribute('href')).toBe(
      'https://maplibre.org/maplibre-style-spec/layers/#paint',
    );
    expect(screen.getByRole('link', { name: 'URL syntax' }).getAttribute('href')).toBe(
      'https://en.wikipedia.org/wiki/URL#Syntax',
    );
  });

  it('drops links that are not http', () => {
    const { container } = render(<SpecDoc markdown="Do not [click](data:text/html,hi) here." />);

    expect(screen.queryByRole('link')).toBeNull();
    expect(container.textContent).toBe('Do not click here.');
  });

  it('splits paragraphs and merges list items separated by blank lines into one list', () => {
    const { container } = render(
      <SpecDoc
        markdown={
          'The URL must include:\n\n - `{fontstack}` - the font stack. \n\n - `{range}` - 256 code points.\n\nThe URL must be absolute.'
        }
      />,
    );

    expect([...container.querySelectorAll('p')].map((p) => p.textContent)).toEqual([
      'The URL must include:',
      'The URL must be absolute.',
    ]);
    expect(screen.getAllByRole('listitem').map((li) => li.textContent)).toEqual([
      '{fontstack} - the font stack.',
      '{range} - 256 code points.',
    ]);
  });
});

describe('specDocText', () => {
  it('keeps the words and drops Markdown syntax and images', () => {
    expect(
      specDocText(
        'Unlike [`line-opacity`](layers/#line-opacity), the _layer_ opacity is applied **once**.\n\n - one\n - two\n\n![diagram](assets/x.svg)',
      ),
    ).toBe('Unlike line-opacity, the layer opacity is applied once. one two');
  });
});
