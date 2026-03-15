import { Check, Copy } from 'lucide-react';
import { useState } from 'react';

const TABS = [
  {
    code: `cargo binstall martin
martin --help`,
    id: 'cargo-binstall',
    label: 'Binstall',
  },
  {
    code: `cargo install martin --locked
martin --help`,
    id: 'source',
    label: 'From source',
  },
  {
    code: `docker run -p 3000:3000 \\
  -e DATABASE_URL=postgres://user:pass@host/db \\
  ghcr.io/maplibre/martin:latest`,
    id: 'docker',
    label: 'Docker',
  },
  {
    code: `brew tap maplibre/martin
brew install martin
martin --help`,
    id: 'brew',
    label: 'Homebrew',
  },
] as const;

export default function InstallBox() {
  const [active, setActive] = useState<string>(TABS[0].id);
  const [copied, setCopied] = useState(false);

  const current = TABS.find((t) => t.id === active) ?? TABS[0];

  function handleCopy() {
    navigator.clipboard.writeText(current.code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }

  return (
    <div className="bg-card border border-border rounded-lg overflow-hidden font-mono text-sm">
      <div className="flex items-center border-b border-border overflow-x-auto">
        {TABS.map((tab) => (
          <button
            className={[
              'px-4 py-2.5 text-xs shrink-0 transition-colors border-b-2',
              active === tab.id
                ? 'text-accent border-accent bg-primary/10'
                : 'text-muted-foreground border-transparent hover:text-foreground hover:bg-muted/30',
            ].join(' ')}
            key={tab.id}
            onClick={() => setActive(tab.id)}
            type="button"
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div className="relative">
        <pre className="px-5 py-5 pr-12 text-xs leading-relaxed text-foreground overflow-x-auto whitespace-pre">
          <code>{current.code}</code>
        </pre>
        <button
          aria-label="Copy to clipboard"
          className="absolute top-3 right-3 p-1.5 rounded text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-colors"
          onClick={handleCopy}
          type="button"
        >
          {copied ? <Check className="size-3.5 text-accent" /> : <Copy className="size-3.5" />}
        </button>
      </div>
    </div>
  );
}
