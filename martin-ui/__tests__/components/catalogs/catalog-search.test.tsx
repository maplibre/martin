import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CatalogSearch } from '@/components/catalogs/catalog-search';

describe('CatalogSearch', () => {
  afterEach(() => {
    cleanup();
  });

  it('reports each typed query', () => {
    const onChange = vi.fn();
    render(<CatalogSearch onChange={onChange} placeholder="Search fonts..." value="" />);

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search fonts' }), {
      target: { value: 'noto' },
    });

    expect(onChange).toHaveBeenCalledWith('noto');
  });

  it('shows the current query', () => {
    render(<CatalogSearch onChange={vi.fn()} placeholder="Search styles..." value="basic" />);

    expect(screen.getByRole<HTMLInputElement>('searchbox').value).toBe('basic');
  });

  it('clears the query', () => {
    const onChange = vi.fn();
    render(<CatalogSearch onChange={onChange} placeholder="Search sprites..." value="pin" />);

    fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));

    expect(onChange).toHaveBeenCalledWith('');
  });

  it('has no clear button while empty', () => {
    render(<CatalogSearch onChange={vi.fn()} placeholder="Search sprites..." value="" />);

    expect(screen.queryByRole('button', { name: 'Clear search' })).toBeNull();
  });
});
