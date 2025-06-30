"use client"

import { AlertTriangle, RefreshCw, WifiOff } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"

interface ErrorStateProps {
  title?: string
  description?: string
  error?: Error | string
  onRetry?: () => void
  isRetrying?: boolean
  variant?: "network" | "server" | "generic" | "timeout"
  showDetails?: boolean
}

export function ErrorState({
  title,
  description,
  error,
  onRetry,
  isRetrying = false,
  variant = "generic",
  showDetails = false,
}: ErrorStateProps) {
  const getErrorConfig = () => {
    switch (variant) {
      case "network":
        return {
          icon: <WifiOff className="h-8 w-8 text-red-500" />,
          defaultTitle: "Network Error",
          defaultDescription: "Unable to connect to the server. Please check your internet connection.",
        }
      case "server":
        return {
          icon: <AlertTriangle className="h-8 w-8 text-red-500" />,
          defaultTitle: "Server Error",
          defaultDescription: "The server encountered an error. Please try again later.",
        }
      case "timeout":
        return {
          icon: <AlertTriangle className="h-8 w-8 text-orange-500" />,
          defaultTitle: "Request Timeout",
          defaultDescription: "The request took too long to complete. Please try again.",
        }
      default:
        return {
          icon: <AlertTriangle className="h-8 w-8 text-red-500" />,
          defaultTitle: "Something went wrong",
          defaultDescription: "An unexpected error occurred. Please try again.",
        }
    }
  }

  const config = getErrorConfig()
  const errorMessage = typeof error === "string" ? error : error?.message

  return (
    <Card className="w-full max-w-md mx-auto">
      <CardHeader className="text-center">
        <div className="flex justify-center mb-4">{config.icon}</div>
        <CardTitle className="text-lg">{title || config.defaultTitle}</CardTitle>
        <CardDescription>{description || config.defaultDescription}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {showDetails && errorMessage && (
          <div className="p-3 bg-red-50 rounded-md border border-red-200">
            <p className="text-sm text-red-700 font-mono break-words">{errorMessage}</p>
          </div>
        )}
        {onRetry && (
          <Button onClick={onRetry} disabled={isRetrying} className="w-full">
            <RefreshCw className={`h-4 w-4 mr-2 ${isRetrying ? "animate-spin" : ""}`} />
            {isRetrying ? "Retrying..." : "Try Again"}
          </Button>
        )}
      </CardContent>
    </Card>
  )
}

export function InlineErrorState({
  message,
  onRetry,
  isRetrying = false,
  variant = "generic",
}: {
  message: string
  onRetry?: () => void
  isRetrying?: boolean
  variant?: "network" | "server" | "generic"
}) {
  const getIcon = () => {
    switch (variant) {
      case "network":
        return <WifiOff className="h-4 w-4 text-red-500" />
      case "server":
        return <AlertTriangle className="h-4 w-4 text-red-500" />
      default:
        return <AlertTriangle className="h-4 w-4 text-red-500" />
    }
  }

  return (
    <div className="flex items-center justify-between p-3 bg-red-50 border border-red-200 rounded-md">
      <div className="flex items-center space-x-2">
        {getIcon()}
        <span className="text-sm text-red-700">{message}</span>
      </div>
      {onRetry && (
        <Button size="sm" variant="outline" onClick={onRetry} disabled={isRetrying}>
          <RefreshCw className={`h-3 w-3 mr-1 ${isRetrying ? "animate-spin" : ""}`} />
          {isRetrying ? "Retrying" : "Retry"}
        </Button>
      )}
    </div>
  )
}
