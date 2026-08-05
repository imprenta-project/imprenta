//! Feeding a document in pieces instead of declaring it whole.
//!
//! `build` takes a document that already exists in memory. For a ledger of
//! forty thousand rows that declaration is the largest thing in the process —
//! larger than the pages, larger than the PDF — and it exists only to be read
//! once, in order, and thrown away.
//!
//! A session reads it in order without it ever existing. The producer sends
//! [`Chunk`]s; the engine measures, packs, paints and releases as they arrive,
//! and nothing is held but the tail. What comes out is byte for byte what
//! `build` would have produced from the whole thing.

use crate::build::{Assets, BuildError, Built, OpenTable, Walk};
use crate::compose::Composer;
use crate::ir;
use crate::render::{Fonts, Geometry, Options};
use crate::shape::Shaper;
use imprenta_core::diagnostic::Diagnostics;
use imprenta_core::units::Pt;

/// One piece of a document, in the order it is read.
///
/// Nodes come in batches rather than singly, because a document with no table
/// in it is a real document — a transcript, a log, a book — and forty
/// thousand paragraphs one at a time is forty thousand round trips. A table
/// is opened, fed and closed instead of being sent whole, since it is the one
/// node whose contents can be too big to hold.
#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    Nodes(Vec<ir::Node>),
    OpenTable(ir::TableHead),
    Rows(Vec<ir::Row>),
    CloseTable,
}

#[derive(Debug, thiserror::Error)]
pub enum OutOfOrder {
    #[error("rows arrived with no table open")]
    RowsWithNoTable,
    #[error("a table was closed that was never opened")]
    CloseWithNoTable,
}

/// A document being read, one piece at a time.
///
/// Owns everything the walk borrows, so it can live between calls — across a
/// network read, or across the boundary into another language.
/// The bands a document repeats on every page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bands {
    pub header: Option<ir::Band>,
    pub footer: Option<ir::Band>,
}

impl Bands {
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether any band asks how many pages there are, which is the one
    /// question that cannot be answered while pages are being released.
    pub fn needs_total(&self) -> bool {
        crate::build::bands_need_total(self.header.as_ref(), self.footer.as_ref())
    }
}

pub struct Session {
    shaper: Shaper,
    assets: Assets,
    diagnostics: Diagnostics,
    composer: Composer,
    width: Pt,
    open: Option<OpenTable>,
    bands: Bands,
    names: Vec<String>,
}

impl Session {
    pub fn open(
        page: ir::PageSetup,
        accumulators: usize,
        assets: Assets,
        options: Options,
    ) -> Result<Self, BuildError> {
        Self::open_with(page, Bands::none(), accumulators, assets, options)
    }

    /// As [`open`](Self::open), with a header and a footer on every page.
    ///
    /// Declared here rather than fed as chunks because they belong to the
    /// document and not to any page: the paginator has to know how much room
    /// they take before it packs the first row.
    pub fn open_with(
        page: ir::PageSetup,
        bands: Bands,
        accumulators: usize,
        assets: Assets,
        options: Options,
    ) -> Result<Self, BuildError> {
        if assets.fonts.is_empty() {
            return Err(BuildError::NoFonts);
        }
        let shaper = Shaper::with_faces(assets.fonts.iter().cloned());
        let fonts = Fonts::from_shaper(&shaper)?;
        let geometry = Geometry {
            width: page.width,
            height: page.height,
            margin: page.margin,
            bands: crate::render::Bands {
                header: bands.header.as_ref().map_or(Pt(0.0), |b| b.height),
                footer: bands.footer.as_ref().map_or(Pt(0.0), |b| b.height),
            },
        };
        let mut composer =
            Composer::with_options(geometry, fonts, options)?.with_accumulators(accumulators);
        if bands.needs_total() {
            composer = composer.holding_pages();
        }
        Ok(Self {
            width: geometry.width - geometry.margin.horizontal(),
            composer,
            shaper,
            assets,
            diagnostics: Diagnostics::default(),
            open: None,
            bands,
            names: Vec::new(),
        })
    }

    /// Composes a piece of a document some other session began.
    ///
    /// See [`crate::compose::Composer::resuming`]: the fragment is told which
    /// page it starts on, how many there are in all, and what the running
    /// totals stood at when the piece before it ended. It produces the pages
    /// the whole document would have produced **only if it starts where a page
    /// started**, which is what [`crate::build::plan`] is for.
    pub fn resuming(mut self, page: usize, total: usize, opening: Vec<f64>) -> Self {
        self.composer = self.composer.resuming(page, total, opening);
        self
    }

    /// Names the running totals, so a band can ask for one by name.
    pub fn with_accumulator_names(mut self, names: Vec<String>) -> Self {
        self.names = names;
        self
    }

    /// Adds rows that have already been measured.
    ///
    /// The other half of [`crate::build::measure_rows`], and the reason a
    /// sharded render measures a row once rather than twice: the engine that
    /// measured it to plan is the engine that paints it, and what it kept is
    /// exactly what painting needs.
    ///
    /// The rows must have been measured against the same table and the same
    /// width, which is the caller's to get right — measuring them for one
    /// layout and painting them into another is not something this can see.
    pub fn feed_measured(&mut self, rows: &[crate::build::MeasuredRow]) -> Result<(), BuildError> {
        for row in rows {
            let atom = row.atom();
            let content = crate::content::Content::Box(row.clone().into_content());
            self.composer.push(atom, content);
            if self.composer.pending() >= 256 {
                self.composer.flush();
            }
        }
        Ok(())
    }

    /// Reads one piece.
    ///
    /// Whatever the piece is, it goes through the same walk a whole declared
    /// document does. There is no second layout path here to disagree with
    /// the first one.
    pub fn feed(&mut self, chunk: &Chunk) -> Result<(), BuildError> {
        let width = self.width;
        let mut walk = Walk {
            shaper: &mut self.shaper,
            assets: &self.assets,
            diagnostics: &mut self.diagnostics,
            composer: &mut self.composer,
            pending_break: None,
        };

        match chunk {
            Chunk::Nodes(nodes) => {
                for node in nodes {
                    walk.node(node, width)?;
                }
            }
            Chunk::OpenTable(head) => {
                // A table already open is closed first rather than nested:
                // the IR has no nested tables, and silently discarding the
                // first one would lose its rows.
                if let Some(open) = self.open.take() {
                    walk.close_table(open);
                }
                self.open = Some(walk.open_table(head, width));
            }
            Chunk::Rows(rows) => {
                let open = self
                    .open
                    .as_mut()
                    .ok_or(BuildError::OutOfOrder(OutOfOrder::RowsWithNoTable))?;
                walk.table_rows(open, rows);
            }
            Chunk::CloseTable => {
                let open = self
                    .open
                    .take()
                    .ok_or(BuildError::OutOfOrder(OutOfOrder::CloseWithNoTable))?;
                walk.close_table(open);
            }
        }
        Ok(())
    }

    /// Atoms not yet painted. About one page's worth, whatever has been fed.
    pub fn pending(&self) -> usize {
        self.composer.pending()
    }

    /// Paints the tail and closes the file.
    ///
    /// A table left open is closed here rather than refused: a producer whose
    /// stream ended early — a dropped connection, a cancelled job — should
    /// get the pages that did arrive.
    pub fn finish(mut self) -> Result<Built, BuildError> {
        if let Some(open) = self.open.take() {
            let mut walk = Walk {
                shaper: &mut self.shaper,
                assets: &self.assets,
                diagnostics: &mut self.diagnostics,
                composer: &mut self.composer,
                pending_break: None,
            };
            walk.close_table(open);
        }
        let Session {
            mut shaper,
            assets,
            mut diagnostics,
            composer,
            width,
            bands,
            names,
            ..
        } = self;
        let composed = crate::build::finish_with_bands(
            composer,
            &mut shaper,
            &assets,
            &mut diagnostics,
            &bands,
            &names,
            width,
        )?;
        Ok(Built {
            pages: composed.totals.len(),
            pdf: composed.pdf,
            diagnostics: diagnostics.iter().map(|d| d.to_string()).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::build;
    use crate::render::Options;
    use crate::shape::Face;
    use imprenta_core::units::{Length, Pt};

    const REGULAR: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn assets() -> Assets {
        Assets::new().with_font(Face::REGULAR, REGULAR.to_vec())
    }

    fn head() -> ir::TableHead {
        ir::TableHead {
            columns: vec![
                ir::ColumnSpec {
                    width: Length::Pt(Pt(80.0)),
                    ..Default::default()
                },
                ir::ColumnSpec::default(),
            ],
            header: vec![ir::Row {
                cells: vec![ir::Cell::new("Ref."), ir::Cell::new("Concepto")],
                ..Default::default()
            }],
            repeat_header: true,
            padding: Default::default(),
            space_after: Pt(0.0),
        }
    }

    fn rows(from: usize, to: usize) -> Vec<ir::Row> {
        (from..to)
            .map(|i| ir::Row {
                cells: vec![
                    ir::Cell::new(format!("{i:03}")),
                    ir::Cell::new(format!("Asiento contable numero {i}")),
                ],
                ..Default::default()
            })
            .collect()
    }

    /// The same ledger as one declared document.
    fn whole(count: usize) -> ir::Document {
        let head = head();
        ir::Document {
            page: ir::PageSetup::default(),
            header: None,
            footer: None,
            accumulators: Vec::new(),
            children: vec![ir::Node::Table(ir::Table {
                columns: head.columns.clone(),
                header: head.header.clone(),
                rows: rows(0, count),
                repeat_header: head.repeat_header,
                padding: head.padding,
                space_after: head.space_after,
            })],
        }
    }

    fn streamed(count: usize, batch: usize) -> Built {
        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
        session.feed(&Chunk::OpenTable(head())).unwrap();
        let mut sent = 0;
        while sent < count {
            let end = (sent + batch).min(count);
            session.feed(&Chunk::Rows(rows(sent, end))).unwrap();
            sent = end;
        }
        session.feed(&Chunk::CloseTable).unwrap();
        session.finish().unwrap()
    }

    #[test]
    fn a_document_fed_in_pieces_is_the_document() {
        // The promise the whole idea rests on. If these differ at all, the
        // streaming path is a second engine, and there will be two sets of
        // bugs to find.
        let declared = build(&whole(400), &assets(), Options::default()).unwrap();

        let fed = streamed(400, 50);

        assert_eq!(fed.pages, declared.pages);
        assert_eq!(fed.pdf, declared.pdf);
    }

    #[test]
    fn how_the_pieces_are_cut_makes_no_difference() {
        // A producer batches by whatever its own data does — a database page,
        // a network frame — and none of that should reach the page.
        let one_at_a_time = streamed(300, 1);
        let in_sevens = streamed(300, 7);
        let all_at_once = streamed(300, 300);

        assert_eq!(one_at_a_time.pdf, in_sevens.pdf);
        assert_eq!(in_sevens.pdf, all_at_once.pdf);
    }

    #[test]
    fn the_header_comes_back_on_every_page_however_the_rows_arrived() {
        let declared = build(&whole(400), &assets(), Options::default()).unwrap();

        let fed = streamed(400, 13);

        assert!(declared.pages > 5, "the sample must paginate");
        assert_eq!(fed.pdf, declared.pdf);
    }

    #[test]
    fn nodes_and_tables_can_be_mixed() {
        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
        session
            .feed(&Chunk::Nodes(vec![ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new("Libro mayor")],
                style: ir::TextStyle::default(),
            })]))
            .unwrap();
        session.feed(&Chunk::OpenTable(head())).unwrap();
        session.feed(&Chunk::Rows(rows(0, 40))).unwrap();
        session.feed(&Chunk::CloseTable).unwrap();
        session
            .feed(&Chunk::Nodes(vec![ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new("Fin")],
                style: ir::TextStyle::default(),
            })]))
            .unwrap();

        let built = session.finish().unwrap();

        assert_eq!(built.pages, 1);
        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    #[test]
    fn what_a_ledger_holds_does_not_grow_with_the_ledger() {
        // The point of all of it, and the only form of the claim worth
        // testing: not a number, but that the number does not move. Four
        // thousand rows must cost no more to be part-way through than four
        // hundred did.
        let held = |count: usize| {
            let mut session =
                Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
            session.feed(&Chunk::OpenTable(head())).unwrap();
            for batch in 0..count / 100 {
                session
                    .feed(&Chunk::Rows(rows(batch * 100, batch * 100 + 100)))
                    .unwrap();
            }
            session.pending()
        };

        let short = held(400);
        let long = held(4_000);
        let longer = held(20_000);

        assert!(long <= short + 100, "400 held {short}, 4,000 held {long}");
        assert!(
            longer <= short + 100,
            "400 held {short}, 20,000 held {longer}"
        );
        assert!(
            longer < 400,
            "a page of rows is nothing like {longer} atoms"
        );
    }

    #[test]
    fn rows_before_a_table_is_open_are_refused() {
        // Not dropped. A producer that got its order wrong has lost data, and
        // finding that out from a short PDF is finding out too late.
        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();

        let loose = session.feed(&Chunk::Rows(rows(0, 10)));

        assert!(loose.is_err(), "rows outside a table were accepted");
    }

    #[test]
    fn closing_a_table_that_was_never_opened_is_refused() {
        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();

        assert!(session.feed(&Chunk::CloseTable).is_err());
    }

    #[test]
    fn a_table_left_open_at_the_end_is_still_printed() {
        // A producer whose stream ended early — a connection dropped, a job
        // cancelled — should get the pages that did arrive rather than
        // nothing at all.
        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
        session.feed(&Chunk::OpenTable(head())).unwrap();
        session.feed(&Chunk::Rows(rows(0, 40))).unwrap();

        let built = session.finish().unwrap();

        assert_eq!(built.pages, 1);
    }

    #[test]
    fn what_the_engine_noticed_survives_to_the_end() {
        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
        session.feed(&Chunk::OpenTable(head())).unwrap();
        session
            .feed(&Chunk::Rows(vec![ir::Row {
                cells: vec![ir::Cell::new("001"), ir::Cell::new("日本語")],
                ..Default::default()
            }]))
            .unwrap();
        session.feed(&Chunk::CloseTable).unwrap();

        let built = session.finish().unwrap();

        assert!(
            built
                .diagnostics
                .iter()
                .any(|d| d.contains("missing-glyph")),
            "{:?}",
            built.diagnostics
        );
    }

    /// A document with no table in it at all.
    fn paragraphs(count: usize) -> Vec<ir::Node> {
        (0..count)
            .map(|i| {
                ir::Node::Text(ir::Text {
                    runs: vec![ir::Run::new(format!(
                        "{i}. Intervención registrada en el acta del expediente"
                    ))],
                    style: ir::TextStyle::default(),
                })
            })
            .collect()
    }

    #[test]
    fn a_document_with_no_table_in_it_streams_too() {
        // Plenty of documents are all prose — a transcript, a log, a book —
        // and the table is not what makes streaming worth doing.
        let declared = build(
            &ir::Document {
                page: ir::PageSetup::default(),
                accumulators: Vec::new(),
                children: paragraphs(600),
                header: None,
                footer: None,
            },
            &assets(),
            Options::default(),
        )
        .unwrap();

        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
        for batch in paragraphs(600).chunks(50) {
            session.feed(&Chunk::Nodes(batch.to_vec())).unwrap();
        }
        let fed = session.finish().unwrap();

        assert!(declared.pages > 5, "the sample must paginate");
        assert_eq!(fed.pdf, declared.pdf);
    }

    #[test]
    fn a_batch_of_nodes_is_the_nodes() {
        // Batching is for the round trips, and must mean nothing else.
        let together = {
            let mut s =
                Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
            s.feed(&Chunk::Nodes(paragraphs(120))).unwrap();
            s.finish().unwrap()
        };
        let singly = {
            let mut s =
                Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
            for node in paragraphs(120) {
                s.feed(&Chunk::Nodes(vec![node])).unwrap();
            }
            s.finish().unwrap()
        };

        assert_eq!(together.pdf, singly.pdf);
    }

    #[test]
    fn prose_holds_no_more_than_a_table_does() {
        let mut session =
            Session::open(ir::PageSetup::default(), 0, assets(), Options::default()).unwrap();
        for batch in paragraphs(4_000).chunks(100) {
            session.feed(&Chunk::Nodes(batch.to_vec())).unwrap();
        }

        assert!(
            session.pending() < 400,
            "still holding {} of 4,000 paragraphs",
            session.pending()
        );
    }
}
