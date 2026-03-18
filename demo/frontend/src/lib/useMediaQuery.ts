import { useEffect, useState } from 'react';

const SM_BREAKPOINT_PX = 640;
/** Min height for single-hero layout (map behind title). Below this we use split layout (title then map). */
const MIN_HEIGHT_FOR_HERO_PX = 850;

/**
 * Returns true when viewport is at least `sm` (640px). SSR-safe: defaults to true
 * so desktop layout is shown on first paint to avoid flash.
 */
export function useMediaQuerySm(): boolean {
  const [matches, setMatches] = useState(true);

  useEffect(() => {
    const mq = window.matchMedia(`(min-width: ${SM_BREAKPOINT_PX}px)`);
    setMatches(mq.matches);
    const handler = (e: MediaQueryListEvent) => setMatches(e.matches);
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  }, []);

  return matches;
}

/**
 * Returns true when viewport height is at least MIN_HEIGHT_FOR_HERO_PX. SSR-safe: defaults to true.
 */
export function useMediaQueryMinHeight(): boolean {
  const [matches, setMatches] = useState(true);

  useEffect(() => {
    const mq = window.matchMedia(`(min-height: ${MIN_HEIGHT_FOR_HERO_PX}px)`);
    setMatches(mq.matches);
    const handler = (e: MediaQueryListEvent) => setMatches(e.matches);
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  }, []);

  return matches;
}
