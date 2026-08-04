import { ChevronDown, CircleAlert, CircleCheck, TriangleAlert } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table';
import { cn } from '@/lib/utils';
import type { Finding } from './types.js';

/**
 * Whether the document is any good, along the bottom.
 *
 * A linter belongs under the thing it judges, out of the way until it has
 * something to say. Every rule is about the medium — whether a printer can lay
 * the ink down — and not one of them is about taste.
 */
export function Checks({
  checks,
  counts,
  format,
  open,
  disabled,
  onOpenChange,
}: {
  checks: Finding[];
  counts: { errors: number; warnings: number };
  /** What was checked, since the two lists have nothing in common. */
  format: 'pdf' | 'xlsx';
  open: boolean;
  disabled: boolean;
  onOpenChange(open: boolean): void;
}) {
  const clean = checks.length === 0;

  return (
    <Collapsible
      // A document that did not render was never checked, and a panel that
      // opens onto nothing invites the reading that nothing was wrong.
      open={open && !disabled}
      onOpenChange={(next) => onOpenChange(next)}
      className="flex-none border-t bg-sidebar"
    >
      <CollapsibleTrigger
        disabled={disabled}
        className="flex h-10 w-full items-center justify-between px-3.5 text-xs text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-default"
      >
        <span className="flex items-center gap-1.5 font-medium tracking-tight">
          <ChevronDown
            className={cn('size-3.5 transition-transform', !(open && !disabled) && '-rotate-90')}
          />
          Checks
        </span>

        {disabled ? (
          <span>not run</span>
        ) : clean ? (
          <span className="flex items-center gap-1.5 text-signal-good">
            <CircleCheck className="size-3.5" />
            Ready to print
          </span>
        ) : (
          <span className="flex items-center gap-1.5">
            {counts.errors > 0 ? (
              <Badge
                className="gap-1 bg-destructive/12 pl-1.5 text-destructive ring-1 ring-destructive/25 ring-inset"
                aria-label={plural(counts.errors, 'error')}
              >
                <CircleAlert className="size-3" />
                {counts.errors}
              </Badge>
            ) : null}
            {counts.warnings > 0 ? (
              <Badge
                className="gap-1 bg-signal-warn/12 pl-1.5 text-signal-warn ring-1 ring-signal-warn/25 ring-inset"
                aria-label={plural(counts.warnings, 'warning')}
              >
                <TriangleAlert className="size-3" />
                {counts.warnings}
              </Badge>
            ) : null}
          </span>
        )}
      </CollapsibleTrigger>

      <CollapsibleContent className="border-t">
        {clean ? (
          <p className="max-w-2xl p-4 text-xs text-muted-foreground">
            {/* The two mediums have nothing to reassure anybody about in
                common. Telling the author of a spreadsheet that the margins
                are fine is how a panel stops being read. */}
            {format === 'xlsx'
              ? 'Nothing to fix. Every number is a number, the formulas point at sheets that exist, and nothing is hidden under a merge.'
              : 'Nothing to fix. Type is legible, the margins are inside what a printer can reach, and the engine had nothing to report.'}
          </p>
        ) : (
          <ScrollArea className="max-h-[34vh]">
            {/* Four columns, three of them fixed. The rule name and the origin
                used to sit in one cell and shuffled left and right with the
                length of the name, which turned a list meant to be scanned
                into a ragged edge. */}
            <Table className="table-fixed">
              <TableBody>
                {checks.map((finding) => (
                  <TableRow key={`${finding.rule}-${finding.detail}`}>
                    <TableCell className="w-9 pt-2.5 pl-3.5 align-top">
                      <Severity status={finding.status} />
                    </TableCell>
                    <TableCell className="w-52 align-top">
                      <code className="font-mono text-xs text-foreground">{finding.rule}</code>
                      {finding.occurrences > 1 ? (
                        <span className="ml-1.5 font-mono text-[11px] text-muted-foreground">
                          ×{finding.occurrences}
                        </span>
                      ) : null}
                    </TableCell>
                    <TableCell className="w-24 align-top">
                      <Badge
                        variant="outline"
                        className="h-4.5 px-1.5 text-[10px] font-normal text-muted-foreground"
                      >
                        {finding.source}
                      </Badge>
                    </TableCell>
                    <TableCell className="pr-4 align-top text-xs text-pretty text-muted-foreground">
                      {finding.detail}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </ScrollArea>
        )}
      </CollapsibleContent>
    </Collapsible>
  );
}

/**
 * Two shapes and two colours, and the word underneath both.
 *
 * A circle and a triangle are quick to tell apart at a glance and are also the
 * two shapes a colour-blind reader still has, which is the whole reason not to
 * lean on red and amber alone.
 */
export function Severity({ status }: { status: Finding['status'] }) {
  const Icon = status === 'error' ? CircleAlert : TriangleAlert;
  return (
    <Icon
      role="img"
      aria-label={status}
      className={cn('size-3.5', status === 'error' ? 'text-destructive' : 'text-signal-warn')}
    />
  );
}

const plural = (n: number, noun: string) => `${n} ${noun}${n === 1 ? '' : 's'}`;
