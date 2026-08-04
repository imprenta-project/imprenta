import type { FC, ReactNode } from 'react';
import type { Instance } from './host.js';
import type { Theme as Tokens } from './tailwind.js';

/**
 * A host element: a string the reconciler hands back untouched, wearing a
 * component's type so that JSX checks its props.
 *
 * Shared because the trick is, and because both surfaces need `<Theme>` — the
 * one element that is about neither a page nor a cell but about how the
 * classes under it resolve.
 */
export const host = <P>(type: string): FC<P> => type as unknown as FC<P>;

export interface ThemeProps {
  /** Colours of the caller's own, on top of Tailwind's. */
  colors?: Record<string, string>;
  /** How many points a rem is worth. Twelve unless said otherwise. */
  ptPerRem?: number;
  children?: ReactNode;
}

/**
 * Colours and scale for everything below it. Draws nothing itself.
 *
 * A theme is not a node and never reaches any engine. It says how the classes
 * under it resolve, and then its children take its place.
 */
export const Theme = host<ThemeProps>('theme');

/**
 * Unwraps any `<Theme>` around the root and hands back what it said.
 *
 * A theme round the whole thing is where an author would naturally put one:
 * once, at the top, covering the page setup or the sheet names as well as the
 * content. It draws nothing, so it is unwrapped rather than refused. Nested
 * ones add to the outer, which is what a reader would expect of anything
 * called a theme.
 */
export function unwrapTheme(root: Instance, expected: string): [Instance, Tokens] {
  let node = root;
  let theme: Tokens = {};
  while (node.type === 'theme') {
    const props = node.props as Tokens;
    theme = {
      colors: { ...theme.colors, ...props.colors },
      ptPerRem: props.ptPerRem ?? theme.ptPerRem,
    };
    const [child, ...others] = node.children;
    if (!child || others.length) {
      throw new Error(
        `a <Theme> outside the ${expected.toLowerCase()} wraps it and nothing else, and this one has ${node.children.length} children`,
      );
    }
    node = child as Instance;
  }
  return [node, theme];
}
