import { cn } from '@/lib/utils';

/**
 * The versal, on its ground.
 *
 * The capital I of a display serif, outlined rather than set as type: an SVG
 * that carries `font-family` falls back to Times anywhere that face is not
 * installed, which is most places this ends up.
 *
 * The slice at half the cap height is the page break, and it is the ground
 * showing through rather than a colour of its own — a knockout, which is what
 * makes it a printing operation instead of a graphic. Painting it would leave
 * a vermilion stripe the moment this tile sits on anything else.
 */
export function Mark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 512 512"
      className={cn('size-6 rounded-[0.3em]', className)}
      role="img"
      aria-label="Imprenta"
    >
      <rect width="512" height="512" fill="var(--color-vermilion-500)" />
      <g transform="translate(190.18 363.52) scale(0.32582)">
        <path
          d="M56 0v-16l83-41v-546l-83-41v-16h292v16l-83 41v546l83 41v16Z"
          fill="var(--color-paper-0)"
        />
      </g>
      <rect x="0" y="256" width="512" height="13.33" fill="var(--color-vermilion-500)" />
    </svg>
  );
}
