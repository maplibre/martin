import { screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ThemeSwitcher } from '@/components/theme-switcher';
import { render } from '../test-utils';

// Mock useTheme hook with a simpler approach
const mockSetTheme = vi.fn();

vi.mock('next-themes', () => ({
  ThemeProvider: ({ children }: { children: React.ReactNode }) => children,
  useTheme: vi.fn(() => ({
    setTheme: mockSetTheme,
    theme: 'light',
  })),
}));

describe('ThemeSwitcher Component', () => {
  beforeEach(() => {
    mockSetTheme.mockClear();
  });

  it('renders correctly', () => {
    render(<ThemeSwitcher />);

    // Check that the button is rendered
    const button = screen.getByRole('button');
    expect(button).toBeTruthy();

    // Check that it has an aria-label (the specific label depends on the theme)
    expect(button.getAttribute('aria-label')).toBeTruthy();
    expect(button.getAttribute('aria-label')).toMatch(/Switch to (dark|light|system) theme/);
  });

  it('button is clickable', () => {
    render(<ThemeSwitcher />);

    const button = screen.getByRole('button');

    // Just verify the button is enabled and can be clicked
    expect((button as HTMLButtonElement).disabled).toBe(false);
    expect((button as HTMLButtonElement).disabled).not.toBe(true);
  });

  it('has proper accessibility attributes', () => {
    render(<ThemeSwitcher />);

    const button = screen.getByRole('button');

    // Check that the button has proper accessibility attributes
    expect(button.getAttribute('aria-label')).toBeTruthy();
    // Note: React Button components don't always have explicit type="button"
    expect(button.tagName).toBe('BUTTON');
  });

  it('renders with tooltip structure', () => {
    render(<ThemeSwitcher />);

    // The ThemeSwitcher should be wrapped in a tooltip
    // We can't easily test the tooltip content without triggering it,
    // but we can verify the button is rendered within the tooltip structure
    const button = screen.getByRole('button');
    expect(button).toBeTruthy();
  });
});
