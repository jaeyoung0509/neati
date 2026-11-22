import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

/**
 * The named type steps from `src/app.css` `@theme`. tailwind-merge must know
 * them as font sizes; otherwise `text-meta` is read as a colour and silently
 * removed when a caller also passes `text-muted-foreground`.
 */
export const TYPE_SCALE = ['micro', 'caption', 'meta', 'body', 'sm', 'title', 'metric', 'metric-lg'] as const;

const merge = extendTailwindMerge({
  extend: {
    theme: { text: [...TYPE_SCALE] },
  },
});

/** Join conditional classes and let a caller's override win over a primitive's default. */
export function cn(...inputs: ClassValue[]): string {
  return merge(clsx(inputs));
}
