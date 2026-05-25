import { Clipboard, ClipboardCheck } from 'lucide-react';
import type * as React from 'react';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { cn } from '@/lib/utils';
import { Button } from './button';

export interface CopyLinkButtonProps extends React.ComponentProps<typeof Button> {
  link: string;
  children?: React.ReactNode;
  className?: string;
  toastMessage?: string;
  size?: 'default' | 'sm' | 'lg' | 'icon';
  variant?: 'default' | 'destructive' | 'outline' | 'secondary' | 'ghost' | 'link';
}

export function CopyLinkButton({
  link,
  children,
  className,
  toastMessage = 'Link copied!',
  size = 'sm',
  variant = 'outline',
  ...props
}: CopyLinkButtonProps) {
  const { copy, copied } = useCopyToClipboard({ successMessage: toastMessage });

  return (
    <Button
      aria-label="Copy link"
      className={cn('gap-2', className)}
      onClick={(e) => {
        e.preventDefault();
        copy(link);
      }}
      size={size}
      type="button"
      variant={variant}
      {...props}
    >
      {copied ? (
        <>
          <ClipboardCheck
            aria-hidden="true"
            className={`w-4 h-4 dark:text-green-600`}
            data-testid="clipboard-icon"
          />
          Copied!
        </>
      ) : (
        <>
          <Clipboard aria-hidden="true" className="w-4 h-4" data-testid="clipboard-icon" />
          Copy Link
        </>
      )}
    </Button>
  );
}
