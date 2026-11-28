import { Breakable } from './breakable';
import { CopyLinkButton } from './copy-link-button';

interface CopyableUrlProps {
  label: string;
  url: string;
}

export function CopyableUrl({ label, url }: CopyableUrlProps) {
  return (
    <fieldset className="m-0 min-w-0 border-0 p-0">
      <legend className="mb-1 p-0 text-sm font-medium">{label}</legend>
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
    </fieldset>
  );
}
