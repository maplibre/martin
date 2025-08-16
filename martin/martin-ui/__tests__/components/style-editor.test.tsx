import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { StyleEditor } from '@/components/style-editor';
import type { ButtonProps } from '@/components/ui/button';
import { cleanup, render } from '../test-utils';

// Mock component interfaces
interface MockComponentProps {
  children?: ReactNode;
  [key: string]: unknown;
}

// Mock the UI components
vi.mock('@/components/ui/button', () => ({
  Button: ({ children, onClick, ...props }: ButtonProps & { onClick?: () => void }) => (
    <button onClick={onClick} {...props}>
      {children}
    </button>
  ),
}));

vi.mock('@/components/ui/card', () => ({
  Card: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardContent: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardHeader: ({ children, ...props }: MockComponentProps) => <div {...props}>{children}</div>,
  CardTitle: ({ children, ...props }: MockComponentProps) => <h3 {...props}>{children}</h3>,
}));

vi.mock('@/lib/api', () => ({
  buildMartinUrl: vi.fn((path: string) => `http://localhost:3000${path}`),
}));

// Mock lucide-react icons
vi.mock('lucide-react', () => ({
  ArrowLeft: () => <span>←</span>,
  X: () => <span>×</span>,
}));

describe('StyleEditor', () => {
  const mockStyle = {
    colors: ['#ff0000', '#00ff00', '#0000ff'],
    lastModifiedAt: new Date('2023-01-01'),
    layerCount: 5,
    path: '/styles/test-style.json',
    type: 'vector' as const,
    versionHash: 'abc123',
  };

  const defaultProps = {
    onClose: vi.fn(),
    style: mockStyle,
    styleName: 'test-style',
  };

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('renders the style editor with correct title', () => {
    const { container } = render(<StyleEditor {...defaultProps} />);

    expect(container.textContent).toContain('test-style');
    expect(container.textContent).toContain('/styles/test-style.json');
  });

  it('renders navigation buttons', () => {
    const { container } = render(<StyleEditor {...defaultProps} />);

    const backButton = container.querySelector('button');
    expect(backButton).toBeTruthy();
    expect(backButton?.textContent).toContain('Back to Catalog');
  });

  it('renders iframe with correct src', () => {
    const { container } = render(<StyleEditor {...defaultProps} />);

    const iframe = container.querySelector('iframe');
    expect(iframe).toBeTruthy();
    expect(iframe?.getAttribute('src')).toBeDefined();

    const src = iframe?.getAttribute('src');
    expect(src).toContain('https://maplibre.org/maputnik/');
    expect(src).toContain('style=http%3A%2F%2Flocalhost%3A3000%2Fstyle%2Ftest-style');
  });

  it('renders iframe without loading state', () => {
    const { container } = render(<StyleEditor {...defaultProps} />);

    const iframe = container.querySelector('iframe');
    expect(iframe).toBeTruthy();
    expect(iframe?.getAttribute('title')).toBe('Maputnik Style Editor - test-style');
  });

  it('calls onClose when back button is clicked', () => {
    const onClose = vi.fn();
    const { container } = render(<StyleEditor {...defaultProps} onClose={onClose} />);

    const backButton = container.querySelector('button');
    expect(backButton).toBeTruthy();
    backButton?.click();

    expect(onClose).toHaveBeenCalled();
  });

  it('renders with proper iframe sandbox attributes', () => {
    const { container } = render(<StyleEditor {...defaultProps} />);

    const iframe = container.querySelector('iframe');
    expect(iframe?.getAttribute('sandbox')).toBe(
      'allow-same-origin allow-scripts allow-forms allow-popups allow-downloads allow-modals',
    );
  });

  it('constructs proper Maputnik URL with encoded style parameter', () => {
    const { container } = render(<StyleEditor {...defaultProps} />);

    const iframe = container.querySelector('iframe');
    const src = iframe?.getAttribute('src');

    expect(src).toContain('https://maplibre.org/maputnik/');
    expect(src).toContain('style=');
    expect(src).toContain('test-style');
  });
});
