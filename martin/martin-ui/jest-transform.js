import { transformSync } from '@swc/core';


export function process(src, filename) {
  // Transform import.meta.env to process.env for Jest
  const transformedSrc = src.replace(
    /import\.meta\.env\.(\w+)/g,
    'process.env.$1'
  );

  try {
    const result = transformSync(transformedSrc, {
      filename,
      jsc: {
        parser: {
          syntax: 'typescript',
          tsx: filename.endsWith('.tsx'),
          decorators: false,
          dynamicImport: false
        },
        transform: {
          react: {
            runtime: 'automatic'
          }
        },
        target: 'es2022'
      },
      module: {
        type: 'commonjs'
      }
    });

    return {
      code: result.code || '',
      map: result.map
    };
  } catch (error) {
    console.error('Transform error:', error);
    return {
      code: transformedSrc
    };
  }
}
