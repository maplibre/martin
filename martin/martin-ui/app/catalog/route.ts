import { NextResponse } from "next/server";

const MOCK_CATALOG = {
  tiles: [
    {
      id: "mock-tileset",
      name: "Mock Tileset",
      description: "A mock tileset for development.",
      bounds: [-180, -85, 180, 85],
      minzoom: 0,
      maxzoom: 14,
      format: "pbf",
      attribution: "Mock Data",
    },
  ],
  styles: [
    {
      id: "mock-style",
      name: "Mock Style",
      description: "A mock style for development.",
      url: "/style/mock-style",
    },
  ],
  fonts: [
    {
      id: "mock-font",
      name: "Mock Font",
      variants: ["Regular", "Bold"],
    },
  ],
  sprites: [
    {
      id: "mock-sprite",
      name: "Mock Sprite",
      json: "/sprite/mock-sprite.json",
      png: "/sprite/mock-sprite.png",
    },
  ],
};

export async function GET() {
  // Only enable in development unless explicitly overridden
  if (
    process.env.NODE_ENV !== "development" &&
    process.env.MARTIN_ENABLE_MOCK_API !== "true"
  ) {
    return new NextResponse("Not Found", { status: 404 });
  }

  return NextResponse.json(MOCK_CATALOG);
}
