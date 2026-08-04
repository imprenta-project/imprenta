import { readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { write, writeToFile } from '../index.js';
import { Book } from '../src/stream.js';

const text = (v: string) => ({ value: { t: 'text', v } });
const number = (v: number) => ({ value: { t: 'number', v } });

const ledger = (rows: number) =>
  Array.from({ length: rows }, (_, i) => ({
    cells: [text(`FV-${String(i).padStart(6, '0')}`), number(i * 1.5)],
  }));

const declared = (rows: number) =>
  JSON.stringify({
    sheets: [{ name: 'Libro', columns: [{ width: 16 }, { width: 12 }], rows: ledger(rows) }],
  });

describe('write', () => {
  it('hands back a zip a reader would recognise', async () => {
    const { xlsx, bytes, sheets } = await write(declared(3));

    expect(sheets).toBe(1);
    expect(bytes).toBeGreaterThan(0);
    expect(xlsx.subarray(0, 2).toString()).toBe('PK');
    expect(xlsx.length).toBe(bytes);
  });

  it('rejects a malformed workbook rather than taking the process down', async () => {
    // It crosses a native boundary. A panic there is not an exception.
    await expect(write('{ not json')).rejects.toThrow(/not valid JSON/);
  });

  it('says by name that a workbook needs a sheet', async () => {
    await expect(write('{"sheets":[]}')).rejects.toThrow(/at least one sheet/);
  });

  it('leaves the main thread free while it works', async () => {
    // The reason both calls are promises: a service has to stay answerable
    // while it exports, which is the trap the browser-based approach falls into.
    let ticks = 0;
    const timer = setInterval(() => {
      ticks += 1;
    }, 1);
    await write(declared(50_000));
    clearInterval(timer);

    expect(ticks).toBeGreaterThan(0);
  });
});

describe('writeToFile', () => {
  const path = join(tmpdir(), `imprenta-xlsx-${process.pid}.xlsx`);

  it('writes from Rust and never makes a Buffer of it', async () => {
    const result = await writeToFile(declared(100), path);
    try {
      expect(result.path).toBe(path);
      expect(readFileSync(path).length).toBe(result.bytes);
      expect('xlsx' in result).toBe(false);
    } finally {
      rmSync(path, { force: true });
    }
  });
});

describe('Book', () => {
  it('streams to the same bytes as declaring the whole thing', async () => {
    // The property the streaming API rests on. If these differ, one of the two
    // ways of exporting is producing a worse file and nobody would find out.
    const rows = ledger(500);
    const setup = { name: 'Libro', columns: [{ width: 16 }, { width: 12 }] };

    const book = new Book([setup]);
    for (let at = 0; at < rows.length; at += 100) {
      await book.rows(rows.slice(at, at + 100));
    }
    const streamed = await book.finish();

    const whole = await write(JSON.stringify({ sheets: [{ ...setup, rows }] }));

    expect(streamed.xlsx).toEqual(whole.xlsx);
  });

  it('refuses a second call while one is still running', async () => {
    // Two promises in flight have no order between them, and a spreadsheet is
    // written in order. Better to say so than to interleave two batches.
    const book = new Book([{ name: 'Libro' }]);
    const first = book.rows(ledger(2000));
    const second = book.rows(ledger(1));

    await expect(Promise.all([first, second])).rejects.toThrow(/already running/);
  });

  it('says so when the workbook has already been finished', async () => {
    const book = new Book([{ name: 'Libro' }]);
    await book.rows([{ cells: [text('uno')] }]);
    await book.finish();

    await expect(book.rows([{ cells: [text('dos')] }])).rejects.toThrow(/already been finished/);
  });

  it('moves between the sheets that were declared, and no further', async () => {
    const book = new Book([{ name: 'Uno' }, { name: 'Dos' }]);
    await book.rows([{ cells: [text('a')] }]);
    await book.nextSheet();
    await book.rows([{ cells: [text('b')] }]);

    await expect(book.nextSheet()).rejects.toThrow(/all 2 declared sheets/);
    const { sheets } = await book.finish();
    expect(sheets).toBe(2);
  });

  it('merges a block once the rows above it are known', async () => {
    const book = new Book([{ name: 'Libro' }]);
    await book.rows(ledger(3));
    await book.merge({ fromRow: 3, fromColumn: 0, toRow: 3, toColumn: 1 });
    await book.rows([{ cells: [text('Total')] }]);
    const { xlsx } = await book.finish();

    expect(xlsx).toBeDefined();
    expect(xlsx?.subarray(0, 2).toString()).toBe('PK');
  });

  it('writes to a file and hands back its size rather than its bytes', async () => {
    const path = join(tmpdir(), `imprenta-xlsx-stream-${process.pid}.xlsx`);
    const book = new Book([{ name: 'Libro' }], { path });
    try {
      await book.rows(ledger(1000));
      const done = await book.finish();

      expect(done.xlsx).toBeUndefined();
      expect(readFileSync(path).length).toBe(done.bytes);
    } finally {
      rmSync(path, { force: true });
    }
  });
});
