import { Code2, Download, FileText, RotateCw, Table2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { SidebarTrigger } from '@/components/ui/sidebar';
import { Spinner } from '@/components/ui/spinner';
import { TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { ThemeToggle } from './theme.js';
import type { PageSetup, Report } from './types.js';
import { pageName } from './viewer.js';

/**
 * What is open, what it came out as, and the two things you can do to it.
 *
 * The tab list belongs to a `Tabs` root up in `App`, not to this header: the
 * panels it switches between fill the stage below, and a list wired to nothing
 * would be a row of buttons wearing a tablist's clothes.
 */
export function Topbar({
  id,
  report,
  page,
  busy,
  onReload,
}: {
  id: string | null;
  report: Report | null;
  /** Only a document has one, and only once it has rendered. */
  page: PageSetup | undefined;
  busy: boolean;
  onReload(): void;
}) {
  const name = id?.split('/').pop() ?? 'no document';
  const folder = id?.includes('/') ? id.slice(0, id.lastIndexOf('/')) : null;

  return (
    <header className="grid h-12 flex-none grid-cols-[1fr_auto_1fr] items-center gap-3 border-b bg-sidebar px-3">
      <div className="flex min-w-0 items-center gap-2">
        <SidebarTrigger className="-ml-1" />
        <Separator orientation="vertical" className="mr-0.5 !h-4" />
        <h1 className="truncate text-sm font-medium">{name}</h1>
        {folder ? (
          <span className="truncate font-mono text-[11px] text-muted-foreground">{folder}</span>
        ) : null}
        {busy ? (
          <Spinner aria-label="Rendering" className="size-3.5 text-muted-foreground" />
        ) : null}
      </div>

      <TabsList variant="line">
        <TabsTrigger value="preview">
          <FileText />
          Preview
        </TabsTrigger>
        <TabsTrigger value="source">
          <Code2 />
          Source
        </TabsTrigger>
      </TabsList>

      <div className="flex items-center justify-end gap-1.5">
        {report ? (
          <>
            <Badge variant="outline" className="gap-1 font-mono text-[10px] uppercase">
              {report.format === 'xlsx' ? <Table2 /> : <FileText />}
              {report.format}
            </Badge>
            <span className="text-xs text-muted-foreground">
              {/* The page size is the one fact the preview knows and the eye
                  cannot check: a document that came out Letter when it was
                  meant to be A4 looks exactly like one that came out A4.

                  Pages for a document, sheets for a workbook: the same count
                  means two different things and one word for both would lie. */}
              {[
                pageName(page),
                `${report.parts} ${noun(report)}`,
                `${(report.bytes / 1024).toFixed(1)} KB`,
              ]
                .filter(Boolean)
                .join(' · ')}
            </span>
          </>
        ) : null}

        <Tooltip>
          <TooltipTrigger
            render={
              <Button variant="ghost" size="icon-sm" aria-label="Render again" onClick={onReload}>
                <RotateCw />
              </Button>
            }
          />
          <TooltipContent side="bottom">Render again</TooltipContent>
        </Tooltip>

        {id ? (
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Download"
                  // `/api/pdf` never existed — the server has one endpoint for
                  // bytes and it wants to be told which of the two formats to
                  // send, because a component only declares that by returning.
                  render={
                    <a
                      href={`/api/file?id=${encodeURIComponent(id)}&format=${report?.format ?? 'pdf'}`}
                      download
                    >
                      <Download />
                    </a>
                  }
                />
              }
            />
            <TooltipContent side="bottom">Download</TooltipContent>
          </Tooltip>
        ) : null}

        <Separator orientation="vertical" className="mx-0.5 !h-4" />
        <ThemeToggle />
      </div>
    </header>
  );
}

function noun(report: Report): string {
  const word = report.format === 'xlsx' ? 'sheet' : 'page';
  return report.parts === 1 ? word : `${word}s`;
}
