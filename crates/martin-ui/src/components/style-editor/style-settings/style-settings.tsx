import { GeneralSection } from './general-section';
import { MetadataSection } from './metadata-section';
import { RenderingSection } from './rendering-section';
import { ResourcesSection } from './resources-section';
import { SourcesSection } from './sources-section';
import type { StyleSectionProps } from './types';

/** The Style tab: every top-level setting of the style, typed from the v8 root spec. */
export function StyleSettings(props: StyleSectionProps) {
  return (
    <div className="flex flex-col divide-y">
      <GeneralSection {...props} />
      <ResourcesSection {...props} />
      <SourcesSection {...props} />
      <RenderingSection {...props} />
      <MetadataSection {...props} />
    </div>
  );
}
