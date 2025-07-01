/* eslint-disable react-refresh/only-export-components */
import type { Metadata } from "next";
import { Roboto } from "next/font/google";
import type React from "react";
import "./globals.css";
import { ThemeProvider } from "next-themes";

const inter = Roboto({ subsets: ["latin"] });

export const metadata: Metadata = {
	title: "Martin Tileserver Dashboard",
	description: "Dasboard of the Martin Tile server. Martin is a tile generator and server able to create Map Vector Tiles (MVTs) from large PostGIS databases on the fly, or serve tiles from PMTile and MBTile files. Martin optimizes for speed and heavy traffic, and is written in Rust. It includes CLI tools for generating, diffing, extracting, and combining MBTiles files.",
};

export default function RootLayout({
	children,
}: {
	children: React.ReactNode;
}) {
	return (
		<html lang="en">
			<body className={inter.className}>
  			<ThemeProvider
					attribute="class"
					defaultTheme="system"
					enableSystem
					disableTransitionOnChange
				>
     			{children}
  			</ThemeProvider>
			</body>
		</html>
	);
}
