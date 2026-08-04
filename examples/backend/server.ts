import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import { google, loadFonts } from '@imprentajs/fonts';
import { render as toPdf } from '@imprentajs/pdf';
import { render } from '@imprentajs/react/pdf';
import { render as toWorkbook } from '@imprentajs/react/xlsx';
import { write as toXlsx } from '@imprentajs/xlsx';
import { createElement } from 'react';
import Factura from './factura.tsx';
import Ventas from './ventas.tsx';

/**
 * Fetched once, when the process starts, and kept.
 *
 * Not per request: a controller must not wait on Google, and the engine wants
 * the same bytes every time anyway.
 */
const fonts = await loadFonts(google('Roboto', { weights: ['regular', 'bold'] }), {
  cache: fileURLToPath(new URL('.imprenta/fonts', import.meta.url)),
});

const lines = [
  {
    ref: '001',
    concept: 'Licencia anual Imprenta Server',
    on: new Date(Date.UTC(2026, 0, 15)),
    paid: true,
    price: 1200,
  },
  {
    ref: '002',
    concept: 'Implantación y migración',
    on: new Date(Date.UTC(2026, 2, 1)),
    paid: false,
    price: 3400,
  },
];

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? '/', 'http://localhost');
  const number = url.searchParams.get('n') ?? 'FV-1';

  // The same shape whichever format is asked for: component to IR, IR to
  // bytes, bytes out. Different components and different writers, because a
  // page and a sheet are different things — but the controller does not care.
  if (url.pathname === '/ventas.xlsx') {
    const ir = await toWorkbook(createElement(Ventas, { lines }));
    const { xlsx } = await toXlsx(ir);

    response.writeHead(200, {
      'content-type': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
      'content-disposition': 'attachment; filename="ventas.xlsx"',
    });
    response.end(xlsx);
    return;
  }

  const ir = await render(createElement(Factura, { number, lines }));
  const { pdf } = await toPdf(ir, { fonts });

  response.writeHead(200, {
    'content-type': 'application/pdf',
    'content-disposition': `inline; filename="${number}.pdf"`,
  });
  response.end(pdf);
});

server.listen(4500, () => {
  process.stdout.write('  http://localhost:4500/?n=FV-2026-00418\n');
  process.stdout.write('  http://localhost:4500/ventas.xlsx\n');
});
