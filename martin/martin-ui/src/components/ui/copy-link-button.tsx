import { Clipboard } from 'lucide-react';
import type * as React from 'react';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { Button } from './button';

export interface CopyLinkButtonProps extends React.ComponentProps<typeof Button> {
  link: string;
  children?: React.ReactNode;
  className?: string;
  toastMessage?: string;
  size?: 'default' | 'sm' | 'lg' | 'icon';
  variant?: 'default' | 'destructive' | 'outline' | 'secondary' | 'ghost' | 'link';
  iconPosition?: 'left' | 'right';
}

export function CopyLinkButton({
  link,
  children,
  className,
  toastMessage = 'Link copied!',
  size = 'sm',
  variant = 'outline',
  iconPosition = 'left',
  ...props
}: CopyLinkButtonProps) {
  const { copy, copied } = useCopyToClipboard({ successMessage: toastMessage });

  return (
    <Button
      aria-label="Copy link"
      className={className}
      onClick={(e) => {
        e.preventDefault();
        copy(link);
      }}
      size={size}
      type="button"
      variant={variant}
      {...props}
    >
      {iconPosition === 'left' && (
        <Clipboard aria-hidden="true" className="w-4 h-4 mr-2" data-testid="clipboard-icon" />
      )}
      {children ?? (copied ? 'Copied!' : 'Copy Link')}
      {iconPosition === 'right' && (
        <Clipboard aria-hidden="true" className="w-4 h-4 ml-2" data-testid="clipboard-icon" />
      )}
    </Button>
  );
}
