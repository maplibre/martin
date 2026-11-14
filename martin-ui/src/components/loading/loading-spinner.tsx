import { Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';

interface LoadingSpinnerProps {
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

export function LoadingSpinner({ size = 'md', className }: LoadingSpinnerProps) {
  const sizeClasses = {
    lg: 'h-8 w-8',
    md: 'h-6 w-6',
    sm: 'h-4 w-4',
  };

  return <Loader2 className={cn('animate-spin', sizeClasses[size], className)} />;
}
