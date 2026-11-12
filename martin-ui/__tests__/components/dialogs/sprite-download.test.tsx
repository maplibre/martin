import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SpriteDownloadDialog } from '@/components/dialogs/sprite-download';
import { render } from '../../test-utils';

// Mock navigator.clipboard
Object.assign(navigator, {
  clipboard: {
    writeText: vi.fn().mockImplementation(() => Promise.resolve()),
  },
});

// Mock useToast hook
vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({
    toast: vi.fn(),
  }),
}));

describe('SpriteDownloadDialog Component', () => {
  const mockSprite = {
    id: 'test-sprite',
    images: ['icon1', 'icon2', 'icon3'],
  };

  const mockProps = {
    name: 'test-sprite',
    onCloseAction: vi.fn(),
    sprite: mockSprite,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders correctly', () => {
    const { container } = render(<SpriteDownloadDialog {...mockProps} />);
    expect(container).toMatchSnapshot();
  });

  it('displays the sprite name', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText('test-sprite')).toBeTruthy();
  });

  it('renders the PNG format section', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getAllByText('PNG').length).toBeGreaterThan(0);
    expect(screen.getByText('Standard Format')).toBeTruthy();
    expect(
      screen.getByText('Standard sprite format with multiple colors and transparency.'),
    ).toBeTruthy();
  });

  it('renders the SDF format section', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getAllByText('SDF').length).toBeGreaterThan(0);
    expect(screen.getByText('Signed Distance Field')).toBeTruthy();
    expect(screen.getByText('For dynamic coloring at runtime.')).toBeTruthy();
  });

  it('lists all download options for PNG format', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText('PNG JSON')).toBeTruthy();
    expect(screen.getByText('PNG Spritesheet')).toBeTruthy();
    expect(screen.getByText('High DPI PNG Spritesheet')).toBeTruthy();
  });

  it('lists all download options for SDF format', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText('SDF Spritesheet')).toBeTruthy();
    expect(screen.getByText('SDF JSON')).toBeTruthy();
    expect(screen.getByText('High DPI SDF Spritesheet')).toBeTruthy();
  });

  it('calls onCloseAction when dialog is closed', async () => {
    const user = userEvent.setup();
    const { getByRole } = render(<SpriteDownloadDialog {...mockProps} />);

    // Find and click the close button (X in the dialog)
    const closeButton = getByRole('button', { name: /close/i });
    await user.click(closeButton);

    // Check if onCloseAction was called
    expect(mockProps.onCloseAction).toHaveBeenCalled();
  });
});
