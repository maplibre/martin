import { ArrowLeft, Copy, ExternalLink, Monitor, RotateCw, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { LoadingSpinner } from '@/components/loading/loading-spinner';
import { Button } from '@/components/ui/button';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { buildMartinUrl } from '@/lib/api';
import type { Catalog } from '@/lib/types.gen';

interface StyleEditorProps {
  styleName: string;
  style: Catalog['styles'][string];
  onClose: () => void;
}

export function StyleEditor({ styleName, style, onClose }: StyleEditorProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [smallScreen] = useState(() => window.matchMedia?.('(max-width: 767px)').matches ?? false);
  const [proceed, setProceed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const gated = smallScreen && !proceed;

  // Construct the Maputnik URL with the style loaded
  const maputnikUrl = new URL('https://maplibre.org/maputnik/');

  // Add the style URL as a parameter for Maputnik to load
  maputnikUrl.searchParams.set('style', buildMartinUrl(`/style/${styleName}`));

  const { copy: copyEditorLink } = useCopyToClipboard({ successMessage: 'Editor link copied!' });

  const handleIframeLoad = () => {
    setLoaded(true);
    setError(null);
  };

  const retry = () => {
    setError(null);
    setLoaded(false);
    setAttempt((n) => n + 1);
  };

  const handleIframeError = () => {
    setError('Failed to load Maputnik editor');
  };

  // The iframe never fires onError for network failures, so fall back to a timeout
  useEffect(() => {
    if (loaded || gated) return;
    const timer = window.setTimeout(() => {
      setError(
        'The editor is taking too long to load. Check your internet connection: Maputnik is hosted at maplibre.org.',
      );
    }, 20000);
    return () => window.clearTimeout(timer);
  }, [loaded, gated, attempt]);

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-[#1c1f24] text-foreground">
      <div className="flex min-w-0 items-center gap-2 border-b border-white/10 bg-[#1c1f24] px-3 py-2 text-[#e8eaee] sm:gap-4 sm:px-4">
        <Button
          className="shrink-0 border-white/25 bg-transparent text-[#e8eaee] hover:bg-white/10 hover:text-white"
          onClick={onClose}
          size="sm"
          variant="outline"
        >
          <ArrowLeft aria-hidden="true" />
          <span className="sr-only sm:not-sr-only">Back to Catalog</span>
        </Button>
        <div className="flex min-w-0 flex-1 flex-col sm:flex-row sm:items-baseline sm:gap-3">
          <h2 className="truncate font-mono text-base font-semibold sm:text-lg" title={styleName}>
            {styleName}
          </h2>
          <span className="truncate font-mono text-xs text-[#b4bac4]" title={style.path}>
            {style.path}
          </span>
        </div>
        <span
          className="hidden shrink-0 items-center gap-2 text-xs text-[#b4bac4] sm:flex"
          role="status"
        >
          {error ? 'Failed to load' : loaded ? 'Editor ready' : 'Loading…'}
        </span>
        <Button
          asChild
          className="shrink-0 border-white/25 bg-transparent text-[#e8eaee] hover:bg-white/10 hover:text-white"
          size="sm"
          variant="outline"
        >
          <a href={maputnikUrl.toString()} rel="noreferrer" target="_blank">
            <ExternalLink aria-hidden="true" />
            <span className="sr-only sm:not-sr-only">Open in Maputnik</span>
          </a>
        </Button>
      </div>
      <div className="relative flex min-h-0 flex-1 bg-[#1c1f24]">
        <div className="relative min-h-0 flex-1 overflow-hidden bg-[#1c1f24]">
          {gated ? (
            <div className="tile-grid absolute inset-0 z-10 flex items-center justify-center p-4">
              <div className="w-full max-w-sm rounded-xl border bg-card p-6 text-center shadow-card">
                <Monitor aria-hidden="true" className="mx-auto mb-4 size-10 text-brand" />
                <h3 className="mb-2 text-lg font-semibold">
                  Editing works best on a larger screen
                </h3>
                <p className="mb-5 text-sm text-muted-foreground">
                  The style editor is a full desktop tool and is cramped on small screens. Open this
                  page on a tablet or computer for the best experience.
                </p>
                <div className="flex flex-col gap-2">
                  <Button onClick={() => setProceed(true)}>Continue anyway</Button>
                  <Button onClick={() => copyEditorLink(maputnikUrl.toString())} variant="outline">
                    <Copy aria-hidden="true" />
                    Copy editor link
                  </Button>
                  <Button asChild variant="outline">
                    <a href={maputnikUrl.toString()} rel="noreferrer" target="_blank">
                      <ExternalLink aria-hidden="true" />
                      Open in Maputnik
                    </a>
                  </Button>
                  <Button onClick={onClose} variant="outline">
                    Back to Catalog
                  </Button>
                </div>
              </div>
            </div>
          ) : null}
          {error && !gated ? (
            <div className="absolute inset-0 z-10 flex items-center justify-center bg-background p-4">
              <div className="w-full max-w-sm rounded-xl border bg-card p-6 text-center shadow-card">
                <X aria-hidden="true" className="mx-auto mb-4 size-10 text-destructive" />
                <h3 className="mb-2 text-lg font-semibold">Editor Error</h3>
                <p className="mb-4 text-sm text-muted-foreground">{error}</p>
                <div className="flex flex-col gap-2 sm:flex-row sm:justify-center">
                  <Button onClick={retry}>
                    <RotateCw aria-hidden="true" />
                    Try again
                  </Button>
                  <Button asChild variant="outline">
                    <a href={maputnikUrl.toString()} rel="noreferrer" target="_blank">
                      <ExternalLink aria-hidden="true" />
                      Open in Maputnik
                    </a>
                  </Button>
                </div>
              </div>
            </div>
          ) : null}
          {!(loaded || error || gated) && (
            <div
              aria-live="polite"
              className="tile-grid absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 text-sm text-muted-foreground"
              role="status"
            >
              <LoadingSpinner />
              Loading style editor…
            </div>
          )}

          {gated ? null : (
            <iframe
              allow="local-network-access; clipboard-write"
              className="h-full w-full border-0"
              key={attempt}
              onError={handleIframeError}
              onLoad={handleIframeLoad}
              ref={iframeRef}
              sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-downloads allow-modals"
              src={maputnikUrl.toString()}
              title={`Maputnik Style Editor - ${styleName}`}
            />
          )}
        </div>
      </div>
    </div>
  );
}
