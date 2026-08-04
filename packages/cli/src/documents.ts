import type { Dirent } from 'node:fs';
import { readdir } from 'node:fs/promises';
import { join, relative, sep } from 'node:path';

/** A document the preview can show. */
export interface Found {
  /** Path from the documents folder, without the extension: `ventas/factura`. */
  id: string;
  /** The folder it sits in, or null at the top. */
  group: string | null;
  path: string;
}

/** Files that sit beside documents without being one. */
const NOT_A_DOCUMENT = /(\.test|\.spec|\.stories)\.[jt]sx?$/;

/**
 * Every document under `dir`, in a settled order.
 *
 * A component in a `.tsx` file, one per file. Helpers, tests and anything
 * starting with an underscore are left alone — they live beside documents
 * because that is where they are useful, not because they are documents.
 *
 * A folder that is not there yields nothing rather than throwing: `dev` in a
 * fresh project should say the folder is empty, not fall over.
 */
export async function findDocuments(dir: string): Promise<Found[]> {
  const found: Found[] = [];
  await walk(dir, dir, found);
  // Sorted, so the list does not rearrange itself between reloads.
  return found.sort((a, b) => a.id.localeCompare(b.id));
}

async function walk(root: string, dir: string, out: Found[]): Promise<void> {
  let entries: Dirent[];
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }

  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules' && !entry.name.startsWith('.')) {
        await walk(root, path, out);
      }
      continue;
    }
    if (!entry.name.endsWith('.tsx') && !entry.name.endsWith('.jsx')) {
      continue;
    }
    if (entry.name.startsWith('_') || NOT_A_DOCUMENT.test(entry.name)) {
      continue;
    }

    const id = relative(root, path)
      .replace(/\.[jt]sx$/, '')
      .split(sep)
      .join('/');
    const folder = id.includes('/') ? id.slice(0, id.lastIndexOf('/')) : null;
    out.push({ id, group: folder, path });
  }
}

/**
 * The sample props a document declares for its own preview.
 *
 * The data that makes a document look like a real document lives beside it and
 * ships nowhere: the preview renders `<Document {...Document.PreviewProps} />`.
 * A tool that kept the sample data instead would have to be taught about every
 * document anybody ever writes.
 */
export function previewProps(component: unknown): Record<string, unknown> {
  const declared = (component as { PreviewProps?: unknown } | null | undefined)?.PreviewProps;
  return declared && typeof declared === 'object' && !Array.isArray(declared)
    ? (declared as Record<string, unknown>)
    : {};
}
