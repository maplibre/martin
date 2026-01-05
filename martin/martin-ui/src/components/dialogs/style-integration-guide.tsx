'use client';

import { Code, Copy, ExternalLink } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
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
import type { Style } from '@/lib/types';

interface StyleIntegrationGuideDialogProps {
  name: string;
  style: Style;
  onCloseAction: () => void;
}

const CodeBlock = ({ code }: { code: string }) => {
  const { copy, copiedText } = useCopyToClipboard();

  return (
    <div className="relative">
      <pre className="bg-muted p-4 rounded-md overflow-x-auto text-sm border">
        <Button
          className="absolute top-2 right-2 h-6 px-2 z-10"
          onClick={() => copy(code)}
          size="sm"
          variant="ghost"
        >
          <Copy className="w-3 h-3 mr-1" />
          {copiedText === code ? 'Copied!' : 'Copy'}
        </Button>
        <code>{code}</code>
      </pre>
    </div>
  );
};

export function StyleIntegrationGuideDialog({
  name,
  style,
  onCloseAction,
}: StyleIntegrationGuideDialogProps) {
  const styleUrl = buildMartinUrl(`/style/${name}`);

  const webJsCode = `// Include MapLibre GL JS in your HTML
<script src="https://unpkg.com/maplibre-gl@latest/dist/maplibre-gl.js"></script>
<link href="https://unpkg.com/maplibre-gl@latest/dist/maplibre-gl.css" rel="stylesheet" />

// Initialize the map
const map = new maplibregl.Map({
  container: 'map', // container ID
  style: '${styleUrl}', // your Martin style URL
  center: [-100, 40], // starting position [lng, lat]
  zoom: 3 // starting zoom
});`;

  const webNpmCode = `// Install MapLibre GL JS
npm install maplibre-gl

// Import in your JavaScript/TypeScript
import maplibregl from 'maplibre-gl';
import 'maplibre-gl/dist/maplibre-gl.css';

// Initialize the map
const map = new maplibregl.Map({
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
      <DialogContent className="max-w-4xl w-full p-6 max-h-[90vh] overflow-auto">
        <DialogHeader className="mb-6 truncate">
          <DialogTitle>
            <div className="text-2xl flex items-center gap-2">
              <Code className="w-6 h-6" />
              Integration Guide: <code>{name}</code>
            </div>
          </DialogTitle>
          <DialogDescription>
            Learn how to integrate this style into your MapLibre application across different
            platforms.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-6">
          {/* Style Information */}
          <div className="bg-muted/30 p-4 rounded-lg">
            <h3 className="font-semibold mb-2">Style Information</h3>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
              <div>
                <span className="font-medium">Style URL:</span>
                <br />
                <code className="text-xs bg-background px-2 py-1 rounded-sm break-all">
                  {styleUrl}
                </code>
              </div>
              <div>
                <span className="font-medium">Path:</span>
                <br />
                <code className="text-xs">{style.path}</code>
              </div>
              {style.type && (
                <div>
                  <span className="font-medium">Type:</span>
                  <br />
                  <Badge variant="secondary">{style.type}</Badge>
                </div>
              )}
              {style.layerCount && (
                <div>
                  <span className="font-medium">Layer Count:</span>
                  <br />
                  <span>{style.layerCount}</span>
                </div>
              )}
            </div>
          </div>

          {/* Integration Examples */}
          <Tabs className="w-full" defaultValue="web">
            <TabsList className="grid w-full grid-cols-2">
              <TabsTrigger value="web">MapLibre GL JS</TabsTrigger>
              <TabsTrigger value="native">MapLibre Native</TabsTrigger>
            </TabsList>

            <TabsContent className="space-y-4" value="web">
              <div className="space-y-4">
                <div>
                  <h4 className="font-medium mb-2 flex items-center gap-2">
                    Web Browser (CDN) - HTML + JavaScript
                    <Button asChild size="sm" variant="ghost">
                      <a
                        href="https://maplibre.org/maplibre-gl-js/docs/examples/simple-map/"
                        rel="noopener noreferrer"
                        target="_blank"
                      >
                        <ExternalLink className="w-3 h-3" />
                      </a>
                    </Button>
                  </h4>
                  <CodeBlock code={webJsCode} />
                </div>

                <div>
                  <h4 className="font-medium mb-2 flex items-center gap-2">
                    NPM/Webpack - JavaScript
                    <Button asChild size="sm" variant="ghost">
                      <a
                        href="https://maplibre.org/maplibre-gl-js/docs/"
                        rel="noopener noreferrer"
                        target="_blank"
                      >
                        <ExternalLink className="w-3 h-3" />
                      </a>
                    </Button>
                  </h4>
                  <CodeBlock code={webNpmCode} />
                </div>

                <div>
                  <h4 className="font-medium mb-2 flex items-center gap-2">
                    React - TypeScript
                    <Button asChild size="sm" variant="ghost">
                      <a
                        href="https://visgl.github.io/react-map-gl/docs/get-started"
                        rel="noopener noreferrer"
                        target="_blank"
                      >
                        <ExternalLink className="w-3 h-3" />
                      </a>
                    </Button>
                  </h4>
                  <CodeBlock code={reactCode} />
                </div>
              </div>
            </TabsContent>

            <TabsContent className="space-y-4" value="native">
              <div className="space-y-4">
                <div>
                  <h4 className="font-medium mb-2 flex items-center gap-2">
                    Android - Java/Kotlin
                    <Button asChild size="sm" variant="ghost">
                      <a
                        href="https://maplibre.org/maplibre-native/android/api/"
                        rel="noopener noreferrer"
                        target="_blank"
                      >
                        <ExternalLink className="w-3 h-3" />
                      </a>
                    </Button>
                  </h4>
                  <CodeBlock code={androidCode} />
                </div>

                <div>
                  <h4 className="font-medium mb-2 flex items-center gap-2">
                    iOS - Swift
                    <Button asChild size="sm" variant="ghost">
                      <a
                        href="https://maplibre.org/maplibre-native/ios/latest/documentation/maplibre/"
                        rel="noopener noreferrer"
                        target="_blank"
                      >
                        <ExternalLink className="w-3 h-3" />
                      </a>
                    </Button>
                  </h4>
                  <CodeBlock code={iosCode} />
                </div>

                <div>
                  <h4 className="font-medium mb-2 flex items-center gap-2">
                    React Native - JavaScript
                    <Button asChild size="sm" variant="ghost">
                      <a
                        href="https://maplibre.org/maplibre-react-native/"
                        rel="noopener noreferrer"
                        target="_blank"
                      >
                        <ExternalLink className="w-3 h-3" />
                      </a>
                    </Button>
                  </h4>
                  <CodeBlock code={reactNativeCode} />
                </div>
              </div>
            </TabsContent>
          </Tabs>

          {/* Additional Resources */}
          <div className="bg-muted/30 p-4 rounded-lg">
            <h3 className="font-semibold mb-2">Additional Resources</h3>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-2 text-sm">
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://maplibre.org/maplibre-style-spec/"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink className="w-3 h-3 mr-2" />
                  MapLibre Style Specification
                </a>
              </Button>
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://maplibre.org/maplibre-gl-js/docs/examples/"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink className="w-3 h-3 mr-2" />
                  MapLibre GL JS Examples
                </a>
              </Button>
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://github.com/maplibre/awesome-maplibre"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink className="w-3 h-3 mr-2" />
                  Awesome MapLibre
                </a>
              </Button>
              <Button asChild className="justify-start" size="sm" variant="ghost">
                <a
                  href="https://maplibre.org/martin/sources-styles.html"
                  rel="noopener noreferrer"
                  target="_blank"
                >
                  <ExternalLink className="w-3 h-3 mr-2" />
                  Martin Configuration Guide
                </a>
              </Button>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
