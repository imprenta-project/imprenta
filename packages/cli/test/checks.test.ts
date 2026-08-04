import { describe, expect, it } from 'vitest';
import { check } from '../src/checks.js';

const page = { width: 595, height: 842, margin: { top: 40, right: 40, bottom: 40, left: 40 } };
const document = (children: unknown[], setup = page) => ({ page: setup, children });

/** A document that exists only to isolate some other rule. */
const some = [{ t: 'text', runs: [{ text: 'algo' }], style: { size: 10 } }];

const rules = (found: ReturnType<typeof check>) => found.map((f) => f.rule);

describe('check', () => {
  it('finds nothing wrong with a plain document', () => {
    const found = check(
      document([{ t: 'text', runs: [{ text: 'Hola' }], style: { size: 10 } }]),
      [],
    );

    expect(found).toEqual([]);
  });

  describe('text nobody can read', () => {
    it('objects to type below six points', () => {
      // Six point is about where print stops being legible for anyone, and
      // the engine will happily set two.
      const found = check(document([{ t: 'text', runs: [{ text: 'a' }], style: { size: 4 } }]), []);

      expect(rules(found)).toEqual(['tiny-text']);
      expect(found[0].status).toBe('error');
      expect(found[0].detail).toContain('4');
    });

    it('leaves small-but-legible type alone', () => {
      const found = check(document([{ t: 'text', runs: [{ text: 'a' }], style: { size: 7 } }]), []);

      expect(found).toEqual([]);
    });

    it('looks inside boxes and rows', () => {
      const found = check(
        document([
          {
            t: 'box',
            children: [
              { t: 'row', children: [{ t: 'text', runs: [{ text: 'a' }], style: { size: 3 } }] },
            ],
          },
        ]),
        [],
      );

      expect(rules(found)).toEqual(['tiny-text']);
    });

    it('checks a table cell as well as a paragraph', () => {
      const found = check(
        document([{ t: 'table', columns: [{}], rows: [{ cells: [{ text: 'a', size: 3 }] }] }]),
        [],
      );

      expect(rules(found)).toEqual(['tiny-text']);
    });
  });

  describe('ink the printer cannot lay down', () => {
    it('objects to a margin inside the unprintable edge', () => {
      // Nearly every office printer refuses the outer five millimetres, and
      // a document that puts a total there loses the total.
      const tight = { ...page, margin: { top: 40, right: 4, bottom: 40, left: 40 } };
      const found = check(document(some, tight), []);

      expect(rules(found)).toEqual(['unprintable-margin']);
      expect(found[0].detail).toContain('right');
    });

    it('accepts a margin a printer can manage', () => {
      const found = check(
        document(some, { ...page, margin: { top: 15, right: 15, bottom: 15, left: 15 } }),
        [],
      );

      expect(found).toEqual([]);
    });

    it('names every side that is too tight, not just the first', () => {
      const tight = { ...page, margin: { top: 2, right: 2, bottom: 40, left: 40 } };
      const found = check(document(some, tight), []);

      expect(found[0].detail).toContain('top');
      expect(found[0].detail).toContain('right');
    });
  });

  describe('text that will not be read on paper', () => {
    it('objects to type too pale against its background', () => {
      const found = check(
        document([
          {
            t: 'box',
            style: { background: '#f8fafc' },
            children: [{ t: 'text', runs: [{ text: 'a' }], style: { color: '#e2e8f0' } }],
          },
        ]),
        [],
      );

      expect(rules(found)).toEqual(['faint-text']);
      expect(found[0].status).toBe('warning');
    });

    it('accepts type that stands out from it', () => {
      const found = check(
        document([
          {
            t: 'box',
            style: { background: '#f8fafc' },
            children: [{ t: 'text', runs: [{ text: 'a' }], style: { color: '#1b3a5c' } }],
          },
        ]),
        [],
      );

      expect(found).toEqual([]);
    });

    it('measures a run against the box, not against the page', () => {
      // White on navy is fine; the rule has to know what is behind the text.
      const found = check(
        document([
          {
            t: 'box',
            style: { background: '#1b3a5c' },
            children: [{ t: 'text', runs: [{ text: 'a', color: '#ffffff' }] }],
          },
        ]),
        [],
      );

      expect(found).toEqual([]);
    });

    it('checks a table header against the fill behind it', () => {
      const found = check(
        document([
          {
            t: 'table',
            columns: [{}],
            header: {
              cells: [{ text: 'Ref.', color: '#94a3b8' }],
              style: { background: '#cbd5e1' },
            },
            rows: [],
          },
        ]),
        [],
      );

      expect(rules(found)).toEqual(['faint-text']);
    });
  });

  describe('tables that do not line up', () => {
    it('objects to a row with the wrong number of cells', () => {
      // The engine drops the extra silently, so nothing else would say.
      const found = check(
        document([
          {
            t: 'table',
            columns: [{}, {}],
            rows: [{ cells: [{ text: 'a' }, { text: 'b' }] }, { cells: [{ text: 'c' }] }],
          },
        ]),
        [],
      );

      expect(rules(found)).toEqual(['ragged-row']);
      expect(found[0].detail).toContain('2');
    });

    it('says nothing when every row matches', () => {
      const found = check(
        document([
          { t: 'table', columns: [{}, {}], rows: [{ cells: [{ text: 'a' }, { text: 'b' }] }] },
        ]),
        [],
      );

      expect(found).toEqual([]);
    });
  });

  describe('links', () => {
    it('objects to a link that will not open', () => {
      // A relative href means nothing in a file that may be printed, mailed
      // or opened from a download folder.
      const found = check(
        document([
          { t: 'link', href: '/condiciones', child: { t: 'text', runs: [{ text: 'a' }] } },
        ]),
        [],
      );

      expect(rules(found)).toEqual(['unopenable-link']);
    });

    it('accepts the schemes a reader can follow', () => {
      const found = check(
        document([
          { t: 'link', href: 'https://imprenta.dev', child: { t: 'text', runs: [{ text: 'a' }] } },
          {
            t: 'link',
            href: 'mailto:hola@imprenta.dev',
            child: { t: 'text', runs: [{ text: 'b' }] },
          },
        ]),
        [],
      );

      expect(found).toEqual([]);
    });
  });

  describe('what the engine itself noticed', () => {
    it('carries the engine diagnostics through as findings', () => {
      // The shape the engine really emits, taken from its own output rather
      // than from what it would be convenient for it to emit.
      const found = check(document(some), [
        'warning[missing-glyph]: the font has no glyph for "日" — pick another',
        'warning[text-clipped]: a cell was cut',
      ]);

      expect(rules(found)).toEqual(['missing-glyph', 'text-clipped']);
      expect(found[0].source).toBe('engine');
      expect(found[0].detail).toContain('no glyph');
    });

    it('takes the engine word for how serious it is', () => {
      const found = check(document(some), ['error[unknown-asset]: no image called "sello"']);

      expect(found[0].status).toBe('error');
      expect(found[0].rule).toBe('unknown-asset');
    });

    it('does not lose one it cannot parse', () => {
      const found = check(document(some), ['something the engine said']);

      expect(found).toHaveLength(1);
      expect(found[0].detail).toContain('something the engine said');
    });
  });

  describe('saying a thing once', () => {
    it('collapses the same fault found in several places', () => {
      // The engine aggregates its own diagnostics for exactly this reason:
      // one pale paragraph is one problem, and three rows saying so is a
      // panel nobody reads.
      const found = check(
        document([
          {
            t: 'text',
            runs: [{ text: 'uno' }, { text: 'dos' }, { text: 'tres' }],
            style: { color: '#e2e8f0' },
          },
        ]),
        [],
      );

      expect(found).toHaveLength(1);
      expect(found[0].occurrences).toBe(3);
    });

    it('keeps faults apart when they are different faults', () => {
      const found = check(
        document([
          { t: 'text', runs: [{ text: 'a' }], style: { color: '#e2e8f0' } },
          { t: 'text', runs: [{ text: 'b' }], style: { color: '#f1f5f9' } },
        ]),
        [],
      );

      expect(found).toHaveLength(2);
    });

    it('counts one when a thing happens once', () => {
      const found = check(document([{ t: 'text', runs: [{ text: 'a' }], style: { size: 3 } }]), []);

      expect(found[0].occurrences).toBe(1);
    });

    it('names one of the places, so it can be found', () => {
      const found = check(
        document([
          {
            t: 'text',
            runs: [{ text: 'el primero' }, { text: 'el segundo' }],
            style: { color: '#e2e8f0' },
          },
        ]),
        [],
      );

      expect(found[0].detail).toContain('el primero');
    });
  });

  describe('a face the project does not have', () => {
    // The engine falls back to what it was given, so a heading meant to be
    // bold simply is not, and nothing else in the chain says a word.
    const faces = [{ weight: 'regular' as const, italic: false }];

    it('objects to bold text with no bold font', () => {
      const found = check(
        document([{ t: 'text', runs: [{ text: 'TOTAL', weight: 'bold' }] }]),
        [],
        { faces },
      );

      expect(rules(found)).toEqual(['missing-face']);
      expect(found[0].detail).toContain('bold');
    });

    it('objects to italic with no italic font', () => {
      const found = check(document([{ t: 'text', runs: [{ text: 'según', italic: true }] }]), [], {
        faces,
      });

      expect(rules(found)).toEqual(['missing-face']);
    });

    it('looks in table cells as well as paragraphs', () => {
      const found = check(
        document([
          { t: 'table', columns: [{}], rows: [{ cells: [{ text: 'a', weight: 'bold' }] }] },
        ]),
        [],
        { faces },
      );

      expect(rules(found)).toEqual(['missing-face']);
    });

    it('says nothing when the face is configured', () => {
      const found = check(
        document([{ t: 'text', runs: [{ text: 'TOTAL', weight: 'bold' }] }]),
        [],
        { faces: [...faces, { weight: 'bold' as const, italic: false }] },
      );

      expect(found).toEqual([]);
    });

    it('says nothing when it was not told what the project has', () => {
      // Silence beats guessing: without the list, every document would be
      // accused of missing every face.
      const found = check(document([{ t: 'text', runs: [{ text: 'TOTAL', weight: 'bold' }] }]), []);

      expect(found).toEqual([]);
    });
  });

  describe('images too small to print', () => {
    // 150 dpi is the floor for a logo that is not to look soft, and screen
    // images are 72. Nothing else in the chain measures this.
    const images = { logo: { width: 240, height: 80 } };

    it('objects to an image stretched past its pixels', () => {
      const found = check(document([{ t: 'image', src: 'logo', width: 400 }]), [], { images });

      expect(rules(found)).toEqual(['low-resolution-image']);
      expect(found[0].detail).toContain('dpi');
    });

    it('accepts one printed small enough to hold up', () => {
      const found = check(document([{ t: 'image', src: 'logo', width: 100 }]), [], { images });

      expect(found).toEqual([]);
    });

    it('calls it an error when it is very bad, a warning when it is close', () => {
      const bad = check(document([{ t: 'image', src: 'logo', width: 400 }]), [], { images });
      const close = check(document([{ t: 'image', src: 'logo', width: 130 }]), [], { images });

      expect(bad[0].status).toBe('error');
      expect(close[0].status).toBe('warning');
    });

    it('says nothing about an image it was told nothing about', () => {
      const found = check(document([{ t: 'image', src: 'sello', width: 400 }]), [], { images });

      expect(found).toEqual([]);
    });
  });

  describe('content wider than the page', () => {
    it('objects to a box wider than what is left between the margins', () => {
      // The engine lays it out anyway and the right edge is simply cut off.
      const found = check(
        document([{ t: 'box', style: { width: 600 } }], {
          ...page,
          margin: { top: 40, right: 40, bottom: 40, left: 40 },
        }),
        [],
      );

      expect(rules(found)).toEqual(['wider-than-the-page']);
      expect(found[0].detail).toContain('515');
    });

    it('accepts one that fits', () => {
      const found = check(document([{ t: 'box', style: { width: 500 } }]), []);

      expect(found).toEqual([]);
    });
  });

  describe('a document with nothing in it', () => {
    it('says so rather than printing a blank sheet', () => {
      // Almost always a component that returned nothing, and a blank PDF is
      // a slow way to find that out.
      const found = check(document([]), []);

      expect(rules(found)).toEqual(['empty-document']);
    });

    it('does not complain when there is something', () => {
      const found = check(document([{ t: 'text', runs: [{ text: 'a' }] }]), []);

      expect(found).toEqual([]);
    });
  });

  it('puts the errors above the warnings', () => {
    // What stops the document being usable comes before what merely spoils
    // it.
    const found = check(
      document([
        {
          t: 'box',
          style: { background: '#f8fafc' },
          children: [{ t: 'text', runs: [{ text: 'a' }], style: { color: '#e2e8f0', size: 3 } }],
        },
      ]),
      [],
    );

    expect(found.map((f) => f.status)).toEqual(['error', 'warning']);
  });

  it('survives a document shaped in a way it does not expect', () => {
    // The IR grows; a check that throws would take the whole preview down.
    expect(() =>
      check({ page, children: [null, 42, { t: 'unknown' }] } as never, []),
    ).not.toThrow();
  });
});
