function DashboardLoading() {
  return (
    <div className="animate-pulse space-y-6">
      {/* Tab navigation skeleton */}
      <div className="grid w-full grid-cols-4 h-10 bg-gray-200 rounded"></div>

      {/* Content skeleton */}
      <div className="space-y-4">
        <div className="h-8 bg-gray-200 rounded w-1/3"></div>
        <div className="h-4 bg-gray-200 rounded w-2/3"></div>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          <div className="h-64 bg-gray-200 rounded"></div>
          <div className="h-64 bg-gray-200 rounded"></div>
          <div className="h-64 bg-gray-200 rounded"></div>
        </div>
      </div>
    </div>
  );
}

export default function MartinTileserverDashboard() {
  return (
    <div className="container mx-auto px-6 py-8">
      <DashboardLoading />
    </div>
  );
}
