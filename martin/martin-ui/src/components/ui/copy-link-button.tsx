import { Clipboard } from 'lucide-react';
import * as React from 'react';
import { useToast } from '@/hooks/use-toast';
import { Button } from './button';

/**
 * Props for CopyLinkButton
 * @param link The string to copy to clipboard (required)
 * @param children Optional label or content for the button (defaults to "Copy Link")
 * @param className Optional additional class names for the button
 * @param toastMessage Optional custom toast message (defaults to "Link copied!")
 * @param size Button size (defaults to "sm")
 * @param variant Button variant (defaults to "outline")
 * @param iconPosition "left" or "right" (defaults to "left")
 * @param ...props Any other Button props
 */
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
  const { toast } = useToast();
  const [copied, setCopied] = React.useState(false);

  const handleCopy = async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    try {
      await navigator.clipboard.writeText(link);
      setCopied(true);
      toast({ description: toastMessage });
      setTimeout(() => setCopied(false), 3000);
    } catch {
      toast({ description: 'Failed to copy link', variant: 'destructive' });
    }
  };

  return (
    <Button
      aria-label="Copy link"
      className={className}
      onClick={handleCopy}
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
