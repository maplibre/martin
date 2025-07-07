import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { TileInspectDialog } from '@/components/dialogs/tile-inspect';
import type { TileSource } from '@/lib/types';
import React from 'react';

// Mock MapLibre GL and related imports
jest.mock('maplibre-gl', () => ({
  Map: jest.fn().mockImplementation(() => ({
    addControl: jest.fn(),
    removeControl: jest.fn(),
    on: jest.fn(),
    off: jest.fn(),
    remove: jest.fn(),
  })),
}));

jest.mock('@vis.gl/react-maplibre', () => {
  const { forwardRef } = require('react');
  return {
    Map: forwardRef((props: any, ref: any) => {
      return (
        <div
          data-testid="maplibre-map"
          onClick={() => props.onLoad && props.onLoad()}
          style={props.style}
        />
      );
    }),
  };
});

jest.mock('@maplibre/maplibre-gl-inspect', () => ({
  __esModule: true,
  default: jest.fn().mockImplementation(() => ({
    onAdd: jest.fn(),
    onRemove: jest.fn(),
  })),
}));

// Mock CSS imports
jest.mock('maplibre-gl/dist/maplibre-gl.css', () => ({}));
jest.mock('@maplibre/maplibre-gl-inspect/dist/maplibre-gl-inspect.css', () => ({}));

describe('TileInspectDialog', () => {
  const mockTileSource: TileSource = {
    content_type: 'image/png',
    content_encoding: 'gzip',
    name: 'Test Tile Source',
    description: 'A test tile source for testing',
    attribution: 'Test Attribution',
    layerCount: 5,
  };

  const mockOnClose = jest.fn();

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders dialog with correct title and source information', () => {
    render(
      <TileInspectDialog
        name="test-tiles"
        source={mockTileSource}
        onCloseAction={mockOnClose}
      />
    );

    expect(screen.getByText('Inspect Tile Source:')).toBeInTheDocument();
    expect(screen.getByText('test-tiles')).toBeInTheDocument();
    expect(screen.getByText('Source Information')).toBeInTheDocument();
    expect(screen.getByText('image/png')).toBeInTheDocument();
    expect(screen.getByText('gzip')).toBeInTheDocument();
    expect(screen.getByText('Test Tile Source')).toBeInTheDocument();
    expect(screen.getByText('A test tile source for testing')).toBeInTheDocument();
    expect(screen.getByText('Test Attribution')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
  });

  it('renders map component', () => {
    render(
      <TileInspectDialog
        name="test-tiles"
        source={mockTileSource}
        onCloseAction={mockOnClose}
      />
    );

    expect(screen.getByTestId('maplibre-map')).toBeInTheDocument();
  });

  it('renders description explaining the dialog purpose', () => {
    render(
      <TileInspectDialog
        name="test-tiles"
        source={mockTileSource}
        onCloseAction={mockOnClose}
      />
    );

    expect(
      screen.getByText(/Inspect the tile source to explore tile boundaries and properties/)
    ).toBeInTheDocument();
  });

  it('calls onCloseAction when dialog is closed', () => {
    render(
      <TileInspectDialog
        name="test-tiles"
        source={mockTileSource}
        onCloseAction={mockOnClose}
      />
    );

    // Find the dialog close button (the X button in the top right)
    const closeButton = screen.getByRole('button', { name: /close/i });
    fireEvent.click(closeButton);

    expect(mockOnClose).toHaveBeenCalledTimes(1);
  });

  it('handles vector tile source correctly', () => {
    const vectorTileSource: TileSource = {
      content_type: 'application/x-protobuf',
      name: 'Vector Tiles',
      description: 'Vector tile source',
    };

    render(
      <TileInspectDialog
        name="vector-tiles"
        source={vectorTileSource}
        onCloseAction={mockOnClose}
      />
    );

    expect(screen.getByText('application/x-protobuf')).toBeInTheDocument();
    expect(screen.getByText('Vector Tiles')).toBeInTheDocument();
  });

  it('handles minimal tile source without optional fields', () => {
    const minimalTileSource: TileSource = {
      content_type: 'image/jpeg',
    };

    render(
      <TileInspectDialog
        name="minimal-tiles"
        source={minimalTileSource}
        onCloseAction={mockOnClose}
      />
    );

    expect(screen.getByText('image/jpeg')).toBeInTheDocument();
    expect(screen.getByText('Inspect Tile Source:')).toBeInTheDocument();
    expect(screen.getByText('minimal-tiles')).toBeInTheDocument();
  });


  it('displays content type and encoding information', () => {
    render(
      <TileInspectDialog
        name="test-tiles"
        source={mockTileSource}
        onCloseAction={mockOnClose}
      />
    );

    expect(screen.getByText('Content Type:')).toBeInTheDocument();
    expect(screen.getByText('Encoding:')).toBeInTheDocument();
  });

  it('conditionally renders optional fields', () => {
    const sourceWithoutOptionals: TileSource = {
      content_type: 'image/png',
    };

    render(
      <TileInspectDialog
        name="test-tiles"
        source={sourceWithoutOptionals}
        onCloseAction={mockOnClose}
      />
    );

    expect(screen.queryByText('Encoding:')).not.toBeInTheDocument();
    expect(screen.queryByText('Name:')).not.toBeInTheDocument();
    expect(screen.queryByText('Description:')).not.toBeInTheDocument();
    expect(screen.queryByText('Layer Count:')).not.toBeInTheDocument();
    expect(screen.queryByText('Attribution:')).not.toBeInTheDocument();
  });
});
