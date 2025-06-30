"use client"

import { useState, useEffect } from "react"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Header } from "@/components/header"
import { AnalyticsSection } from "@/components/analytics-section"
import { DataCatalog } from "@/components/catalogs/data"
import { StylesCatalog } from "@/components/catalogs/styles"
import { FontCatalog } from "@/components/catalogs/font"
import { SpriteCatalog } from "@/components/catalogs/sprite"
import { ErrorBoundary } from "@/components/error/error-boundary"
import { Toaster } from "@/components/ui/toaster"
import { useAsyncOperation } from "@/hooks/use-async-operation"
import { useToast } from "@/hooks/use-toast"

// Simulate API functions that can fail
const fetchAnalytics = async () => {
  await new Promise((resolve) => setTimeout(resolve, 1000))

  // Simulate random failures
  if (Math.random() < 0.2) {
    throw new Error(`Failed to fetch analytics data`)
  }

  return {
    serverMetrics: {
      requestsPerSecond: 1247,
      memoryUsage: 68,
      cacheHitRate: 94.2,
      activeSources: 23,
    },
    usageData: [
      { time: "00:00", requests: 400, memory: 45 },
      { time: "04:00", requests: 300, memory: 42 },
      { time: "08:00", requests: 800, memory: 55 },
      { time: "12:00", requests: 1200, memory: 68 },
      { time: "16:00", requests: 1400, memory: 72 },
      { time: "20:00", requests: 900, memory: 58 },
    ],
    tileSourcesData: [
      { name: "osm-bright", requests: 45000, type: "vector", status: "active" },
      { name: "satellite-imagery", requests: 32000, type: "raster", status: "active" },
      { name: "terrain-contours", requests: 18000, type: "vector", status: "active" },
      { name: "poi-markers", requests: 12000, type: "sprite", status: "active" },
      { name: "custom-fonts", requests: 8000, type: "font", status: "active" },
    ],
  }
}

const fetchDataSources = async () => {
  await new Promise((resolve) => setTimeout(resolve, 1200))

  if (Math.random() < 0.15) {
    throw new Error("Network error: Unable to connect to data source API")
  }

  return [
    {
      id: "osm-bright",
      name: "OSM Bright",
      type: "Vector Tiles",
      description: "OpenStreetMap data with bright styling",
      layers: 12,
      lastUpdated: "2 hours ago",
      size: "2.3 GB",
    },
    {
      id: "satellite",
      name: "Satellite Imagery",
      type: "Raster Tiles",
      description: "High-resolution satellite imagery",
      layers: 1,
      lastUpdated: "1 day ago",
      size: "15.7 GB",
    },
    {
      id: "terrain",
      name: "Terrain Contours",
      type: "Vector Tiles",
      description: "Elevation contours and terrain features",
      layers: 8,
      lastUpdated: "6 hours ago",
      size: "1.8 GB",
    },
    {
      id: "poi-sprites",
      name: "POI Sprites",
      type: "Sprites",
      description: "Point of interest icons and markers",
      layers: 1,
      lastUpdated: "3 days ago",
      size: "45 MB",
    },
    {
      id: "custom-fonts",
      name: "Custom Fonts",
      type: "Fonts",
      description: "Typography assets for map labels",
      layers: 1,
      lastUpdated: "1 week ago",
      size: "12 MB",
    },
  ]
}

const fetchStyles = async () => {
  await new Promise((resolve) => setTimeout(resolve, 800))

  if (Math.random() < 0.1) {
    throw new Error("Server timeout: Style service is temporarily unavailable")
  }

  return true
}

const fetchFonts = async () => {
  await new Promise((resolve) => setTimeout(resolve, 900))

  if (Math.random() < 0.1) {
    throw new Error("Font service error: Unable to load font catalog")
  }

  return true
}

const fetchSprites = async () => {
  await new Promise((resolve) => setTimeout(resolve, 1100))

  if (Math.random() < 0.1) {
    throw new Error("Sprite service error: Failed to fetch sprite collections")
  }

  return true
}

export default function MartinTileserverDashboard() {
  const [selectedTimeRange, setSelectedTimeRange] = useState("24h")
  const [searchQuery, setSearchQuery] = useState("")
  const [selectedSprite, setSelectedSprite] = useState(null)
  const [downloadSprite, setDownloadSprite] = useState(null)
  const [isSearching, setIsSearching] = useState(false)
  const [searchError, setSearchError] = useState<Error | null>(null)

  const { toast } = useToast()

  // Analytics operation
  const analyticsOperation = useAsyncOperation(() => fetchAnalytics(selectedTimeRange), {
    showErrorToast: false, // We handle errors in the component
    onError: (error) => {
      console.error("Analytics fetch failed:", error)
    },
  })

  // Data sources operation
  const dataSourcesOperation = useAsyncOperation(fetchDataSources, {
    showErrorToast: false,
    onError: (error) => {
      console.error("Data sources fetch failed:", error)
    },
  })

  // Styles operation
  const stylesOperation = useAsyncOperation(fetchStyles, {
    showErrorToast: false,
    onError: (error) => {
      console.error("Styles fetch failed:", error)
    },
  })

  // Fonts operation
  const fontsOperation = useAsyncOperation(fetchFonts, {
    showErrorToast: false,
    onError: (error) => {
      console.error("Fonts fetch failed:", error)
    },
  })

  // Sprites operation
  const spritesOperation = useAsyncOperation(fetchSprites, {
    showErrorToast: false,
    onError: (error) => {
      console.error("Sprites fetch failed:", error)
    },
  })

  // Load initial data
  useEffect(() => {
    analyticsOperation.execute()
    dataSourcesOperation.execute()
    stylesOperation.execute()
    fontsOperation.execute()
    spritesOperation.execute()
  }, [])

  // Handle time range changes
  const handleTimeRangeChange = async (value: string) => {
    setSelectedTimeRange(value)
    try {
      await analyticsOperation.execute()
    } catch (error) {
      // Error is handled by the operation
    }
  }

  // Handle search with error simulation
  useEffect(() => {
    if (searchQuery) {
      setIsSearching(true)
      setSearchError(null)

      const searchTimer = setTimeout(() => {
        // Simulate search failure
        if (Math.random() < 0.1) {
          setSearchError(new Error("Search service temporarily unavailable"))
        }
        setIsSearching(false)
      }, 500)

      return () => clearTimeout(searchTimer)
    } else {
      setIsSearching(false)
      setSearchError(null)
    }
  }, [searchQuery])

  // Handle sprite selection
  const handleSpriteSelect = async (sprite: any) => {
    setSelectedSprite(sprite)
  }

  const handleSpriteClose = () => {
    setSelectedSprite(null)
  }

  const handleRetrySearch = () => {
    setSearchError(null)
    setIsSearching(true)
    setTimeout(() => {
      setIsSearching(false)
    }, 500)
  }

  const handleDownloadOpen = (sprite: any) => {
    setDownloadSprite(sprite)
  }

  const handleDownloadClose = () => {
    setDownloadSprite(null)
  }

  return (
    <ErrorBoundary
      onError={(error, errorInfo) => {
        console.error("Application error:", error, errorInfo)
        toast({
          variant: "destructive",
          title: "Application Error",
          description: "An unexpected error occurred. The page will reload automatically.",
        })

        // Auto-reload after 3 seconds
        setTimeout(() => {
          window.location.reload()
        }, 3000)
      }}
    >
      <div className="min-h-screen bg-gradient-to-br from-purple-50 to-white">
        <Header
          selectedTimeRange={selectedTimeRange}
          onTimeRangeChange={handleTimeRangeChange}
          isLoading={analyticsOperation.isLoading}
        />

        <div className="container mx-auto px-6 py-8">
          <AnalyticsSection
            serverMetrics={
              analyticsOperation.data?.serverMetrics || {
                requestsPerSecond: 0,
                memoryUsage: 0,
                cacheHitRate: 0,
                activeSources: 0,
              }
            }
            usageData={analyticsOperation.data?.usageData || []}
            tileSourcesData={analyticsOperation.data?.tileSourcesData || []}
            isLoading={analyticsOperation.isLoading}
            error={analyticsOperation.error}
            onRetry={analyticsOperation.retry}
            isRetrying={analyticsOperation.isRetrying}
          />

          <Tabs defaultValue="catalog" className="space-y-6">
            <TabsList className="grid w-full grid-cols-4">
              <TabsTrigger value="catalog">Data Catalog</TabsTrigger>
              <TabsTrigger value="styles">Styles Catalog</TabsTrigger>
              <TabsTrigger value="fonts">Font Catalog</TabsTrigger>
              <TabsTrigger value="sprites">Sprite Catalog</TabsTrigger>
            </TabsList>

            <TabsContent value="catalog">
              <DataCatalog
                dataSources={dataSourcesOperation.data || []}
                searchQuery={searchQuery}
                onSearchChange={setSearchQuery}
                isLoading={dataSourcesOperation.isLoading}
                isSearching={isSearching}
                error={dataSourcesOperation.error}
                searchError={searchError}
                onRetry={dataSourcesOperation.retry}
                onRetrySearch={handleRetrySearch}
                isRetrying={dataSourcesOperation.isRetrying}
              />
            </TabsContent>

            <TabsContent value="styles">
              <StylesCatalog
                isLoading={stylesOperation.isLoading}
                error={stylesOperation.error}
                onRetry={stylesOperation.retry}
                isRetrying={stylesOperation.isRetrying}
              />
            </TabsContent>

            <TabsContent value="fonts">
              <FontCatalog
                isLoading={fontsOperation.isLoading}
                error={fontsOperation.error}
                onRetry={fontsOperation.retry}
                isRetrying={fontsOperation.isRetrying}
              />
            </TabsContent>

            <TabsContent value="sprites">
              <SpriteCatalog
                selectedSprite={selectedSprite}
                onSpriteSelect={handleSpriteSelect}
                onSpriteClose={handleSpriteClose}
                downloadSprite={downloadSprite}
                onDownloadOpen={handleDownloadOpen}
                onDownloadClose={handleDownloadClose}
                isLoading={spritesOperation.isLoading}
                error={spritesOperation.error}
                onRetry={spritesOperation.retry}
                isRetrying={spritesOperation.isRetrying}
              />
            </TabsContent>
          </Tabs>
        </div>

        <Toaster />
      </div>
    </ErrorBoundary>
  )
}
