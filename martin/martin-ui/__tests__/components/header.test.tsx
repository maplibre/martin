import { screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { Header } from '@/components/header';
import { render } from '../test-utils';

// Mock import.meta.env for tests
const mockImportMeta = {
  env: {
    VITE_MARTIN_VERSION: 'v0.0.0-test',
  },
};

// @ts-expect-error
global.import = { meta: mockImportMeta };

describe('Header Component', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders correctly', () => {
    const { container } = render(<Header />);
    expect(container).toMatchSnapshot();
  });

  it('displays the Martin version', () => {
    const { getByText } = render(<Header />);
    expect(getByText('v0.0.0-test')).toBeTruthy();
  });

  it('contains navigation links', () => {
    const { getByText } = render(<Header />);

    const documentationLink = getByText('Documentation');
    expect(documentationLink).toBeTruthy();

    // hidden on mobile, so using a more specific query
    const aboutUsLink = getByText('About us');
    expect(aboutUsLink).toBeTruthy();
    expect(aboutUsLink.closest('a')?.getAttribute('href')).toBe('https://maplibre.org');
  });

  it('includes the theme switcher', () => {
    render(<Header />);
    // Look for the theme switcher button by its aria-label
    const themeSwitcher = screen.getByRole('button', {
      name: /switch to.*theme/i,
    });
    expect(themeSwitcher).toBeTruthy();
  });
});
