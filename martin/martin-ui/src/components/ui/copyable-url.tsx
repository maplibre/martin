import { Breakable } from './breakable';
import { CopyLinkButton } from './copy-link-button';

interface CopyableUrlProps {
  label: string;
  url: string;
}

export function CopyableUrl({ label, url }: CopyableUrlProps) {
  return (
    <div className="min-w-0">
      <div className="mb-1 text-sm font-medium">{label}</div>
      <div className="flex min-w-0 items-center gap-1 rounded-md border bg-sunken pl-3">
        <code className="min-w-0 flex-1 [overflow-wrap:anywhere] py-2 text-xs leading-relaxed">
          <Breakable text={url} />
        </code>
        <CopyLinkButton
          className="shrink-0 self-stretch rounded-l-none border-l"
          link={url}
          size="sm"
          toastMessage={`${label} URL copied!`}
          variant="ghost"
        />
      </div>
    </div>
  );
}
