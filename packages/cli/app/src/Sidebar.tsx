import { hotkeysCoreFeature, selectionFeature, syncDataLoaderFeature } from '@headless-tree/core';
import { useTree } from '@headless-tree/react';
import { FileText, Folder, FolderOpen, FolderSearch, Settings2 } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Tree, TreeItem, TreeItemLabel } from '@/components/reui/tree';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  Sidebar as Panel,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarRail,
} from '@/components/ui/sidebar';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Mark } from './Mark.js';
import type { Listing } from './types.js';

/**
 * The documents, as the tree the author already made.
 *
 * The grouping is theirs: they put `ventas/factura.tsx` somewhere for a reason,
 * and flattening it would throw away the only organisation the project has. It
 * is a real tree rather than a list of headed sections because a folder is a
 * thing you open and close, and because arrow keys ought to work — both of
 * which come from `@headless-tree`, along with `role="tree"`.
 */
export function Sidebar({
  listing,
  selected,
  onSelect,
}: {
  listing: Listing | null;
  selected: string | null;
  onSelect(id: string): void;
}) {
  const documents = listing?.documents;
  const { items, folders } = useMemo(() => build(documents ?? []), [documents]);

  // Expanded unless somebody said otherwise, which is the behaviour worth
  // having: a folder added while the preview is open should show its contents,
  // and one that was closed on purpose should stay closed.
  const [collapsed, setCollapsed] = useState<string[]>([]);

  // Every one of these is memoised, and not as a micro-optimisation: the tree
  // compares its state by identity, so a fresh `[selected]` on each render is
  // a state change on each render, and React stops after the twenty-fifth.
  const expandedItems = useMemo(
    () => folders.filter((id) => !collapsed.includes(id)),
    [folders, collapsed],
  );
  const selectedItems = useMemo(() => (selected ? [selected] : EMPTY), [selected]);
  const state = useMemo(() => ({ expandedItems, selectedItems }), [expandedItems, selectedItems]);
  const dataLoader = useMemo(
    () => ({
      getItem: (id: string) => items[id],
      getChildren: (id: string) => items[id]?.children ?? EMPTY,
    }),
    [items],
  );

  const tree = useTree<Node>({
    rootItemId: ROOT,
    indent: INDENT,
    state,
    setExpandedItems: (next) => {
      const open = typeof next === 'function' ? next(expandedItems) : next;
      setCollapsed(folders.filter((id) => !open.includes(id)));
    },
    // Selection belongs to the app, not to the tree: it decides what the whole
    // window is showing, and it comes back down as `selected`.
    setSelectedItems: noop,
    onPrimaryAction: (item) => {
      if (!item.isFolder()) {
        onSelect(item.getId());
      }
    },
    getItemName: (item) => item.getItemData()?.name ?? '',
    isItemFolder: (item) => (item.getItemData()?.children?.length ?? 0) > 0,
    dataLoader,
    features: [syncDataLoaderFeature, selectionFeature, hotkeysCoreFeature],
  });

  // The loader closes over `items`, and the tree caches what it has already
  // read. A file added while the preview is open is the commonest thing that
  // happens here, so it has to be told.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `items` is the whole point — `tree` is stable
  useEffect(() => tree.rebuildTree(), [items]);

  return (
    <Panel collapsible="icon" className="border-r">
      <SidebarHeader className="h-12 flex-row items-center gap-2.5 border-b px-3">
        <Mark className="size-5 shrink-0" />
        {/* The display serif, which is the point of the brand's pairing: the
            product is a press, so the one word that names it is set the way a
            press would set it. Everything else on this screen is Geist. */}
        <span className="truncate font-display text-[15px] leading-none font-semibold tracking-tight group-data-[collapsible=icon]:hidden">
          Imprenta
        </span>
      </SidebarHeader>

      <SidebarContent className="group-data-[collapsible=icon]:hidden">
        {documents?.length === 0 ? (
          <Empty className="px-4 py-8">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <FolderSearch />
              </EmptyMedia>
              <EmptyTitle>Nothing here yet</EmptyTitle>
              <EmptyDescription className="text-balance">
                Add a <code className="font-mono">.tsx</code> file to{' '}
                <code className="font-mono">{short(listing?.documentsDir ?? '')}</code> that
                default-exports a document.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <ScrollArea className="min-h-0 flex-1">
            <Tree indent={INDENT} tree={tree} aria-label="Documents" className="gap-px p-2">
              {tree.getItems().map((item) => {
                const node = item.getItemData();
                const isFolder = item.isFolder();
                const current = !isFolder && item.getId() === selected;
                return (
                  <TreeItem
                    key={item.getId()}
                    item={item}
                    // The colour says which one is open to somebody looking at
                    // it, and nothing at all to somebody listening.
                    aria-current={current ? 'page' : undefined}
                    // Two steps of warm grey is all the ink ramp has between a
                    // selected row and its own panel, and at this size that is
                    // not enough to find at a glance. The rule in the margin is
                    // the brand colour doing the job it is for.
                    className="relative before:absolute before:inset-y-1 before:-left-0.5 before:w-[2px] before:rounded-full before:bg-primary before:opacity-0 before:transition-opacity data-[selected=true]:before:opacity-100"
                  >
                    <TreeItemLabel className="h-7 gap-1.5 rounded-md bg-transparent px-1.5 py-0 text-[13px] text-sidebar-foreground/75 in-data-[selected=true]:bg-sidebar-accent in-data-[selected=true]:font-medium in-data-[selected=true]:text-sidebar-accent-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground">
                      {isFolder ? (
                        item.isExpanded() ? (
                          <FolderOpen className="size-3.5 text-muted-foreground" />
                        ) : (
                          <Folder className="size-3.5 text-muted-foreground" />
                        )
                      ) : (
                        // Which of the two formats a file is cannot be known
                        // until its component has run, and guessing from the
                        // name would be wrong exactly as often as names are.
                        <FileText
                          className={
                            current ? 'size-3.5 text-primary' : 'size-3.5 text-muted-foreground'
                          }
                        />
                      )}
                      <span className="truncate">{node?.name}</span>
                    </TreeItemLabel>
                  </TreeItem>
                );
              })}
            </Tree>
          </ScrollArea>
        )}
      </SidebarContent>

      <SidebarFooter className="border-t px-3 py-2 group-data-[collapsible=icon]:hidden">
        <Tooltip>
          <TooltipTrigger
            render={
              <p className="flex items-center gap-1.5 text-left font-mono text-[11px] text-muted-foreground">
                <Settings2 className="size-3 shrink-0" />
                <span className="truncate">
                  {listing?.configPath ? short(listing.configPath) : 'no config — using defaults'}
                </span>
              </p>
            }
          />
          <TooltipContent side="top" className="font-mono">
            {listing?.configPath ?? 'no config file'}
          </TooltipContent>
        </Tooltip>
      </SidebarFooter>

      <SidebarRail />
    </Panel>
  );
}

/** The tree's own root, which is never drawn. */
const ROOT = ' root';

/** One shared empty array and one shared no-op, for the identity reason above. */
const EMPTY: string[] = [];
const noop = () => {};
const INDENT = 14;

interface Node {
  name: string;
  children?: string[];
}

/**
 * The listing, as nodes.
 *
 * Loose documents come before the folders and the folders are alphabetical,
 * which is the order a file manager would show and the order somebody scanning
 * the panel expects. A folder's id is prefixed so a project with a folder and
 * a document of the same name still has two distinct nodes.
 */
function build(documents: { id: string; group: string | null }[]) {
  const items: Record<string, Node> = {};
  const rootChildren: string[] = [];
  const grouped = new Map<string, string[]>();

  for (const document of documents) {
    if (document.group) {
      grouped.set(document.group, [...(grouped.get(document.group) ?? []), document.id]);
    } else {
      items[document.id] = { name: leaf(document.id) };
      rootChildren.push(document.id);
    }
  }

  const folders: string[] = [];
  for (const [group, ids] of [...grouped].sort(([a], [b]) => a.localeCompare(b))) {
    const folder = ` ${group}`;
    items[folder] = { name: group, children: ids };
    folders.push(folder);
    rootChildren.push(folder);
    for (const id of ids) {
      items[id] = { name: leaf(id) };
    }
  }

  items[ROOT] = { name: 'documents', children: rootChildren };
  return { items, folders };
}

const leaf = (id: string) => id.split('/').pop() ?? id;

/** The last two segments, which is as much as fits and as much as helps. */
const short = (path: string) => path.split('/').slice(-2).join('/');
