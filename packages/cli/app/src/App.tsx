import { CircleAlert, Download, Info } from 'lucide-react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { Spinner } from '@/components/ui/spinner';
import { Tabs, TabsContent } from '@/components/ui/tabs';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Checks } from './Checks.js';
import { Grid } from './Grid.js';
import { Sidebar } from './Sidebar.js';
import { Source } from './Source.js';
import { Topbar } from './Topbar.js';
import { ThemeProvider } from './theme.js';
import type { IrDocument, IrWorkbook, Listing, Report, View } from './types.js';
import { bytesFor, fit, type Rendered } from './viewer.js';

/**
 * The preview.
 *
 * Three regions, and the shape is the argument: the documents down the left,
 * the document itself filling the middle, and a panel along the bottom saying
 * whether it is any good. "Any good" means one thing here — whether it will
 * survive being printed.
 */
export function App() {
  const [listing, setListing] = useState<Listing | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // The bytes, and which document they are the bytes of. Keeping the second
  // half is the whole fix for a viewer that used to show the last render while
  // the next one was still in flight.
  const [rendered, setRendered] = useState<Rendered | null>(null);
  const [view, setView] = useState<View>('preview');
  const [panel, setPanel] = useState(true);
  const [busy, setBusy] = useState(false);
  // Bumped on every save. The browser cannot know a module changed on the
  // server, so the server says so, and this is how it is heard.
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    // This page is a build now, so there is no Vite client in it to carry a
    // custom HMR event. An EventSource is the smaller thing anyway: one
    // direction, one message, and it reconnects by itself.
    const changes = new EventSource('/api/changes');
    changes.addEventListener('changed', () => setRevision((n) => n + 1));
    return () => changes.close();
  }, []);

  useEffect(() => {
    fetch(`/api/documents?v=${revision}`)
      .then((r) => r.json())
      .then((found: Listing) => {
        setListing(found);
        setSelected((current) => current ?? found.documents[0]?.id ?? null);
      })
      .catch(() => setListing({ documentsDir: '', configPath: null, documents: [] }));
  }, [revision]);

  const load = useCallback(async (id: string, at: number) => {
    setBusy(true);
    try {
      const response = await fetch(`/api/render?id=${encodeURIComponent(id)}&v=${at}`);
      const body = await response.json();
      if (!response.ok) {
        setFailure(body.error ?? 'the document did not render');
        setReport(null);
        return;
      }
      setFailure(null);
      const shown = body as Report;
      setReport(shown);
      // Fetched as bytes rather than pointed at, so the viewer shows what was
      // just rendered instead of what it already had.
      const bytes = await fetch(
        `/api/file?id=${encodeURIComponent(id)}&format=${shown.format}&cached=1&v=${at}`,
      );
      const blob = await bytes.blob();
      setRendered((old) => {
        if (old) URL.revokeObjectURL(old.url);
        return { id, format: shown.format, url: URL.createObjectURL(blob) };
      });
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (selected) {
      void load(selected, revision);
    }
  }, [selected, revision, load]);

  const counts = useMemo(() => {
    const checks = report?.checks ?? [];
    return {
      errors: checks.filter((c) => c.status === 'error').length,
      warnings: checks.filter((c) => c.status === 'warning').length,
    };
  }, [report]);

  const format = report?.format ?? 'pdf';
  const file = bytesFor(rendered, selected, format);
  const page = format === 'pdf' ? (report?.ir as IrDocument | undefined)?.page : undefined;

  return (
    <ThemeProvider>
      <TooltipProvider>
        <SidebarProvider className="h-svh min-h-0 overflow-hidden">
          <Sidebar listing={listing} selected={selected} onSelect={setSelected} />

          <SidebarInset className="min-w-0 overflow-hidden">
            <Tabs
              value={view}
              onValueChange={(value) => setView(value as View)}
              className="flex min-h-0 flex-1 flex-col gap-0"
            >
              <Topbar
                id={selected}
                report={report}
                page={page}
                busy={busy}
                onReload={() => setRevision((n) => n + 1)}
              />

              {failure ? (
                <div className="min-h-0 flex-1 overflow-auto p-5">
                  <Alert variant="destructive" className="max-w-3xl">
                    <CircleAlert />
                    <AlertTitle>This document did not render</AlertTitle>
                    <AlertDescription>
                      <pre className="mt-1 font-mono text-xs leading-relaxed whitespace-pre-wrap">
                        {failure}
                      </pre>
                    </AlertDescription>
                  </Alert>
                </div>
              ) : (
                <>
                  {/* Kept mounted so switching to the source and back does not
                      throw the rendered file away and fetch it again. */}
                  <TabsContent
                    value="preview"
                    keepMounted
                    className="flex min-h-0 flex-1 flex-col overflow-hidden"
                  >
                    {report?.format === 'xlsx' ? (
                      <Workbook
                        ir={report.ir as IrWorkbook}
                        file={file}
                        name={selected?.split('/').pop() ?? 'workbook'}
                      />
                    ) : (
                      // `container-type: size` is what makes `fit()` work: it
                      // turns this pane into the frame of reference for the
                      // `cqw`/`cqh` the page box is written in.
                      <div className="grid min-h-0 flex-1 place-items-center overflow-hidden p-5 [container-type:size]">
                        {file ? (
                          <iframe
                            title={selected ?? 'document'}
                            // `Fit`, not `FitH`. The frame is now the shape of
                            // the page, so fitting the whole page fills it
                            // exactly and scrolling moves a page at a time.
                            src={`${file}#toolbar=0&view=Fit`}
                            style={fit(page)}
                            // A sheet casts a shadow on a desk, not in a darkroom: the ink theme
                            // needs a deep one to lift the page off the canvas, and paper needs
                            // barely any or the page looks like it is hovering.
                            className="w-auto rounded-lg bg-sheet shadow-[0_0_0_1px_var(--border),0_10px_30px_-14px_rgb(0_0_0/0.20)] dark:shadow-[0_0_0_1px_var(--border),0_18px_40px_-16px_rgb(0_0_0/0.55)]"
                          />
                        ) : (
                          <Spinner
                            className="size-5 text-muted-foreground"
                            aria-label="Rendering"
                          />
                        )}
                      </div>
                    )}
                  </TabsContent>

                  <TabsContent value="source" className="min-h-0 flex-1 overflow-hidden">
                    <Source ir={report?.ir} />
                  </TabsContent>
                </>
              )}
            </Tabs>

            <Checks
              checks={report?.checks ?? []}
              counts={counts}
              format={format}
              open={panel}
              disabled={Boolean(failure)}
              onOpenChange={setPanel}
            />
          </SidebarInset>
        </SidebarProvider>
      </TooltipProvider>
    </ThemeProvider>
  );
}

/**
 * A workbook, and the sentence that keeps this pane honest.
 *
 * No browser opens a spreadsheet, so the grid stands in for one — built from
 * the same IR the writer is handed, not from the file. The PDF pane shows the
 * actual bytes; this one cannot, and says so out loud rather than letting
 * somebody believe they have seen the artefact.
 */
function Workbook({ ir, file, name }: { ir: IrWorkbook; file: string | null; name: string }) {
  return (
    <>
      <Alert className="flex-none items-center rounded-none border-0 border-b bg-sidebar py-2 pr-3 pl-3.5">
        <Info className="text-muted-foreground" />
        <AlertDescription className="text-xs text-muted-foreground">
          The grid below is the workbook as declared. The file itself is what Excel opens.
        </AlertDescription>
        {file ? (
          <AlertAction className="top-1/2 right-3 -translate-y-1/2">
            <Button
              variant="outline"
              size="xs"
              render={
                <a href={file} download={`${name}.xlsx`}>
                  <Download />
                  Download
                </a>
              }
            />
          </AlertAction>
        ) : null}
      </Alert>
      <Grid workbook={ir} />
    </>
  );
}
