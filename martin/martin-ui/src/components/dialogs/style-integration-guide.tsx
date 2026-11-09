'use client';

import { Code, Copy, CopyCheck, ExternalLink } from 'lucide-react';
import { useState } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CopyableUrl } from '@/components/ui/copyable-url';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useCopyToClipboard } from '@/hooks/use-copy-to-clipboard';
import { buildMartinUrl } from '@/lib/api';
import type { Catalog } from '@/lib/types.gen';

interface StyleIntegrationGuideDialogProps {
  name: string;
  style: Catalog['styles'][string];
  onCloseAction: () => void;
}

const CodeBlock = ({ code }: { code: string }) => {
  const { copy, copied } = useCopyToClipboard();
  const [wrap, setWrap] = useState(false);

  return (
    <div className="relative min-w-0 rounded-md border bg-sunken">
      <Button
        aria-label={copied ? 'Code copied' : 'Copy code'}
        className="absolute right-1.5 top-1.5 z-10 bg-sunken/90 backdrop-blur"
        onClick={() => copy(code)}
        size="sm"
        variant="ghost"
      >
        {copied ? (
          <>
            Copied
            <CopyCheck className="text-success" />
          </>
        ) : (
          <>
            Copy
            <Copy />
          </>
        )}
      </Button>
      <Button
        aria-pressed={wrap}
        className="absolute left-1.5 top-1.5 z-10 bg-sunken/90 backdrop-blur sm:hidden"
        onClick={() => setWrap((w) => !w)}
        size="sm"
        variant="ghost"
      >
        {wrap ? 'Scroll lines' : 'Wrap lines'}
      </Button>
      <pre
        className={`max-w-full p-4 pt-14 text-xs leading-relaxed sm:overflow-x-auto sm:whitespace-pre sm:pt-12 sm:text-sm ${wrap ? 'whitespace-pre-wrap [overflow-wrap:anywhere]' : 'overflow-x-auto whitespace-pre [mask-image:linear-gradient(to_right,black_calc(100%-2rem),transparent)] sm:[mask-image:none]'}`}
      >
        <code>{code}</code>
      </pre>
    </div>
  );
};

const DocLink = ({ href, label }: { href: string; label: string }) => (
  <Button asChild className="size-10 sm:size-8" size="icon" variant="ghost">
    <a
      aria-label={`Open ${label} documentation`}
      href={href}
      rel="noopener noreferrer"
      target="_blank"
    >
      <ExternalLink className="size-3.5" />
    </a>
  </Button>
);

export function StyleIntegrationGuideDialog({
  name,
  style,
  onCloseAction,
}: StyleIntegrationGuideDialogProps) {
  const styleUrl = buildMartinUrl(`/style/${name}`);

  const webJsCode = `<!-- Include MapLibre GL JS in your HTML -->
<link href="https://unpkg.com/maplibre-gl@^6.0.0/dist/maplibre-gl.css" rel="stylesheet" />
<div id="map" style="height: 400px"></div>

<script type="module">
  import * as maplibregl from 'https://unpkg.com/maplibre-gl@^6.0.0/dist/maplibre-gl.mjs';

  // The worker is auto-detected when loading directly from a CDN
  const map = new maplibregl.Map({
    container: 'map', // container ID
    style: '${styleUrl}', // your Martin style URL
    center: [-100, 40], // starting position [lng, lat]
    zoom: 3 // starting zoom
  });
</script>`;

  const webNpmCode = `// Install MapLibre GL JS
npm install maplibre-gl

// Import in your JavaScript/TypeScript (Vite shown; see the MapLibre
// docs for the equivalent webpack/esbuild/Rollup/Turbopack setup)
import { Map, setWorkerUrl } from 'maplibre-gl';
import 'maplibre-gl/dist/maplibre-gl.css';
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url';

// Bundlers need this one-time call, or no tiles will load
setWorkerUrl(workerUrl);

// Initialize the map
const map = new Map({
  container: 'map',
  style: '${styleUrl}',
  center: [-100, 40],
  zoom: 3
});`;

  const reactCode = `// Install React MapLibre
npm install react-map-gl maplibre-gl

// React component
import { Map } from 'react-map-gl/maplibre';
import 'maplibre-gl/dist/maplibre-gl.css';

function MyMap() {
  return (
    <Map
      initialViewState={{
        longitude: -100,
        latitude: 40,
        zoom: 3
      }}
      style={{width: '100%', height: '400px'}}
      mapStyle="${styleUrl}"
    />
  );
}`;

  const androidCode = `// Add to your app's build.gradle
implementation 'org.maplibre.gl:android-sdk:<version>'

// In your layout XML
<com.maplibre.android.maps.MapView
    android:id="@+id/mapView"
    android:layout_width="match_parent"
    android:layout_height="match_parent" />

// In your Activity/Fragment
MapView mapView = findViewById(R.id.mapView);
mapView.getMapAsync(maplibreMap -> {
    maplibreMap.setStyle("${styleUrl}");
});`;

  const iosCode = `// Add to your Package.swift or use CocoaPods
.package(url: "https://github.com/maplibre/maplibre-gl-native-distribution", from: "<version>")

// Swift code
import MapLibre

class ViewController: UIViewController {
    override func viewDidLoad() {
        super.viewDidLoad()

        let styleURL = URL(string: "${styleUrl}")
        let mapView = MLNMapView(frame: view.bounds, styleURL: styleURL)
        mapView.autoresizingMask = [.flexibleWidth, .flexibleHeight]

        view.addSubview(mapView)
    }
}`;

  const reactNativeCode = `// Install React Native MapLibre
npm install @maplibre/maplibre-react-native

// React Native component
import { MapView } from '@maplibre/maplibre-react-native';

function MyMap() {
  return (
    <MapView
      style={{ flex: 1 }}
      styleURL="${styleUrl}"
    />
  );
}`;

  return (
    <Dialog onOpenChange={(v) => !v && onCloseAction()} open={true}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle className="flex items-start gap-2 text-xl">
            <Code aria-hidden="true" className="mt-0.5 size-5 shrink-0 text-brand" />
            <span className="min-w-0">
              Integration Guide: <code className="font-mono text-lg">{name}</code>
            </span>
          </DialogTitle>
          <DialogDescription>
            Learn how to integrate this style into your MapLibre application across different
            platforms.
          </DialogDescription>
        </DialogHeader>

        <div className="min-w-0 space-y-6">
          {/* Style Information */}
          <section className="rounded-lg border bg-card p-4">
            <h3 className="mb-3 text-sm font-semibold">Style Information</h3>
            <div className="space-y-4">
              <CopyableUrl label="Style URL" url={styleUrl} />
              <dl className="grid grid-cols-2 gap-3 text-sm">
                <div className="col-span-2 min-w-0">
                  <dt className="text-muted-foreground">Path</dt>
                  <dd className="font-mono text-xs [overflow-wrap:anywhere]">{style.path}</dd>
                </div>
                {style.type ? (
                  <div>
                    <dt className="text-muted-foreground">Type</dt>
                    <dd>
                      <Badge variant="secondary">{style.type}</Badge>
                    </dd>
                  </div>
                ) : null}
                {style.layer_count ? (
                  <div>
                    <dt className="text-muted-foreground">Layer Count</dt>
                    <dd>{style.layer_count}</dd>
                  </div>
                ) : null}
              </dl>
            </div>
          </section>

          {/* Integration Examples */}
          <Tabs className="w-full min-w-0" defaultValue="web">
            <TabsList className="grid w-full grid-cols-2">
              <TabsTrigger value="web">MapLibre GL JS</TabsTrigger>
              <TabsTrigger value="native">MapLibre Native</TabsTrigger>
            </TabsList>

            <TabsContent className="space-y-4" value="web">
              <div className="space-y-4">
                <div>
                  <h4 className="mb-2 flex items-center justify-between gap-2 text-sm font-medium">
                    Web Browser (CDN) - HTML + JavaScript
                    <DocLink
                      href="https://maplibre.org/maplibre-gl-js/docs/examples/simple-map/"
                      label="Web Browser (CDN)"
                    />
                  </h4>
                  <CodeBlock code={webJsCode} />
                </div>

                <div>
                  <h4 className="mb-2 flex items-center justify-between gap-2 text-sm font-medium">
                    NPM/Webpack - JavaScript
                    <DocLink href="https://maplibre.org/maplibre-gl-js/docs/" label="NPM/Webpack" />
                  </h4>
                  <CodeBlock code={webNpmCode} />
                </div>

                <div>
                  <h4 className="mb-2 flex items-center justify-between gap-2 text-sm font-medium">
                    React - TypeScript
                    <DocLink
                      href="https://visgl.github.io/react-map-gl/docs/get-started"
                      label="React"
                    />
                  </h4>
                  <CodeBlock code={reactCode} />
                </div>

                <p className="text-sm text-muted-foreground">
                  MapLibre GL JS does not send an <code>Accept</code> header on tile requests yet.
                  Sources served as MLT additionally need a <code>transformRequest</code> that adds
                  it. See the{' '}
                  <a
                    className="text-brand underline underline-offset-2"
                    href="https://maplibre.org/martin/postprocessing/mlt/"
                    rel="noopener noreferrer"
                    target="_blank"
                  >
                    MLT guide
                  </a>
                  .
                </p>
              </div>
            </TabsContent>

            <TabsContent className="space-y-4" value="native">
              <div className="space-y-4">
                <div>
                  <h4 className="mb-2 flex items-center justify-between gap-2 text-sm font-medium">
                    Android - Java/Kotlin
                    <DocLink
                      href="https://maplibre.org/maplibre-native/android/api/"
                      label="Android"
                    />
                  </h4>
                  <CodeBlock code={androidCode} />
                </div>

                <div>
                  <h4 className="mb-2 flex items-center justify-between gap-2 text-sm font-medium">
                    iOS - Swift
                    <DocLink
                      href="https://maplibre.org/maplibre-native/ios/latest/documentation/maplibre/"
                      label="iOS"
                    />
                  </h4>
                  <CodeBlock code={iosCode} />
                </div>

                <div>
                  <h4 className="mb-2 flex items-center justify-between gap-2 text-sm font-medium">
                    React Native - JavaScript
                    <DocLink
                      href="https://maplibre.org/maplibre-react-native/"
                      label="React Native"
                    />
                  </h4>
                  <CodeBlock code={reactNativeCode} />
                </div>
              </div>
            </TabsContent>
          </Tabs>

          {/* Additional Resources */}
          <section className="rounded-lg border bg-card p-4">
            <h3 className="mb-2 text-sm font-semibold">Additional Resources</h3>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-2 text-sm">
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://maplibre.org/maplibre-style-spec/"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink />
                  MapLibre Style Specification
                </a>
              </Button>
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://maplibre.org/maplibre-gl-js/docs/examples/"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink />
                  MapLibre GL JS Examples
                </a>
              </Button>
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://github.com/maplibre/awesome-maplibre"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink />
                  Awesome MapLibre
                </a>
              </Button>
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://maplibre.org/martin/sources-styles.html"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink />
                  Martin Configuration Guide
                </a>
              </Button>
            </div>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
