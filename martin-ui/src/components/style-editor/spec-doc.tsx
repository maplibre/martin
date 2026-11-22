import { Fragment, type ReactNode } from 'react';

const SPEC_SITE = 'https://maplibre.org/maplibre-style-spec/';

type Inline =
  | { kind: 'text' | 'code'; text: string }
  | { kind: 'strong' | 'em'; children: Inline[] }
  | { kind: 'link'; href: string | undefined; children: Inline[] }
  | { kind: 'image'; alt: string; src: string | undefined };

type Block = { kind: 'p'; content: Inline[] } | { kind: 'ul'; items: Inline[][] };

const INLINE =
  /!\[([^\]]*)\]\(([^)\s]+)\)|\[((?:[^\]`]|`[^`]*`)+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*|(?<![\w*])_([^_]+)_(?!\w)/g;

const LIST_ITEM = /^\s*[-*]\s+/;

function resolveUrl(href: string): string | undefined {
  try {
    const url = new URL(href.replace(/^(?:\.\/)?([\w-]+)\.md(?=#|$)/, '$1/'), SPEC_SITE);
    return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : undefined;
  } catch {
    return undefined;
  }
}

function parseInline(source: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  for (const m of source.matchAll(INLINE)) {
    if (m.index > last) out.push({ kind: 'text', text: source.slice(last, m.index) });
    last = m.index + m[0].length;
    const [, alt, src, label, href, code, strong, em] = m;
    if (src !== undefined) out.push({ alt: alt ?? '', kind: 'image', src: resolveUrl(src) });
    else if (label !== undefined && href !== undefined)
      out.push({ children: parseInline(label), href: resolveUrl(href), kind: 'link' });
    else if (code !== undefined) out.push({ kind: 'code', text: code });
    else if (strong !== undefined) out.push({ children: parseInline(strong), kind: 'strong' });
    else if (em !== undefined) out.push({ children: parseInline(em), kind: 'em' });
  }
  if (last < source.length) out.push({ kind: 'text', text: source.slice(last) });
  return out;
}

function parseBlocks(markdown: string): Block[] {
  const blocks: Block[] = [];
  for (const chunk of markdown.trim().split(/\n\s*\n/)) {
    const lines = chunk.split('\n').filter((l) => l.trim() !== '');
    if (lines.length > 0 && lines.every((l) => LIST_ITEM.test(l))) {
      const items = lines.map((l) => parseInline(l.replace(LIST_ITEM, '').trim()));
      const previous = blocks.at(-1);
      if (previous?.kind === 'ul') previous.items.push(...items);
      else blocks.push({ items, kind: 'ul' });
    } else {
      blocks.push({ content: parseInline(lines.join(' ').trim()), kind: 'p' });
    }
  }
  return blocks;
}

function plain(nodes: Inline[]): string {
  return nodes
    .map((n) => {
      switch (n.kind) {
        case 'text':
        case 'code':
          return n.text;
        case 'image':
          return '';
        default:
          return plain(n.children);
      }
    })
    .join('');
}

/** The text of a style-spec doc string without its Markdown syntax and images, for one-line summaries and search. */
export function specDocText(markdown: string): string {
  return parseBlocks(markdown)
    .map((b) => (b.kind === 'p' ? plain(b.content) : b.items.map(plain).join(' ')))
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function renderInline(nodes: Inline[]): ReactNode[] {
  return nodes.map((n, i) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: parsed from a fixed string, never reordered
    <Fragment key={i}>{renderNode(n)}</Fragment>
  ));
}

function renderNode(n: Inline): ReactNode {
  switch (n.kind) {
    case 'text':
      return n.text;
    case 'code':
      return <code className="rounded-sm bg-muted px-0.5 font-mono text-[0.95em]">{n.text}</code>;
    case 'strong':
      return <strong>{renderInline(n.children)}</strong>;
    case 'em':
      return <em>{renderInline(n.children)}</em>;
    case 'link':
      return n.href ? (
        <a
          className="underline decoration-muted-foreground/50 underline-offset-2 hover:decoration-current"
          href={n.href}
          rel="noreferrer"
          target="_blank"
        >
          {renderInline(n.children)}
        </a>
      ) : (
        <span>{renderInline(n.children)}</span>
      );
    default:
      return n.src ? (
        <img
          alt={n.alt}
          className="my-1 block w-full rounded-sm bg-white p-1"
          loading="lazy"
          src={n.src}
        />
      ) : null;
  }
}

/** Renders a style-spec doc string: paragraphs, lists, code, emphasis, links and images, with links resolved against the spec site. */
export function SpecDoc({ markdown }: { markdown: string }) {
  return (
    <div className="space-y-1.5">
      {parseBlocks(markdown).map((b, i) =>
        b.kind === 'p' ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: parsed from a fixed string, never reordered
          <p key={i}>{renderInline(b.content)}</p>
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: parsed from a fixed string, never reordered
          <ul className="list-disc space-y-1 pl-4" key={i}>
            {b.items.map((item, j) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: parsed from a fixed string, never reordered
              <li key={j}>{renderInline(item)}</li>
            ))}
          </ul>
        ),
      )}
    </div>
  );
}
