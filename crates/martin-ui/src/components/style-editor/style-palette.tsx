import { createContext, type ReactNode, useContext } from 'react';
import type { Color } from './colors';

/** One color already used by the style, offered for reuse in every color field. */
export interface PaletteColor {
  /** Canonical key from `colorKey`. */
  key: string;
  /** The color. */
  color: Color;
  /** How many places in the style use it. */
  uses: number;
}

const StylePaletteContext = createContext<PaletteColor[]>([]);

/** Makes the style's colors available to every color field below it. */
export function StylePaletteProvider({
  colors,
  children,
}: {
  colors: PaletteColor[];
  children: ReactNode;
}) {
  return <StylePaletteContext value={colors}>{children}</StylePaletteContext>;
}

/** The style's colors, most used first; empty outside the editor. */
export function useStylePalette(): PaletteColor[] {
  return useContext(StylePaletteContext);
}
