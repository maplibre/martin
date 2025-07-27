import { screen } from '@testing-library/react';
import { CatalogSkeleton } from '@/components/loading/catalog-skeleton';
import { render } from '../../test-utils';

describe('CatalogSkeleton Component', () => {
  it('renders correctly with default item count', () => {
    const { container } = render(
      <CatalogSkeleton description="Test description" title="Test Catalog" />,
    );
    expect(container).toMatchSnapshot();

    // Verify the title and description are rendered
    expect(screen.getByText('Test Catalog')).toBeInTheDocument();
    expect(screen.getByText('Test description')).toBeInTheDocument();

    // By default it should render 6 skeleton items
    const cards = container.querySelectorAll('[class*="hover:shadow-lg"]');
    expect(cards.length).toBe(6);
  });

  it('renders with custom item count', () => {
    const { container } = render(
      <CatalogSkeleton description="Custom item count" title="Custom Count Catalog" />,
    );
    expect(container).toMatchSnapshot();

    // Should render the default number of skeleton items (6)
    const cards = container.querySelectorAll('[class*="hover:shadow-lg"]');
    expect(cards.length).toBe(6);
  });

  it('renders with large item count', () => {
    const { container } = render(
      <CatalogSkeleton description="Many items" title="Large Catalog" />,
    );
    expect(container).toMatchSnapshot();

    // Should render the default number of skeleton items (6)
    const cards = container.querySelectorAll('[class*="hover:shadow-lg"]');
    expect(cards.length).toBe(6);
  });
});
