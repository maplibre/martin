import { useCallback, useEffect, useState } from 'react';

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

  // Update URL when params change
  useEffect(() => {
    const url = new URL(window.location.href);
    const searchParams = new URLSearchParams();

    // Add non-null and non-empty params to URL
    for (const [key, value] of Object.entries(params)) {
      if (value !== null && value !== undefined && value !== '') {
        searchParams.set(key, value);
      }
    }

    url.search = searchParams.toString();

    // Update URL without triggering a page reload
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

  const updateParam = useCallback((key: string, value: string | undefined) => {
    setParams((prev) => ({
      ...prev,
      [key]: value,
    }));
  }, []);

  const updateParams = useCallback((updates: Partial<URLParamsState>) => {
    setParams((prev) => {
      const newParams: URLParamsState = { ...prev };
      for (const [key, value] of Object.entries(updates)) {
        if (value !== undefined) {
          newParams[key] = value;
        }
      }
      return newParams;
    });
  }, []);

  return {
    params,
    updateParam,
    updateParams,
  };
}
