/**
 * @vitest-environment happy-dom
 *
 * Declared here rather than for the whole package: the server tests need a
 * real Node, and only these need a document to render into.
 */
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SidebarProvider } from '../../app/src/components/ui/sidebar.js';
import { Sidebar } from '../../app/src/Sidebar.js';
import type { Listing } from '../../app/src/types.js';

const listing = (documents: Listing['documents'], over: Partial<Listing> = {}): Listing => ({
  documentsDir: '/project/documents',
  configPath: '/project/imprenta.config.ts',
  documents,
  ...over,
});

/**
 * The sidebar reads its collapsed state from context, so it cannot be rendered
 * alone — which is right: it is a panel of a layout, not a widget.
 */
const show = (props: {
  listing: Listing | null;
  selected?: string | null;
  onSelect?: (id: string) => void;
}) =>
  render(
    <SidebarProvider>
      <Sidebar
        listing={props.listing}
        selected={props.selected ?? null}
        onSelect={props.onSelect ?? (() => {})}
      />
    </SidebarProvider>,
  );

/** Everything in the tree, folders included, in the order it was drawn. */
const rows = () => screen.getAllByRole('treeitem').map((item) => item.textContent?.trim() ?? '');

// Without globals, testing-library does not register its own teardown, and
// one render's markup would answer the next one's questions.
afterEach(cleanup);

describe('the sidebar', () => {
  it('shows a document by its own name, not its path', () => {
    show({ listing: listing([{ id: 'ventas/factura', group: 'ventas' }]) });

    expect(screen.getByRole('treeitem', { name: 'factura' })).toBeTruthy();
    expect(screen.getByRole('treeitem', { name: 'ventas' })).toBeTruthy();
  });

  it('keeps the folders the author put things in', () => {
    // The grouping is the only organisation a project has, and flattening it
    // would throw that away.
    show({
      listing: listing([
        { id: 'ventas/factura', group: 'ventas' },
        { id: 'compras/recibo', group: 'compras' },
        { id: 'informe', group: null },
      ]),
    });

    // Loose documents first, then the folders alphabetically, each with its
    // own beneath it.
    expect(rows()).toEqual(['informe', 'compras', 'recibo', 'ventas', 'factura']);
  });

  it('opens every folder it finds', () => {
    // A preview that shows three closed folders has shown nothing. Somebody
    // who closes one gets to keep it closed; that is the only exception.
    show({ listing: listing([{ id: 'ventas/factura', group: 'ventas' }]) });

    expect(screen.getByRole('treeitem', { name: 'ventas' }).getAttribute('aria-expanded')).toBe(
      'true',
    );
  });

  it('marks the one being looked at, where a screen reader can hear it', () => {
    // `data-selected` is what colours the row, and a colour says nothing out
    // loud. What is being shown is `aria-current="page"`.
    show({ listing: listing([{ id: 'informe', group: null }]), selected: 'informe' });

    const current = screen.getByRole('treeitem', { current: 'page' });
    expect(current.textContent?.trim()).toBe('informe');
  });

  it('marks exactly one', () => {
    show({
      listing: listing([
        { id: 'informe', group: null },
        { id: 'otro', group: null },
      ]),
      selected: 'informe',
    });

    expect(screen.getAllByRole('treeitem', { current: 'page' })).toHaveLength(1);
  });

  it('reports the one that was picked', () => {
    const onSelect = vi.fn();
    show({ listing: listing([{ id: 'ventas/factura', group: 'ventas' }]), onSelect });

    screen.getByRole('treeitem', { name: 'factura' }).click();

    expect(onSelect).toHaveBeenCalledWith('ventas/factura');
  });

  it('does not try to open a folder as a document', () => {
    // Every node is a button, and clicking the wrong one used to be a render
    // of a document called `ventas`.
    const onSelect = vi.fn();
    show({ listing: listing([{ id: 'ventas/factura', group: 'ventas' }]), onSelect });

    screen.getByRole('treeitem', { name: 'ventas' }).click();

    expect(onSelect).not.toHaveBeenCalled();
  });

  it('says what to do when a project has no documents yet', () => {
    // The first thing a new project shows, so it had better be an
    // instruction rather than an empty box.
    show({ listing: listing([]) });

    expect(screen.getByText(/default-exports a document/i)).toBeTruthy();
  });

  it('says when there is no config, since that changes where it looked', () => {
    show({ listing: listing([], { configPath: null }) });

    expect(screen.getByText(/using defaults/i)).toBeTruthy();
  });

  it('does not call an unread project empty', () => {
    // Null is not empty. Saying "nothing here" while the first fetch is still
    // in flight accuses a perfectly good project of being blank.
    show({ listing: null });

    expect(screen.queryByText(/default-exports a document/i)).toBeNull();
  });
});
