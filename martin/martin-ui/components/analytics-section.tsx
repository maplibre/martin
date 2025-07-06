import { Image, Layers, Palette, Type } from "lucide-react";
import { ErrorState } from "@/components/error/error-state";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import type { AnalyticsData } from "@/lib/types";

interface AnalyticsSectionProps {
  analytics?: AnalyticsData;
  isLoading?: boolean;
  error?: Error | null;
  onRetry?: () => void;
  isRetrying?: boolean;
}

const CARD_CONFIG = [
  {
    icon: Layers,
    key: "tiles",
    title: "Tiles",
  },
  {
    icon: Palette,
    key: "styles",
    title: "Styles",
  },
  {
    icon: Type,
    key: "fonts",
    title: "Fonts",
  },
  {
    icon: Image,
    key: "sprites",
    title: "Sprites",
  },
] as const;

export function AnalyticsSection({
  analytics,
  isLoading,
  error = null,
  onRetry,
  isRetrying = false,
}: AnalyticsSectionProps) {
  if (error) {
    return (
      <div className="mb-8">
        <ErrorState
          description="Unable to fetch server metrics and usage data"
          error={error}
          isRetrying={isRetrying}
          onRetry={onRetry}
          showDetails={true}
          title="Failed to Load Analytics"
          variant="server"
        />
      </div>
    );
  }

  return (
    <div className="space-y-6 mb-8">
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
        {CARD_CONFIG.map(({ key, title, icon: Icon }) => {
          const data = analytics?.[key as keyof AnalyticsData];
          return (
            <Card key={key}>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">{title}</CardTitle>
                <Icon className="h-4 w-4 text-primary" />
              </CardHeader>
              <CardContent>
                {isLoading || !data ? (
                  <>
                    <Skeleton className="h-8 w-24 mb-2" />
                    <Skeleton className="h-3 w-32" />
                  </>
                ) : (
                  <>
                    <div className="text-2xl font-bold">
                      {Math.round(data.averageRequestDurationMs)} ms
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {data.requestCount.toLocaleString()} requests
                    </p>
                  </>
                )}
              </CardContent>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
