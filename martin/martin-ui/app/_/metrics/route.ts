import { NextResponse } from "next/server";

const MOCK_METRICS = `
# HELP http_requests_total The total number of HTTP requests.
# TYPE http_requests_total counter
http_requests_total{method="get",endpoint="/catalog"} 42
http_requests_total{method="get",endpoint="/font/{fontstack}/{start}-{end}"} 17
http_requests_total{method="get",endpoint="/sprite/{source_ids}.json"} 5
http_requests_total{method="get",endpoint="/sprite/{source_ids}.png"} 8
http_requests_total{method="get",endpoint="/style/{style_id}"} 13
http_requests_total{method="get",endpoint="/{source_ids}/{z}/{x}/{y}"} 99
`;

export async function GET() {
  // Only enable in development unless explicitly overridden
  if (
    process.env.NODE_ENV !== "development" &&
    process.env.MARTIN_ENABLE_MOCK_API !== "true"
  ) {
    return new NextResponse("Not Found", { status: 404 });
  }

  return new NextResponse(MOCK_METRICS, {
    status: 200,
    headers: {
      "Content-Type": "text/plain; version=0.0.4",
    },
  });
}
