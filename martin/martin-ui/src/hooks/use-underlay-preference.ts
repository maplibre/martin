import { useCallback, useState } from 'react';

const STORAGE_KEY = 'martin-ui:inspector-underlay';
const URL_PARAM = 'underlay';

function safeGetStorage(): string | null {
  try {
    return window.localStorage?.getItem?.(STORAGE_KEY) ?? null;
  } catch {
    return null;
  }
}

function safeSetStorage(value: string | undefined) {
  try {
    if (value) {
      window.localStorage?.setItem?.(STORAGE_KEY, value);
    } else {
      window.localStorage?.removeItem?.(STORAGE_KEY);
    }
  } catch {
    // localStorage may be unavailable (private mode, test env) — silently ignore
  }
}

function readInitial<T extends string>(validIds: readonly T[]): T | undefined {
  if (typeof window === 'undefined') return undefined;

  const fromUrl = new URLSearchParams(window.location.search).get(URL_PARAM);
  if (fromUrl && (validIds as readonly string[]).includes(fromUrl)) return fromUrl as T;

  const fromStorage = safeGetStorage();
  if (fromStorage && (validIds as readonly string[]).includes(fromStorage)) return fromStorage as T;

  return undefined;
}

function writeUrl(value: string | undefined) {
  const url = new URL(window.location.href);
  if (value) {
    url.searchParams.set(URL_PARAM, value);
  } else {
    url.searchParams.delete(URL_PARAM);
  }
  window.history.replaceState({}, '', url.toString());
}

export function useUnderlayPreference<T extends string>(
  validIds: readonly T[],
): [T | undefined, (value: T | undefined) => void] {
  const [value, setValueState] = useState<T | undefined>(() => readInitial(validIds));

  const setValue = useCallback(
    (next: T | undefined) => {
      const normalized = next && (validIds as readonly string[]).includes(next) ? next : undefined;
      setValueState((prev) => {
        if (prev === normalized) return prev;
        safeSetStorage(normalized);
        writeUrl(normalized);
        return normalized;
      });
    },
    [validIds],
  );

  return [value, setValue];
}
