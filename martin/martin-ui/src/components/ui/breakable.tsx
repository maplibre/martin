import { Fragment } from 'react';

/** Renders an identifier or URL that may only wrap after `/`, `_` or `-` (and keeps `{z}/{x}/{y}` together). */
export function Breakable({ text }: { text: string }) {
  const parts = text.split(/(?<=[/_-])(?![{/])|(?=\/\{z\})/);
  return (
    <>
      {parts.map((part, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: parts are static for a given text
        <Fragment key={i}>
          {i > 0 ? <wbr /> : null}
          {part}
        </Fragment>
      ))}
    </>
  );
}
