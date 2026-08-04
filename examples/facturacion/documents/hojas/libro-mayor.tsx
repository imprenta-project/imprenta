import { Cell, Column, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';

/**
 * A ledger with everything on it at once.
 *
 * Deliberately overdone: a merged title band, a shaded header, banded rows,
 * negatives in red, dates, booleans, per-section subtotals, cross-sheet
 * formulas and five thousand rows. If any of it is going to come out wrong,
 * it will come out wrong here — and the only way to know is to open the file.
 */

const ACCOUNTS = [
  { code: '430', name: 'Clientes' },
  { code: '400', name: 'Proveedores' },
  { code: '600', name: 'Compras' },
  { code: '700', name: 'Ventas de mercaderías' },
  { code: '628', name: 'Suministros' },
  { code: '640', name: 'Sueldos y salarios' },
];

const CONCEPTS = [
  'Factura emitida',
  'Factura recibida',
  'Abono parcial',
  'Regularización de saldo',
  'Provisión mensual',
  'Ajuste por diferencias de cambio',
];

const MONEY = '#,##0.00 €';
const DATE = 'dd/mm/yyyy';

interface Entry {
  n: number;
  on: Date;
  account: string;
  name: string;
  concept: string;
  debit: number;
  credit: number;
  settled: boolean;
}

/** Deterministic, so two builds of the same document are the same file. */
function entries(count: number): Entry[] {
  const out: Entry[] = [];
  for (let n = 1; n <= count; n += 1) {
    const account = ACCOUNTS[n % ACCOUNTS.length];
    const day = 1 + ((n * 7) % 27);
    const month = (n * 3) % 12;
    const amount = Math.round((120 + ((n * 977) % 480_000) / 100) * 100) / 100;
    const debit = n % 3 === 0;
    out.push({
      n,
      on: new Date(Date.UTC(2026, month, day)),
      account: account.code,
      name: account.name,
      concept: `${CONCEPTS[n % CONCEPTS.length]} nº ${String(n).padStart(5, '0')}`,
      debit: debit ? amount : 0,
      credit: debit ? 0 : amount,
      settled: n % 4 !== 0,
    });
  }
  return out;
}

/** Every column, once, so the widths and the headings cannot drift apart. */
const COLUMNS = [
  { heading: 'Asiento', width: 10 },
  { heading: 'Fecha', width: 13, format: DATE },
  { heading: 'Cuenta', width: 9 },
  { heading: 'Denominación', width: 26 },
  { heading: 'Concepto', width: 42 },
  { heading: 'Debe', width: 15, format: MONEY },
  { heading: 'Haber', width: 15, format: MONEY },
  { heading: 'Conciliado', width: 12 },
];

export interface Props {
  rows: number;
}

export default function LibroMayor({ rows }: Props) {
  const book = entries(rows);
  const debit = book.reduce((sum, e) => sum + e.debit, 0);
  const credit = book.reduce((sum, e) => sum + e.credit, 0);

  // Rows 1 and 2 are the title band, 3 is the heading, so the entries start
  // on 4 and the arithmetic below has to agree with that or every formula
  // points one row out.
  const firstEntry = 4;
  const lastEntry = firstEntry + book.length - 1;

  return (
    <Workbook>
      <Sheet name="Libro mayor" freeze={{ rows: 3 }}>
        {COLUMNS.map((column) => (
          <Column key={column.heading} width={column.width} format={column.format} />
        ))}

        {/* A title across the whole width, which is what a merge is for. */}
        <Row className="bg-slate-800 text-white font-bold text-lg align-middle" height={30}>
          <Cell colSpan={COLUMNS.length}>Libro mayor · ejercicio 2026</Cell>
        </Row>
        <Row className="bg-slate-100 text-slate-600 italic align-middle" height={18}>
          <Cell colSpan={COLUMNS.length}>
            {`${book.length.toLocaleString('es-ES')} asientos · generado desde React`}
          </Cell>
        </Row>

        <Row
          className="bg-slate-700 text-white font-bold text-center align-middle whitespace-normal border-b-2 border-slate-900"
          height={26}
        >
          {COLUMNS.map((column) => (
            <Cell key={column.heading}>{column.heading}</Cell>
          ))}
        </Row>

        {book.map((entry) => {
          // Banding by hand, which is what a spreadsheet does anyway — Excel's
          // own banded tables are a table object, and that is a later feature.
          const band = entry.n % 2 === 0 ? 'bg-slate-50 ' : '';
          return (
            <Row key={entry.n} className={`${band}border-b border-slate-200`}>
              <Cell className="text-center">{String(entry.n).padStart(5, '0')}</Cell>
              <Cell value={entry.on} format={DATE} />
              <Cell className="text-center font-bold">{entry.account}</Cell>
              <Cell>{entry.name}</Cell>
              <Cell>{entry.concept}</Cell>
              <Cell value={entry.debit || undefined} format={MONEY} />
              <Cell
                value={entry.credit || undefined}
                format={MONEY}
                className={entry.credit > 4000 ? 'text-red-600 font-bold' : ''}
              />
              <Cell value={entry.settled} className="text-center" />
            </Row>
          );
        })}

        <Row className="bg-slate-100 font-bold border-t-2 border-slate-700" height={22}>
          <Cell colSpan={5} className="text-right align-middle">
            Sumas del ejercicio
          </Cell>
          <Cell formula={`SUM(F${firstEntry}:F${lastEntry})`} cached={debit} format={MONEY} />
          <Cell formula={`SUM(G${firstEntry}:G${lastEntry})`} cached={credit} format={MONEY} />
          <Cell />
        </Row>
        <Row className="font-bold border-t border-slate-400">
          <Cell colSpan={5} className="text-right">
            Saldo
          </Cell>
          <Cell
            colSpan={2}
            formula={`F${lastEntry + 1}-G${lastEntry + 1}`}
            cached={debit - credit}
            format={MONEY}
            className={debit - credit < 0 ? 'text-red-600' : 'text-slate-800'}
          />
          <Cell />
        </Row>
      </Sheet>

      {/* A second sheet whose numbers are all formulas into the first. */}
      <Sheet name="Resumen" freeze={{ rows: 2 }}>
        <Column width={30} />
        <Column width={18} format={MONEY} />
        <Column width={18} format={MONEY} />
        <Column width={18} format={MONEY} />

        <Row className="bg-slate-800 text-white font-bold text-lg align-middle" height={28}>
          <Cell colSpan={4}>Resumen por cuenta</Cell>
        </Row>
        <Row className="bg-slate-700 text-white font-bold text-center" height={22}>
          <Cell>Cuenta</Cell>
          <Cell>Debe</Cell>
          <Cell>Haber</Cell>
          <Cell>Saldo</Cell>
        </Row>

        {ACCOUNTS.map((account, at) => {
          const line = at + 3;
          const mine = book.filter((e) => e.account === account.code);
          const d = mine.reduce((sum, e) => sum + e.debit, 0);
          const c = mine.reduce((sum, e) => sum + e.credit, 0);
          const range = `'Libro mayor'!$C$${firstEntry}:$C$${lastEntry}`;
          return (
            <Row key={account.code} className={at % 2 === 0 ? 'bg-slate-50' : ''}>
              <Cell>{`${account.code} · ${account.name}`}</Cell>
              {/* SUMIF across sheets, which is the reason to write a formula
                  here rather than the number: it survives editing the ledger. */}
              <Cell
                formula={`SUMIF(${range},"${account.code}",'Libro mayor'!$F$${firstEntry}:$F$${lastEntry})`}
                cached={d}
              />
              <Cell
                formula={`SUMIF(${range},"${account.code}",'Libro mayor'!$G$${firstEntry}:$G$${lastEntry})`}
                cached={c}
              />
              <Cell
                formula={`B${line}-C${line}`}
                cached={d - c}
                className={d - c < 0 ? 'text-red-600 font-bold' : 'font-bold'}
              />
            </Row>
          );
        })}

        <Row className="bg-slate-100 font-bold border-t-2 border-slate-700">
          <Cell className="text-right">Total</Cell>
          <Cell formula={`SUM(B3:B${ACCOUNTS.length + 2})`} cached={debit} />
          <Cell formula={`SUM(C3:C${ACCOUNTS.length + 2})`} cached={credit} />
          <Cell formula={`SUM(D3:D${ACCOUNTS.length + 2})`} cached={debit - credit} />
        </Row>
      </Sheet>

      {/* A third, small and plain, because not every sheet is a showpiece. */}
      <Sheet name="Cuentas">
        <Column width={9} />
        <Column width={34} />
        <Row className="font-bold border-b border-slate-400">
          <Cell>Código</Cell>
          <Cell>Denominación</Cell>
        </Row>
        {ACCOUNTS.map((account) => (
          <Row key={account.code}>
            <Cell>{account.code}</Cell>
            <Cell>{account.name}</Cell>
          </Row>
        ))}
      </Sheet>
    </Workbook>
  );
}

LibroMayor.PreviewProps = { rows: 5000 } satisfies Props;
