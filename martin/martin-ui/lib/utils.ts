import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

/**
 * Formats a file size in bytes to a human-readable string
 * @param bytes The file size in bytes
 * @returns A formatted string representation of the file size
 */
export function formatFileSize(bytes: number): string {
	if (bytes === 0) return "0 Bytes";
	if (!bytes || isNaN(bytes) || bytes < 0) return "Unknown size";

	const k = 1024;
	const sizes = ["Bytes", "KB", "MB", "GB", "TB"];
	const i = Math.floor(Math.log(bytes) / Math.log(k));

	// Handle potential overflow or incorrect values
	if (i >= sizes.length) return "File too large";

	return parseFloat((bytes / k ** i).toFixed(2)) + " " + sizes[i];
}
