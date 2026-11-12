import { useCallback, useEffect, useRef, useState } from 'react';

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
    // localStorage may be unavailable (private mode, test env) - silently ignore
  }
}

// Persisted when the user explicitly picks "None", so a default basemap does not come back.
const NONE = 'none';

function readInitial<T extends string>(
  validIds: readonly T[],
  defaultId: T | undefined,
): T | undefined {
  if (typeof window === 'undefined') return defaultId;

  for (const candidate of [
    new URLSearchParams(window.location.search).get(URL_PARAM),
    safeGetStorage(),
  ]) {
    if (candidate === NONE) return undefined;
    if (candidate && (validIds as readonly string[]).includes(candidate)) return candidate as T;
  }

  return defaultId;
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
  defaultId?: T,
): [T | undefined, (value: T | undefined) => void] {
  const [value, setValueState] = useState<T | undefined>(() => readInitial(validIds, defaultId));
  const defaultIdRef = useRef(defaultId);
  defaultIdRef.current = defaultId;

  // Sync to storage + URL when value changes. Keeping side effects out of the
  // setState updater is required: StrictMode invokes updaters twice in dev to
  // surface impure logic, which would double-fire writes here.
  const isFirstRun = useRef(true);
  useEffect(() => {
    if (isFirstRun.current) {
      isFirstRun.current = false;
      return;
    }
    const persisted = value ?? (defaultIdRef.current ? NONE : undefined);
    safeSetStorage(persisted);
    writeUrl(persisted);
  }, [value]);

  const setValue = useCallback(
    (next: T | undefined) => {
      const normalized = next && (validIds as readonly string[]).includes(next) ? next : undefined;
      setValueState((prev) => (prev === normalized ? prev : normalized));
    },
    [validIds],
  );

  return [value, setValue];
}
