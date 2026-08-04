import type { ReactElement } from 'react';
import { type Container, reconciler } from './host.js';

const LEGACY_ROOT = 0;

/**
 * Runs React and hands back the tree of host elements it built.
 *
 * Shared by every output format, because none of this is about any of them: a
 * reconciler runs components, resolves hooks and context, decides
 * conditionals, and leaves host elements with plain props. What those elements
 * mean — a page, a sheet — is decided after this, by whichever surface asked.
 *
 * Asynchronous because React may suspend: a component that awaits its data is
 * ordinary now, and a producer that has to change shape later is a worse thing
 * to inflict on callers than an `await` they did not need.
 */
export async function reconcile(element: ReactElement): Promise<Container> {
  const container: Container = { children: [] };

  // A component that throws is a bug in the author's document, and the caller
  // is the only one who can do anything about it. Left to itself React logs
  // the error and commits a tree with a hole where the content should be,
  // which is right for a screen and wrong for a file nobody will look at until
  // it is opened.
  let failure: unknown;
  const remember = (error: unknown) => {
    failure ??= error;
  };

  const root = reconciler.createContainer(
    container,
    LEGACY_ROOT,
    null,
    false,
    null,
    'imprenta',
    remember,
    remember,
    remember,
    null,
  );

  await new Promise<void>((resolve) => {
    reconciler.updateContainer(element, root, null, () => resolve());
  });

  if (failure) {
    throw failure;
  }

  return container;
}

/**
 * The one root element a document or a workbook is, or a named error.
 *
 * Every format has exactly one thing at the top, and the two ways of getting
 * it wrong are worth telling apart: nothing at all usually means a component
 * returned nothing, and several usually means a fragment where a root was
 * expected.
 *
 * `expected` arrives written out — `<Document>` — rather than as a bare name,
 * so that a caller which will take either can say so without smuggling the
 * angle brackets through the middle of one.
 */
export function only(container: Container, expected: string): Container['children'][number] {
  const [first, ...rest] = container.children;
  if (!first || rest.length) {
    throw new Error(
      `render expects one ${expected}, and was given ${container.children.length} elements`,
    );
  }
  return first;
}
