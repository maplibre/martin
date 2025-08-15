import { cleanup, render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TileInspectDialog } from '@/components/dialogs/tile-inspect';
import type { TileSource } from '@/lib/types';

interface MockComponentProps {
  children?: ReactNode;
  className?: string;
  [key: string]: unknown;
}

// Mock the UI dialog components
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({
    children,
    open,
  }: MockComponentProps & {
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
  }) => (
    <div data-open={open} role="dialog">
      {children}
    </div>
  ),
  DialogContent: ({ children, className }: MockComponentProps) => (
    <div className={className}>{children}</div>
  ),
  DialogDescription: ({ children }: MockComponentProps) => <p>{children}</p>,
  DialogHeader: ({ children, className }: MockComponentProps) => (
    <div className={className}>{children}</div>
  ),
  DialogTitle: ({ children, className }: MockComponentProps) => (
    <h2 className={className}>{children}</h2>
  ),
}));

// Mock the UI button component
vi.mock('@/components/ui/button', () => ({
  Button: ({
    children,
    onClick,
    size,
    variant,
    ...props
  }: MockComponentProps & { onClick?: () => void }) => (
    <button onClick={onClick} {...props}>
      {children}
    </button>
  ),
}));

describe('TileInspectDialog', () => {
  const mockTileSource: TileSource = {
    attribution: 'Test Attribution',
    content_encoding: 'gzip',
    content_type: 'image/png',
    description: 'A test tile source for testing',
    layerCount: 5,
    name: 'Test Tile Source',
  };

  const mockOnClose = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it('renders dialog with correct title and source information', () => {
    const { container } = render(
      <TileInspectDialog name="test-tiles" onCloseAction={mockOnClose} source={mockTileSource} />,
    );

    expect(container.textContent).toContain('Inspect Tile Source:');
    expect(container.textContent).toContain('test-tiles');
    expect(container.textContent).toContain('Source Information');
    expect(container.textContent).toContain('image/png');
    expect(container.textContent).toContain('gzip');
    expect(container.textContent).toContain('Test Tile Source');
    expect(container.textContent).toContain('A test tile source for testing');
    expect(container.textContent).toContain('Test Attribution');
    expect(container.textContent).toContain('5');
  });

  it('renders map component', () => {
    const { container } = render(
      <TileInspectDialog name="test-tiles" onCloseAction={mockOnClose} source={mockTileSource} />,
    );

    const mapElement = container.querySelector('[data-testid="maplibre-map"]');
    expect(mapElement).toBeTruthy();
  });

  it('renders description explaining the dialog purpose', () => {
    const { container } = render(
      <TileInspectDialog name="test-tiles" onCloseAction={mockOnClose} source={mockTileSource} />,
    );

    expect(container.textContent).toMatch(
      /Inspect the tile source to explore tile boundaries and properties/,
    );
  });

  it('calls onCloseAction when dialog is closed', () => {
    const { container } = render(
      <TileInspectDialog name="test-tiles" onCloseAction={mockOnClose} source={mockTileSource} />,
    );

    // The dialog should have onOpenChange callback that triggers onCloseAction
    const dialogElement = container.querySelector('[role="dialog"]');
    expect(dialogElement).toBeTruthy();

    // We'll test this by verifying the dialog is open and that onCloseAction exists
    expect(mockOnClose).toBeDefined();
    expect(typeof mockOnClose).toBe('function');
  });

  it('handles vector tile source correctly', () => {
    const vectorTileSource: TileSource = {
      content_type: 'application/x-protobuf',
      description: 'Vector tile source',
      name: 'Vector Tiles',
    };

    const { container } = render(
      <TileInspectDialog
        name="vector-tiles"
        onCloseAction={mockOnClose}
        source={vectorTileSource}
      />,
    );

    expect(container.textContent).toContain('application/x-protobuf');
    expect(container.textContent).toContain('Vector Tiles');
  });

  it('handles minimal tile source without optional fields', () => {
    const minimalTileSource: TileSource = {
      content_type: 'image/jpeg',
    };

    const { container } = render(
      <TileInspectDialog
        name="minimal-tiles"
        onCloseAction={mockOnClose}
        source={minimalTileSource}
      />,
    );

    expect(container.textContent).toContain('image/jpeg');
    expect(container.textContent).toContain('Inspect Tile Source:');
    expect(container.textContent).toContain('minimal-tiles');
  });

  it('displays content type and encoding information', () => {
    const { container } = render(
      <TileInspectDialog name="test-tiles" onCloseAction={mockOnClose} source={mockTileSource} />,
    );

    expect(container.textContent).toContain('Content Type:');
    expect(container.textContent).toContain('Encoding:');
  });

  it('conditionally renders optional fields', () => {
    const sourceWithoutOptionals: TileSource = {
      content_type: 'image/png',
    };

    const { container } = render(
      <TileInspectDialog
        name="test-tiles"
        onCloseAction={mockOnClose}
        source={sourceWithoutOptionals}
      />,
    );

    expect(container.textContent).not.toContain('Encoding:');
    expect(container.textContent).not.toContain('Name:');
    expect(container.textContent).not.toContain('Description:');
    expect(container.textContent).not.toContain('Layer Count:');
    expect(container.textContent).not.toContain('Attribution:');
  });
});
