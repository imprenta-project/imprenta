//! Streaming composition: pages painted and released as the document is fed.
//!
//! # Why this exists
//!
//! Holding a whole document in memory costs about **405 KB per page**,
//! measured on a ledger. At fifty thousand pages that is twenty gigabytes,
//! and the engine cannot produce the documents it was built for.
//!
//! Almost none of that is the PDF writer. The same measurement puts krilla's
//! own retention at **15.5 KB per page** — it keeps every page until
//! `finish()` because an annotation may point at a page not yet written, and
//! that floor cannot be lowered from outside. The other **96 % is ours**:
//! atoms, shaped lines and glyph runs kept alive long after they were painted.
//!
//! So the fix is not to stream the writer. It is to stop holding our own
//! side: feed content in, pack what is settled, paint it, drop it.
//!
//! # Why releasing a page is safe
//!
//! Every rule the packer applies looks **forward**. `keep_with_next` binds an
//! atom to what follows; a forced break pushes the next run down; a parity
//! break inserts blank pages after the current one. Nothing reaches back.
//!
//! So once a page is not the last one in hand, no atom still to arrive can
//! change what landed on it. The last page always stays, because an atom yet
//! to come could be pinned to the one at its foot and drag it forward.

use crate::atom::Atom;
use crate::content::Content;
use crate::pack::{Contribution, Flow, Group, Page, Repeat, pack};
use crate::render::{Fonts, Geometry, Options, PageSink, RenderError};
use imprenta_core::units::Pt;
use std::collections::HashMap;

/// What a finished page opened and closed at.
///
/// Kept after the page's content is gone, because a document that wants
/// "carried forward" printed in a footer needs the figure long after the
/// atoms that produced it have been released.
#[derive(Debug, Clone, PartialEq)]
pub struct PageTotals {
    pub opening: Vec<f64>,
    pub closing: Vec<f64>,
}

/// What a page can be told about itself while its bands are built.
///
/// A header cannot be shaped once and reused: "3 of 12" and "4 of 12" are
/// different glyphs, and so is a carried-forward total. So the bands are
/// built per page, and this is what they are built from.
#[derive(Debug, Clone, PartialEq)]
pub struct PageContext {
    /// One-based, as a reader counts.
    pub number: usize,
    /// How many pages there are in all — `None` while pages are still being
    /// released, because at that point nobody knows and a guess would print.
    pub total: Option<usize>,
    /// The running totals as this page opened and as it closed.
    pub opening: Vec<f64>,
    pub closing: Vec<f64>,
}

/// What a page's bands come out as.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Painted {
    pub header: Option<Content>,
    pub footer: Option<Content>,
}

/// A finished document and what each of its pages totalled.
#[derive(Debug, Clone, PartialEq)]
pub struct Composed {
    pub pdf: Vec<u8>,
    pub totals: Vec<PageTotals>,
}

/// Composes a document page by page, releasing each as it is painted.
pub struct Composer {
    geometry: Geometry,
    /// Owned rather than borrowed: a composer outlives any one call once a
    /// document is fed to it in pieces, and a borrow would tie it to whatever
    /// stack frame happened to build the fonts.
    fonts: Fonts,
    sink: PageSink,

    /// Atoms not yet painted — only ever the tail of the document.
    atoms: Vec<Atom>,
    contents: Vec<Content>,
    /// How many atoms have been painted and dropped. `atoms[0]` is atom
    /// number `released` in absolute terms.
    released: usize,

    groups: Vec<Group>,
    contributions: Vec<Contribution>,
    accumulators: usize,
    /// The running totals as they stood when the tail began.
    carried: Vec<f64>,

    /// Repeated prefixes, kept alive past the release of the atoms that
    /// named them: a table header is painted on page 400 from an atom
    /// dropped after page 1.
    prefixes: HashMap<usize, Content>,

    totals: Vec<PageTotals>,

    /// Whether nothing may be painted until the last page is packed.
    hold: bool,
    /// Known only once nothing more can arrive.
    total: Option<usize>,
}

impl Composer {
    pub fn new(geometry: Geometry, fonts: Fonts) -> Result<Self, RenderError> {
        Self::with_options(geometry, fonts, Options::default())
    }

    pub fn with_options(
        geometry: Geometry,
        fonts: Fonts,
        options: Options,
    ) -> Result<Self, RenderError> {
        Ok(Self {
            geometry,
            fonts,
            sink: PageSink::new(geometry, options)?,
            atoms: Vec::new(),
            contents: Vec::new(),
            released: 0,
            groups: Vec::new(),
            contributions: Vec::new(),
            accumulators: 0,
            carried: Vec::new(),
            prefixes: HashMap::new(),
            totals: Vec::new(),
            hold: false,
            total: None,
        })
    }

    /// Declares how many running totals the document keeps.
    pub fn with_accumulators(mut self, count: usize) -> Self {
        self.accumulators = count;
        self.carried = vec![0.0; count];
        self
    }

    /// The height a page has for content.
    pub fn budget(&self) -> Pt {
        self.geometry.content_height()
    }

    /// Adds one atom and what it draws, returning its absolute index.
    pub fn push(&mut self, atom: Atom, content: Content) -> usize {
        let index = self.released + self.atoms.len();
        self.atoms.push(atom);
        self.contents.push(content);
        index
    }

    /// Adds `value` to a running total when `atom` is placed.
    pub fn contribute(&mut self, atom: usize, accumulator: usize, value: f64) {
        self.contributions.push(Contribution {
            atom,
            accumulator,
            value,
        });
    }

    /// Registers a group that repeats `prefix` at the top of every page it
    /// continues onto.
    ///
    /// The prefix content is cloned, because the atom naming it is released
    /// long before the group stops continuing — a table header is painted on
    /// page 400 from an atom dropped after page 1.
    pub fn repeat(&mut self, atoms: std::ops::Range<usize>, prefix: usize, height: Pt) {
        if let Some(content) = prefix
            .checked_sub(self.released)
            .and_then(|i| self.contents.get(i))
        {
            self.prefixes.insert(prefix, content.clone());
        }
        self.groups.push(Group {
            atoms,
            repeat_prefix: Some(Repeat {
                atom: prefix,
                height,
            }),
        });
    }

    /// Begins a group whose extent is not known yet.
    ///
    /// [`repeat`](Self::repeat) needs the range of atoms up front, which for
    /// a table means waiting for the last row — and nothing may be released
    /// while a group is still unnamed, so a ledger of forty thousand rows
    /// would be held whole. Opening the group first costs nothing and lets
    /// the pages behind it be painted and dropped as they close.
    ///
    /// The group runs to [`close_repeat`](Self::close_repeat), or to the end
    /// of the document if it is never closed.
    pub fn open_repeat(&mut self, prefix: usize, height: Pt) {
        self.repeat(prefix..usize::MAX, prefix, height);
    }

    /// Ends a group opened by [`open_repeat`](Self::open_repeat) after the
    /// atoms pushed so far.
    ///
    /// Identified by its prefix, which is an absolute atom index and so stays
    /// valid however many pages have been released in between.
    pub fn close_repeat(&mut self, prefix: usize) {
        let end = self.released + self.atoms.len();
        if let Some(group) = self.groups.iter_mut().find(|g| {
            g.atoms.end == usize::MAX && g.repeat_prefix.is_some_and(|r| r.atom == prefix)
        }) {
            group.atoms.end = end;
        }
    }

    /// Pages finished so far.
    pub fn pages(&self) -> usize {
        self.sink.pages()
    }

    /// What each finished page opened and closed at.
    pub fn page_totals(&self) -> &[PageTotals] {
        &self.totals
    }

    /// How many atoms are still held.
    pub fn pending(&self) -> usize {
        self.atoms.len()
    }

    /// Holds every page until the document is finished.
    ///
    /// The price of a footer that says "of 12": nothing can know the total
    /// until the last page is packed, so nothing can be painted until then.
    /// A document that only numbers its pages pays none of this.
    pub fn holding_pages(mut self) -> Self {
        self.hold = true;
        self
    }

    /// Paints and releases every page that can no longer change, with no
    /// bands on any of them.
    ///
    /// For a document that has none. Anything with a header or a footer has
    /// to go through [`flush_with`](Self::flush_with), or the pages released
    /// early would come out bare while the last one did not.
    pub fn flush(&mut self) {
        self.flush_with(&mut |_| Painted::default());
    }

    /// As [`flush`](Self::flush), building each page's bands as it goes.
    pub fn flush_with(&mut self, bands: &mut dyn FnMut(&PageContext) -> Painted) {
        if self.hold {
            // Nothing can be painted until the total is known, and the total
            // is not known until nothing more can arrive.
            return;
        }
        let packed = self.pack_pending();
        // The last page stays: an atom yet to arrive could still pin the run
        // at its foot and pull it forward.
        if packed.len() > 1 {
            self.release(&packed[..packed.len() - 1], bands);
        }
    }

    /// Paints what remains and returns the finished document.
    ///
    /// The totals come back with the bytes because `finish` is what paints
    /// the final page: reading them beforehand would silently miss it.
    pub fn finish(self) -> Result<Composed, RenderError> {
        let mut none = |_: &PageContext| Painted::default();
        self.finish_with(&mut none)
    }

    /// As [`finish`](Self::finish), building each page's bands.
    pub fn finish_with(
        mut self,
        bands: &mut dyn FnMut(&PageContext) -> Painted,
    ) -> Result<Composed, RenderError> {
        let packed = self.pack_pending();
        // Only now can the total be known, which is the whole reason a
        // document that prints one holds its pages.
        self.total = Some(self.totals.len() + packed.len());
        self.release(&packed, bands);
        Ok(Composed {
            pdf: self.sink.finish()?,
            totals: self.totals,
        })
    }

    /// Paints `pages` and drops the atoms they consumed.
    fn release(&mut self, pages: &[Page], bands: &mut dyn FnMut(&PageContext) -> Painted) {
        let base = self.released;
        let mut highest = None;

        for page in pages {
            let contents = &self.contents;
            let prefixes = &self.prefixes;
            let painted = bands(&PageContext {
                number: self.totals.len() + 1,
                total: self.total,
                opening: page.opening.clone(),
                closing: page.closing.clone(),
            });
            self.sink.paint_page_with(
                page,
                &self.fonts,
                |atom| {
                    atom.checked_sub(base)
                        .and_then(|i| contents.get(i))
                        .or_else(|| prefixes.get(&atom))
                },
                &painted,
            );
            self.totals.push(PageTotals {
                opening: page.opening.clone(),
                closing: page.closing.clone(),
            });
            if let Some(last) = page.placements.last() {
                highest = Some(highest.map_or(last.atom, |h: usize| h.max(last.atom)));
            }
            if !page.closing.is_empty() {
                self.carried.clone_from(&page.closing);
            }
        }

        // Placements are absolute, so the count released is measured from
        // the base rather than from zero.
        let Some(highest) = highest else { return };
        let consumed = highest + 1 - base;

        self.atoms.drain(..consumed);
        self.contents.drain(..consumed);
        self.released += consumed;

        // A prefix whose group is finished can go; one still continuing has
        // already been cloned into `prefixes` and outlives its atom.
        self.groups.retain(|g| g.atoms.end > self.released);
        let live: std::collections::HashSet<usize> = self
            .groups
            .iter()
            .filter_map(|g| g.repeat_prefix.map(|r| r.atom))
            .collect();
        self.prefixes.retain(|atom, _| live.contains(atom));
        self.contributions.retain(|c| c.atom >= self.released);
    }

    /// Packs the tail as it stands, in tail-relative indices.
    fn pack_pending(&self) -> Vec<Page> {
        let base = self.released;
        let live = self.groups.iter().filter(|g| g.atoms.end > base);
        let groups: Vec<Group> = live
            .clone()
            .map(|g| Group {
                atoms: g.atoms.start.saturating_sub(base)..(g.atoms.end - base),
                // The prefix atom stays absolute. The packer treats it as an
                // opaque label — it needs the height, not the content — and
                // the painter resolves it against `prefixes`, where a header
                // released four hundred pages ago still lives. Rebasing it
                // would clamp to zero and pick up whatever row happens to
                // start the tail.
                repeat_prefix: g.repeat_prefix,
            })
            .collect();
        // A group that began before the tail has already placed atoms, so the
        // first page of the tail is a page it is being continued onto. The
        // rebased range cannot say that on its own: a group starting at zero
        // and one clamped to zero look identical.
        let started: Vec<bool> = live.map(|g| g.atoms.start < base).collect();
        let contributions: Vec<Contribution> = self
            .contributions
            .iter()
            .filter(|c| c.atom >= base)
            .map(|c| Contribution {
                atom: c.atom - base,
                ..*c
            })
            .collect();

        let mut pages = pack(
            &Flow::new(&self.atoms)
                .with_groups(&groups)
                .with_accumulators(self.accumulators, &contributions)
                .continuing_from(&self.carried)
                .resuming(&started),
            self.geometry.content_height(),
        );

        // Placements come back tail-relative; the sink is told the base and
        // resolves them itself, so shift them into absolute terms here.
        // Continuations are left alone: their atom was never rebased.
        for page in &mut pages {
            for placement in &mut page.placements {
                placement.atom += base;
            }
        }
        pages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::{TextStyle, measure_text};
    use crate::render::render_faces;
    use crate::shape::Shaper;

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn fonts_and_shaper() -> (Fonts, Shaper) {
        let shaper = Shaper::new(ROBOTO.to_vec());
        (Fonts::from_shaper(&shaper).unwrap(), shaper)
    }

    fn geometry() -> Geometry {
        Geometry::a4()
    }

    /// `count` single-line rows of measured text.
    fn rows(shaper: &mut Shaper, count: usize) -> Vec<(Atom, Content)> {
        (0..count)
            .map(|i| {
                let m = measure_text(
                    shaper,
                    &format!("Asiento {i} — prestación de servicios profesionales"),
                    TextStyle::new(Pt(9.0)),
                    Pt(400.0),
                );
                let line = m.lines.into_iter().next().unwrap();
                (Atom::new(line.height), Content::Text(line))
            })
            .collect()
    }

    /// The same document built in one go, for comparison.
    fn all_at_once(fonts: &Fonts, rows: &[(Atom, Content)]) -> Vec<u8> {
        let atoms: Vec<Atom> = rows.iter().map(|(a, _)| a.clone()).collect();
        let contents: Vec<Content> = rows.iter().map(|(_, c)| c.clone()).collect();
        let pages = crate::pack::pack(&crate::pack::Flow::new(&atoms), geometry().content_height());
        render_faces(&pages, &contents, fonts, geometry(), Options::default()).unwrap()
    }

    #[test]
    fn a_composed_document_is_byte_identical_to_one_built_all_at_once() {
        // The whole claim: releasing pages as they are finished changes what
        // is held, never what is produced.
        let (fonts, mut shaper) = fonts_and_shaper();
        let rows = rows(&mut shaper, 300);

        let mut composer = Composer::new(geometry(), fonts.clone()).unwrap();
        for (atom, content) in &rows {
            composer.push(atom.clone(), content.clone());
            composer.flush();
        }
        let streamed = composer.finish().unwrap().pdf;

        assert_eq!(streamed, all_at_once(&fonts, &rows));
    }

    #[test]
    fn flushing_at_every_atom_gives_the_same_answer_as_flushing_never() {
        let (fonts, mut shaper) = fonts_and_shaper();
        let rows = rows(&mut shaper, 200);

        let eager = {
            let mut c = Composer::new(geometry(), fonts.clone()).unwrap();
            for (a, content) in &rows {
                c.push(a.clone(), content.clone());
                c.flush();
            }
            c.finish().unwrap().pdf
        };
        let lazy = {
            let mut c = Composer::new(geometry(), fonts.clone()).unwrap();
            for (a, content) in &rows {
                c.push(a.clone(), content.clone());
            }
            c.finish().unwrap().pdf
        };

        assert_eq!(eager, lazy);
    }

    #[test]
    fn what_is_still_held_does_not_grow_with_the_document() {
        // The point of the exercise. Without releasing, `pending` would climb
        // to the atom count; with it, it stays within a page.
        let (fonts, mut shaper) = fonts_and_shaper();
        let rows = rows(&mut shaper, 2000);

        let mut composer = Composer::new(geometry(), fonts.clone()).unwrap();
        let mut peak = 0usize;
        for (atom, content) in rows {
            composer.push(atom, content);
            composer.flush();
            peak = peak.max(composer.pending());
        }

        assert!(
            peak < 120,
            "held {peak} atoms at once — that is not bounded by a page"
        );
        composer.finish().unwrap();
    }

    #[test]
    fn the_last_page_is_never_released_early() {
        // An atom yet to arrive could be pinned to the run at its foot and
        // drag it forward, so the page in hand must stay in hand.
        let (fonts, mut shaper) = fonts_and_shaper();
        let rows = rows(&mut shaper, 200);

        let mut composer = Composer::new(geometry(), fonts.clone()).unwrap();
        for (atom, content) in rows {
            composer.push(atom, content);
            composer.flush();
            assert!(composer.pending() > 0, "the tail was released too eagerly");
        }
        composer.finish().unwrap();
    }

    #[test]
    fn running_totals_carry_across_a_release() {
        // Each segment is packed on its own; without carrying, the totals
        // would restart at zero and every page after the first would be wrong.
        let (fonts, mut shaper) = fonts_and_shaper();
        let rows = rows(&mut shaper, 300);

        let mut composer = Composer::new(geometry(), fonts.clone())
            .unwrap()
            .with_accumulators(1);
        for (atom, content) in rows {
            let index = composer.push(atom, content);
            composer.contribute(index, 0, 10.0);
            composer.flush();
        }
        let totals = composer.finish().unwrap().totals;

        assert!(totals.len() > 2, "the sample must span pages");
        for pair in totals.windows(2) {
            assert_eq!(
                pair[1].opening, pair[0].closing,
                "a page did not open where the one before it closed"
            );
        }
        assert_eq!(totals[0].opening, vec![0.0]);
        assert_eq!(totals.last().unwrap().closing, vec![3000.0]);
    }

    /// A one-line header and the height it occupies.
    fn header(shaper: &mut Shaper) -> (crate::shape::Line, Pt) {
        let line = measure_text(shaper, "CABECERA", TextStyle::new(Pt(9.0)), Pt(400.0))
            .lines
            .remove(0);
        let height = line.height;
        (line, height)
    }

    #[test]
    fn a_repeated_header_survives_the_release_of_its_own_atom() {
        // The header atom is dropped after the first page; it must still be
        // painted at the top of page four hundred.
        let (fonts, mut shaper) = fonts_and_shaper();
        let header = measure_text(&mut shaper, "CABECERA", TextStyle::new(Pt(9.0)), Pt(400.0))
            .lines
            .remove(0);
        let header_height = header.height;
        let rows = rows(&mut shaper, 400);

        let mut composer = Composer::new(geometry(), fonts.clone()).unwrap();
        let first = composer.push(Atom::new(header_height), Content::Text(header));
        for (atom, content) in rows {
            composer.push(atom, content);
        }
        composer.repeat(first..first + 401, first, header_height);

        let mut composer = composer;
        composer.flush();
        let pdf = composer.finish().unwrap().pdf;

        // One text run per row plus one per repeated header. Cheap proxy: the
        // document must be larger than the same rows with no repeat.
        assert!(pdf.len() > 1000);
    }

    #[test]
    fn a_group_can_be_declared_before_the_rows_it_will_contain() {
        // Declaring the range afterwards means nothing may be released until
        // the last row has arrived — which for a ledger is the whole
        // document. Opening the group first is what lets a long table stream,
        // and it has to come out identical to the closed form.
        let (fonts, mut shaper) = fonts_and_shaper();

        let closed = {
            let mut c = Composer::new(geometry(), fonts.clone()).unwrap();
            let (h, height) = header(&mut shaper);
            let first = c.push(Atom::new(height), Content::Text(h));
            for (atom, content) in rows(&mut shaper, 400) {
                c.push(atom, content);
            }
            c.repeat(first..first + 401, first, height);
            c.finish().unwrap().pdf
        };

        let streamed = {
            let mut c = Composer::new(geometry(), fonts.clone()).unwrap();
            let (h, height) = header(&mut shaper);
            let first = c.push(Atom::new(height), Content::Text(h));
            c.open_repeat(first, height);
            for (atom, content) in rows(&mut shaper, 400) {
                c.push(atom, content);
                // The flush the closed form cannot do.
                c.flush();
            }
            c.close_repeat(first);
            c.finish().unwrap().pdf
        };

        assert_eq!(
            shape_of(&streamed),
            shape_of(&closed),
            "streaming changed the document"
        );
        assert_eq!(streamed, closed);
    }

    /// Page count and size, for a failure message a human can read.
    fn shape_of(pdf: &[u8]) -> (usize, usize) {
        let text = String::from_utf8_lossy(pdf);
        let count = text
            .split("/Count ")
            .nth(1)
            .and_then(|t| t.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        (count, pdf.len())
    }

    #[test]
    fn an_open_group_does_not_pin_the_rows_that_have_already_passed() {
        let (fonts, mut shaper) = fonts_and_shaper();
        let mut c = Composer::new(geometry(), fonts.clone()).unwrap();
        let (h, height) = header(&mut shaper);
        let first = c.push(Atom::new(height), Content::Text(h));
        c.open_repeat(first, height);

        for (atom, content) in rows(&mut shaper, 400) {
            c.push(atom, content);
            c.flush();
        }

        assert!(
            c.pending() < 100,
            "an open group held {} of 400 rows",
            c.pending()
        );
    }

    #[test]
    fn an_empty_document_is_rejected_rather_than_producing_a_broken_file() {
        let (fonts, _) = fonts_and_shaper();
        let composer = Composer::new(geometry(), fonts.clone()).unwrap();

        assert!(composer.finish().is_err());
    }

    #[test]
    fn flushing_an_empty_composer_does_nothing() {
        let (fonts, _) = fonts_and_shaper();
        let mut composer = Composer::new(geometry(), fonts.clone()).unwrap();

        composer.flush();

        assert_eq!(composer.pages(), 0);
        assert_eq!(composer.pending(), 0);
    }

    #[test]
    fn measuring_in_parallel_and_streaming_still_gives_the_same_document() {
        // The three routes a caller can take must agree byte for byte, or a
        // document would depend on how it happened to be produced.
        use crate::parallel::{Block, Faces, measure_all_in};
        use crate::shape::Face;

        let (fonts, mut shaper) = fonts_and_shaper();
        let texts: Vec<String> = (0..400)
            .map(|i| format!("Asiento {i} — prestación de servicios profesionales"))
            .collect();

        // Route one: measured one at a time, composed all at once.
        let rows: Vec<(Atom, Content)> = texts
            .iter()
            .map(|t| {
                let line = measure_text(&mut shaper, t, TextStyle::new(Pt(9.0)), Pt(400.0))
                    .lines
                    .remove(0);
                (Atom::new(line.height), Content::Text(line))
            })
            .collect();
        let batch = all_at_once(&fonts, &rows);

        // Route two: the same, streamed.
        let streamed = {
            let mut c = Composer::new(geometry(), fonts.clone()).unwrap();
            for (a, content) in &rows {
                c.push(a.clone(), content.clone());
                c.flush();
            }
            c.finish().unwrap().pdf
        };

        // Route three: measured across every core, then streamed.
        let faces: Faces = vec![(Face::REGULAR, ROBOTO.to_vec())];
        let blocks: Vec<Block<'_>> = texts
            .iter()
            .map(|t| Block::new(t, TextStyle::new(Pt(9.0)), Pt(400.0)))
            .collect();
        let parallel = {
            let mut c = Composer::new(geometry(), fonts.clone()).unwrap();
            for measured in measure_all_in(&faces, &blocks) {
                for (atom, line) in measured.atoms.into_iter().zip(measured.lines) {
                    c.push(atom, Content::Text(line));
                }
            }
            c.flush();
            c.finish().unwrap().pdf
        };

        assert_eq!(streamed, batch, "streaming changed the document");
        assert_eq!(parallel, batch, "parallel measuring changed the document");
    }
}

#[cfg(test)]
mod bands {
    use super::*;
    use crate::content::{BoxContent, Content};
    use crate::measure::{TextStyle, measure_text};
    use crate::render::Bands;
    use crate::shape::Shaper;
    use imprenta_core::units::Edges;

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn parts() -> (Fonts, Shaper) {
        let shaper = Shaper::new(ROBOTO.to_vec());
        (Fonts::from_shaper(&shaper).unwrap(), shaper)
    }

    fn geometry() -> Geometry {
        Geometry {
            width: Pt(200.0),
            height: Pt(200.0),
            margin: Edges::all(Pt(20.0)),
            bands: Bands::default(),
        }
    }

    /// One line of text as paintable content.
    fn line(shaper: &mut Shaper, text: &str) -> Content {
        let measured = measure_text(shaper, text, TextStyle::new(Pt(8.0)), Pt(300.0));
        Content::Box(BoxContent::default().stack(Content::Text(measured.lines[0].clone())))
    }

    fn rows(shaper: &mut Shaper, count: usize) -> Vec<(Atom, Content)> {
        (0..count)
            .map(|i| {
                let measured = measure_text(
                    shaper,
                    &format!("fila {i}"),
                    TextStyle::new(Pt(9.0)),
                    Pt(160.0),
                );
                (
                    Atom::new(measured.lines[0].height),
                    Content::Text(measured.lines[0].clone()),
                )
            })
            .collect()
    }

    #[test]
    fn a_band_takes_its_room_out_of_the_page() {
        // Declaring a header means less room for content, and the arithmetic
        // has to be the paginator's rather than the author's: a footer that
        // overlapped the last line would be found by whoever printed it.
        let plain = geometry();
        let banded = Geometry {
            bands: Bands {
                header: Pt(30.0),
                footer: Pt(20.0),
            },
            ..geometry()
        };

        assert_eq!(plain.content_height(), Pt(160.0));
        assert_eq!(banded.content_height(), Pt(110.0));
    }

    #[test]
    fn a_footer_is_painted_on_every_page() {
        let (fonts, mut shaper) = parts();
        let geometry = Geometry {
            bands: Bands {
                header: Pt(0.0),
                footer: Pt(14.0),
            },
            ..geometry()
        };
        let mut composer = Composer::new(geometry, fonts).unwrap();
        for (atom, content) in rows(&mut shaper, 60) {
            composer.push(atom, content);
        }

        let mut seen = Vec::new();
        let pdf = composer
            .finish_with(&mut |page: &PageContext| {
                seen.push(page.number);
                Painted {
                    header: None,
                    footer: Some(line(&mut shaper, &format!("pagina {}", page.number))),
                }
            })
            .unwrap();

        assert!(pdf.totals.len() > 3, "the sample must paginate");
        assert_eq!(seen, (1..=pdf.totals.len()).collect::<Vec<_>>());
    }

    #[test]
    fn a_page_knows_its_number_and_what_the_totals_stood_at() {
        // The whole reason a band is built per page rather than once: "suma y
        // sigue" is a different number on every sheet.
        let (fonts, mut shaper) = parts();
        let mut composer = Composer::new(geometry(), fonts)
            .unwrap()
            .with_accumulators(1);
        for (index, (atom, content)) in rows(&mut shaper, 40).into_iter().enumerate() {
            let at = composer.push(atom, content);
            composer.contribute(at, 0, (index + 1) as f64);
        }

        let mut carried = Vec::new();
        composer
            .finish_with(&mut |page: &PageContext| {
                carried.push((page.number, page.opening[0], page.closing[0]));
                Painted::default()
            })
            .unwrap();

        assert!(carried.len() > 1, "the sample must paginate");
        assert_eq!(carried[0].1, 0.0, "the first page opens at nothing");
        for pair in carried.windows(2) {
            assert_eq!(pair[0].2, pair[1].1, "one page closes where the next opens");
        }
    }

    #[test]
    fn a_page_is_told_the_total_when_the_total_is_known() {
        // It cannot be known while pages are being released, so a document
        // that wants "3 of 12" is one the composer must hold whole.
        let (fonts, mut shaper) = parts();
        let mut composer = Composer::new(geometry(), fonts).unwrap().holding_pages();
        for (atom, content) in rows(&mut shaper, 40) {
            composer.push(atom, content);
        }
        composer.flush();

        let mut totals = Vec::new();
        let out = composer
            .finish_with(&mut |page: &PageContext| {
                totals.push(page.total);
                Painted::default()
            })
            .unwrap();

        assert!(out.totals.len() > 1);
        assert!(
            totals.iter().all(|t| *t == Some(out.totals.len())),
            "every page should have been told the same total: {totals:?}"
        );
    }

    #[test]
    fn a_streamed_document_admits_it_does_not_know_the_total() {
        // Rather than guessing, or printing a number that turns out wrong.
        let (fonts, mut shaper) = parts();
        let mut composer = Composer::new(geometry(), fonts).unwrap();
        let mut totals = Vec::new();
        let mut watch = |page: &PageContext| {
            totals.push(page.total);
            Painted::default()
        };

        for (atom, content) in rows(&mut shaper, 40) {
            composer.push(atom, content);
            composer.flush_with(&mut watch);
        }
        composer.finish_with(&mut watch).unwrap();

        assert!(totals.len() > 1, "the sample must paginate");
        assert!(
            totals.iter().any(Option::is_none),
            "a page released before the end cannot have been told a total: {totals:?}"
        );
    }

    #[test]
    fn holding_pages_changes_nothing_but_when_they_are_painted() {
        let (fonts, mut shaper) = parts();
        let build = |held: bool, shaper: &mut Shaper| {
            let mut composer = Composer::new(geometry(), fonts.clone()).unwrap();
            if held {
                composer = composer.holding_pages();
            }
            for (atom, content) in rows(shaper, 40) {
                composer.push(atom, content);
                composer.flush();
            }
            composer
                .finish_with(&mut |_| Painted::default())
                .unwrap()
                .pdf
        };

        assert_eq!(build(true, &mut shaper), build(false, &mut shaper));
    }

    #[test]
    fn a_band_that_paints_nothing_costs_nothing_but_its_room() {
        let (fonts, mut shaper) = parts();
        let mut composer = Composer::new(geometry(), fonts).unwrap();
        for (atom, content) in rows(&mut shaper, 20) {
            composer.push(atom, content);
        }

        let out = composer.finish_with(&mut |_| Painted::default()).unwrap();

        assert!(out.pdf.starts_with(b"%PDF-"));
    }
}
