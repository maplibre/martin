import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SpriteDownloadDialog } from '@/components/dialogs/sprite-download';
import { render } from '../../test-utils';

// Mock navigator.clipboard
Object.assign(navigator, {
  clipboard: {
    writeText: jest.fn().mockImplementation(() => Promise.resolve()),
  },
});

// Mock useToast hook
jest.mock('@/hooks/use-toast', () => ({
  useToast: () => ({
    toast: jest.fn(),
  }),
}));

describe('SpriteDownloadDialog Component', () => {
  const mockSprite = {
    id: 'test-sprite',
    images: ['icon1', 'icon2', 'icon3'],
  };

  const mockProps = {
    name: 'test-sprite',
    onCloseAction: jest.fn(),
    sprite: mockSprite,
  };

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('renders correctly', () => {
    const { container } = render(<SpriteDownloadDialog {...mockProps} />);
    expect(container).toMatchSnapshot();
  });

  it('displays the sprite name', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText('test-sprite')).toBeInTheDocument();
  });

  it('renders the PNG format section', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getAllByText('PNG').length).toBeGreaterThan(0);
    expect(screen.getByText('Standard Format')).toBeInTheDocument();
    expect(
      screen.getByText('Standard sprite format with multiple colors and transparency.'),
    ).toBeInTheDocument();
  });

  it('renders the SDF format section', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getAllByText('SDF').length).toBeGreaterThan(0);
    expect(screen.getByText('Signed Distance Field')).toBeInTheDocument();
    expect(screen.getByText('For dynamic coloring at runtime.')).toBeInTheDocument();
  });

  it('lists all download options for PNG format', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText('PNG JSON')).toBeInTheDocument();
    expect(screen.getByText('PNG Spritesheet')).toBeInTheDocument();
    expect(screen.getByText('High DPI PNG Spritesheet')).toBeInTheDocument();
  });

  it('lists all download options for SDF format', () => {
    render(<SpriteDownloadDialog {...mockProps} />);
    expect(screen.getByText('SDF Spritesheet')).toBeInTheDocument();
    expect(screen.getByText('SDF JSON')).toBeInTheDocument();
    expect(screen.getByText('High DPI SDF Spritesheet')).toBeInTheDocument();
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
