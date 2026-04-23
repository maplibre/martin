import { CopyLinkButton } from './copy-link-button';

interface CopyableUrlProps {
  label: string;
  url: string;
}

export function CopyableUrl({ label, url }: CopyableUrlProps) {
  return (
    <p>
      <span className="font-medium">{label}:</span>
      <br />
      <span className="flex items-center gap-2 mt-1">
        <code className="text-xs break-all flex-1">{url}</code>
        <CopyLinkButton link={url} toastMessage={`${label} URL copied!`} size="sm" variant="ghost" />
      </span>
    </p>
  );
}