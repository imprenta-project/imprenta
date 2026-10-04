import { afterAll, describe, expect, it } from 'vitest';
import { close, render } from '../dist/index.js';
import { preview } from '../src/preview.js';

afterAll(close);

describe('preview of the actual printer payload', () => {
  it('leaves equal paper margins around the configured print area', async () => {
    const result = await render({
      children: [
        { type: 'init', width: 64, marginLeft: 4 },
        { type: 'text', text: 'Ticket con margen' },
      ],
    });
    const svg = await preview(result.escpos, result.profile);
    expect(result.profile.marginLeft).toBe(32);
    expect(result.profile.printableWidth).toBe(511);
    expect(svg).toMatch(/width="576"/);
  });

  it('renders a native receipt as SVG with the same roll width', async () => {
    const result = await render({
      profile: { columns: 32 },
      children: [
        { type: 'text', text: 'España 9,50 €', justification: 'center' },
        { type: 'qrcode', value: 'https://example.com' },
        { type: 'feed', lines: 3 },
        { type: 'cut' },
      ],
    });
    const svg = await preview(result.escpos, result.profile);
    expect(svg).toContain('<svg');
    expect(svg).toMatch(/384/);
    expect(svg.length).toBeGreaterThan(1000);
    expect(svg).not.toContain('undefined');
  });
});
