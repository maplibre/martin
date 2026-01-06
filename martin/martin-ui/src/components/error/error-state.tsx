'use client';

import { AlertTriangle, RefreshCw, WifiOff } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';

export interface ErrorStateProps {
  title?: string;
  description?: string;
  error?: Error | string;
  onRetry?: () => void;
  isRetrying?: boolean;
  variant?: 'network' | 'server' | 'generic' | 'timeout';
  showDetails?: boolean;
}

export function ErrorState({
  title,
  description,
  error,
  onRetry,
  isRetrying = false,
  variant = 'generic',
  showDetails = false,
}: ErrorStateProps) {
  const getErrorConfig = () => {
    switch (variant) {
      case 'network':
        return {
          defaultDescription:
            'Unable to connect to the server. Please check your internet connection.',
          defaultTitle: 'Network Error',
          icon: <WifiOff className="h-8 w-8 text-destructive-foreground" />,
        };
      case 'server':
        return {
          defaultDescription: 'The server encountered an error. Please try again later.',
          defaultTitle: 'Server Error',
          icon: <AlertTriangle className="h-8 w-8 text-destructive-foreground" />,
        };
      case 'timeout':
        return {
          defaultDescription: 'The request took too long to complete. Please try again.',
          defaultTitle: 'Request Timeout',
          icon: <AlertTriangle className="h-8 w-8 text-orange-500" />,
        };
      default:
        return {
          defaultDescription: 'An unexpected error occurred. Please try again.',
          defaultTitle: 'Something went wrong',
          icon: <AlertTriangle className="h-8 w-8 text-destructive-foreground" />,
        };
    }
  };

  const config = getErrorConfig();
  const errorMessage = typeof error === 'string' ? error : error?.message;

  return (
    <Card className="w-full max-w-md mx-auto">
      <CardHeader className="text-center">
        <div className="flex justify-center mb-4">{config.icon}</div>
        <CardTitle className="text-lg">{title || config.defaultTitle}</CardTitle>
        <CardDescription>{description || config.defaultDescription}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {showDetails && errorMessage && (
          <div className="p-3 bg-destructive rounded-md border border-red-200">
            <p className="text-sm text-destructive-foreground font-mono break-words">
              {errorMessage}
            </p>
          </div>
        )}
        {onRetry && (
          <Button className="w-full" disabled={isRetrying} onClick={onRetry}>
            <RefreshCw className={`h-4 w-4 mr-2 ${isRetrying ? 'animate-spin' : ''}`} />
            {isRetrying ? 'Retrying...' : 'Try Again'}
          </Button>
        )}
      </CardContent>
    </Card>
  );
}

export function InlineErrorState({
  message,
  onRetry,
  isRetrying = false,
  variant = 'generic',
}: {
  message: string;
  onRetry?: () => void;
  isRetrying?: boolean;
  variant?: 'network' | 'server' | 'generic';
}) {
  const getIcon = () => {
    switch (variant) {
      case 'network':
        return <WifiOff className="h-4 w-4 text-destructive-foreground" />;
      case 'server':
        return <AlertTriangle className="h-4 w-4 text-destructive-foreground" />;
      default:
        return <AlertTriangle className="h-4 w-4 text-destructive-foreground" />;
    }
  };

  return (
    <div className="flex items-center justify-between p-3 bg-destructive border border-red-200 rounded-md">
      <div className="flex items-center space-x-2">
        {getIcon()}
        <span className="text-sm destructive-foreground">{message}</span>
      </div>
      {onRetry && (
        <Button
          className="gap-1"
          disabled={isRetrying}
          onClick={onRetry}
          size="sm"
          variant="outline"
        >
          <RefreshCw className={`h-4 w-4 ${isRetrying ? 'animate-spin' : ''}`} />
          {isRetrying ? 'Retrying' : 'Retry'}
        </Button>
      )}
    </div>
  );
}
