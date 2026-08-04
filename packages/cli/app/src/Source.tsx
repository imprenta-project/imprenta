import { ScrollArea } from '@/components/ui/scroll-area';

/**
 * The IR the engine was handed.
 *
 * Not the components and not the PDF, but the JSON between them, because that
 * is the contract: what React emitted and what the engine read, with nothing
 * in between for the two to disagree about.
 */
export function Source({ ir }: { ir: unknown }) {
  if (!ir) {
    return <p className="p-6 text-sm text-muted-foreground">Nothing rendered yet.</p>;
  }
  return (
    <ScrollArea className="h-full">
      <pre className="p-4 font-mono text-xs leading-relaxed text-muted-foreground [tab-size:2]">
        {JSON.stringify(ir, null, 2)}
      </pre>
    </ScrollArea>
  );
}
