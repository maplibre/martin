import type { CSSProperties } from 'react';
import { cn } from '@/lib/utils';
import type { SpriteIcon } from './sprite-sheets';

/** An icon cropped out of its sprite sheet and scaled to fit a square box, SDF icons tinted with the foreground color. */
export function IconThumb({ icon, box }: { icon: SpriteIcon; box: number }) {
  const scale = Math.min(box / icon.width, box / icon.height);
  const image = `url(${JSON.stringify(icon.imageUrl)})`;
  const size = `${icon.sheetWidth * scale}px ${icon.sheetHeight * scale}px`;
  const position = `${-icon.x * scale}px ${-icon.y * scale}px`;
  const crop: CSSProperties = icon.sdf
    ? {
        maskImage: image,
        maskPosition: position,
        maskRepeat: 'no-repeat',
        maskSize: size,
      }
    : {
        backgroundImage: image,
        backgroundPosition: position,
        backgroundRepeat: 'no-repeat',
        backgroundSize: size,
      };
  return (
    <span
      aria-hidden
      className="flex shrink-0 items-center justify-center"
      style={{ height: box, width: box }}
    >
      <span
        className={cn('block', icon.sdf && 'bg-foreground')}
        style={{ ...crop, height: icon.height * scale, width: icon.width * scale }}
      />
    </span>
  );
}
