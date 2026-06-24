import { useEffect, useState } from 'react';

type URLParamsState = Record<string, string | undefined>;

export function useURLParams(initialParams: URLParamsState = {}) {
  const [params, setParams] = useState<URLParamsState>(() => {
    // Initialize from URL on mount
    const urlParams = new URLSearchParams(window.location.search);
    const initialState: URLParamsState = {};

    // Set initial values from URL or defaults
    for (const [key, defaultValue] of Object.entries(initialParams)) {
      initialState[key] = urlParams.get(key) || defaultValue;
    }

    return initialState;
  });

  // Update URL when params change. Preserves any URL params not managed by
  // this hook (e.g. ?underlay= owned by useUnderlayPreference) by mutating
  // the existing query string instead of rebuilding it from scratch.
  useEffect(() => {
    const url = new URL(window.location.href);
    const searchParams = url.searchParams;

    for (const [key, value] of Object.entries(params)) {
      if (value !== null && value !== undefined && value !== '') {
        searchParams.set(key, value);
      } else {
        searchParams.delete(key);
      }
    }

    window.history.replaceState({}, '', url.toString());
  }, [params]);

  // Listen for browser back/forward navigation
  useEffect(() => {
    const handlePopState = () => {
      const urlParams = new URLSearchParams(window.location.search);
      const newParams: URLParamsState = {};

      for (const [key, defaultValue] of Object.entries(initialParams)) {
        newParams[key] = urlParams.get(key) || defaultValue;
      }

      setParams(newParams);
    };

    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, [initialParams]);

  const updateParam = (key: string, value: string | undefined) => {
    setParams((prev) => ({
      ...prev,
      [key]: value,
    }));
  };

  const updateParams = (updates: Partial<URLParamsState>) => {
    setParams((prev) => {
      const newParams: URLParamsState = { ...prev };
      for (const [key, value] of Object.entries(updates)) {
        if (value !== undefined) {
          newParams[key] = value;
        }
      }
      return newParams;
    });
  };

  return {
    params,
    updateParam,
    updateParams,
  };
}
