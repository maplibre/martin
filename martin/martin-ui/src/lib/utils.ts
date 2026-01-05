import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Copies text to the clipboard, with a fallback for non-HTTPS/non-localhost contexts.
 *
 * The modern navigator.clipboard API only works in secure contexts (HTTPS or localhost).
 * When Martin starts at http://0.0.0.0:3000, the clipboard API won't work, so we fall
 * back to the legacy document.execCommand('copy') method.
 *
 * @param text The text to copy to the clipboard
 * @returns A promise that resolves when the text is copied, or rejects on failure
 */
export async function copyToClipboard(text: string): Promise<void> {
  // Try the modern clipboard API first (works in secure contexts)
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Fall through to legacy method
    }
  }

  // Fallback for non-HTTPS/non-localhost contexts (e.g., http://0.0.0.0:3000)
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  textarea.style.pointerEvents = 'none';
  document.body.appendChild(textarea);
  textarea.select();
  try {
    const success = document.execCommand('copy');
    if (!success) {
      throw new Error('Copy command failed');
    }
  } finally {
    document.body.removeChild(textarea);
  }
}

/**
 * Formats a file size in bytes to a human-readable string
 * @param bytes The file size in bytes
 * @returns A formatted string representation of the file size
 */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 Bytes';
  if (bytes === 1) return '1 Byte';
  if (!bytes || Number.isNaN(bytes) || bytes < 0) return 'Unknown size';

  // there are no half-bit princes. This is not harry potter..
  bytes = Math.floor(bytes);
  const k = 1000;
  const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  // Handle potential overflow or incorrect values
  if (i >= sizes.length) return 'File too large';

  return `${parseFloat((bytes / k ** i).toFixed(2))} ${sizes[i]}`;
}
