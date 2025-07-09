import { ArrowLeft, Download, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { buildMartinUrl } from "@/lib/api";
import type { Style } from "@/lib/types";

interface StyleEditorProps {
  styleName: string;
  style: Style;
  onClose: () => void;
}

export function StyleEditor({ styleName, style, onClose }: StyleEditorProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [error, setError] = useState<string | null>(null);

  // Construct the Maputnik URL with the style loaded
  const maputnikUrl = new URL("https://maplibre.org/maputnik/");

  // Add the style URL as a parameter for Maputnik to load
  maputnikUrl.searchParams.set("style", buildMartinUrl(`/style/${styleName}`));

  const handleIframeLoad = useCallback(() => {
    setError(null);
  }, []);

  const handleIframeError = useCallback(() => {
    setError("Failed to load Maputnik editor");
  }, []);

  const handleDownload = useCallback(() => {
    if (!iframeRef.current) return;

    try {
      // Request download from Maputnik
      iframeRef.current.contentWindow?.postMessage(
        {
          type: "maputnik:download-style",
        },
        "*",
      );
    } catch (err) {
      console.error("Failed to download style from Maputnik:", err);
    }
  }, []);

  // Listen for messages from Maputnik iframe
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      console.log(event);
      if (event.origin !== "https://maplibre.org") return;
    };

    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  }, []);

  return (
    <div className="fixed bg-[#191b20] inset-0 z-50 flex flex-col">
      {/* Header */}
      <Card className="rounded-none bg-[#191b20] border-0">
        <CardHeader className="py-2 border-0">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-4">
              <Button className="rounded-none" onClick={onClose} size="sm" variant="outline">
                <ArrowLeft className="w-4 h-4 mr-2" />
                Back to Catalog
              </Button>
              <div>
                <CardTitle>
                  <span className="text-xl text-white font-mono">{styleName}</span>{" "}
                  <span className="text-sm text-gray-500 dark:text-gray-300 font-mono">
                    {style.path}
                  </span>
                </CardTitle>
              </div>
            </div>
            <div className="flex items-center space-x-2">
              <Button className="rounded-none" onClick={handleDownload} size="sm" variant="default">
                <Download className="w-4 h-4 mr-2" />
                Download
              </Button>
            </div>
          </div>
        </CardHeader>
      </Card>
      {/* Editor Content */}
      <div className="flex-1 relative">
        {error && (
          <div className="absolute inset-0 bg-background flex items-center justify-center z-10">
            <Card className="w-96">
              <CardContent className="pt-6">
                <div className="text-center">
                  <X className="w-12 h-12 text-destructive mx-auto mb-4" />
                  <h3 className="text-lg font-semibold mb-2">Editor Error</h3>
                  <p className="text-sm text-muted-foreground mb-4">{error}</p>
                  <Button onClick={onClose} variant="outline">
                    Back to Catalog
                  </Button>
                </div>
              </CardContent>
            </Card>
          </div>
        )}

        <iframe
          className="w-full h-full border-0"
          onError={handleIframeError}
          onLoad={handleIframeLoad}
          ref={iframeRef}
          sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-downloads allow-modals"
          src={maputnikUrl.toString()}
          title={`Maputnik Style Editor - ${styleName}`}
        />
      </div>
    </div>
  );
}
