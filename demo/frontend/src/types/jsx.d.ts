// This file provides the JSX namespace declaration to fix compatibility issues
// with libraries that still reference the old JSX namespace after React 18+

import type { ReactElement } from 'react';

declare global {
  namespace JSX {
    type Element = ReactElement;
  }
}
