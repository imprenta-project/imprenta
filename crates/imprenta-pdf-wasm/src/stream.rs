//! A document fed in pieces.
//!
//! # Why there is no thread in here
//!
//! The Node binding gives its [`Session`] a dedicated OS thread, feeds it down
//! a channel, and refuses a second call while one is in flight. All of that
//! exists for one reason: `Session` is not `Send`, because krilla keeps its
//! fonts behind an `Rc`, so a half-written document cannot move between
//! threads at all.
//!
//! A WebAssembly instance is a single-threaded world by construction, so the
//! session simply lives here between calls. No thread, no channel, no busy
//! flag — and the ordering guarantee the Node side enforces at run time is
//! structural, because one call into a module cannot interleave with another.
//!
//! Keeping the host responsive is still the host's problem, and it solves it
//! one level up: put the whole instance in a worker. One boundary at the top
//! instead of one inside the binding.

use imprenta_pdf::ir;
use imprenta_pdf::render::Options;
use imprenta_pdf::session::{Bands, Chunk, Session};
use serde::Deserialize;

use crate::job::{JobError, Library, Outcome};

/// Everything a document declares before its content arrives.
///
/// The bands are here rather than fed as pieces because they belong to the
/// document and not to any page: the paginator has to know how much room a
/// header takes before it packs the first row.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub page: ir::PageSetup,
    #[serde(default)]
    pub header: Option<ir::Band>,
    #[serde(default)]
    pub footer: Option<ir::Band>,
    /// Names of the running totals, in the order a band refers to them.
    #[serde(default)]
    pub accumulators: Vec<String>,
    /// Where this piece stands in a document some other instance began.
    ///
    /// Absent for a whole document, which starts at page one of however many
    /// there turn out to be.
    #[serde(default)]
    pub resume: Option<Resume>,
}

/// What a fragment cannot work out for itself.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    /// The number this fragment's first page carries.
    pub page: usize,
    /// How many pages the whole document has, so `{{pages}}` is answerable
    /// without holding anything.
    pub total: usize,
    /// The running totals as the piece before this one closed.
    #[serde(default)]
    pub opening: Vec<f64>,
}

/// A document being read, one piece at a time.
pub struct Printer {
    session: Session,
}

impl Printer {
    pub fn open(setup_json: &[u8], library: &Library) -> Result<Self, JobError> {
        let setup: Setup =
            serde_json::from_slice(setup_json).map_err(|e| JobError::Malformed(e.to_string()))?;
        let assets = library.assets()?;

        let bands = Bands {
            header: setup.header,
            footer: setup.footer,
        };
        let session = Session::open_with(
            setup.page,
            bands,
            setup.accumulators.len(),
            assets,
            Options::default(),
        )
        .map_err(|e| JobError::Build(e.to_string()))?
        .with_accumulator_names(setup.accumulators);

        let session = match setup.resume {
            Some(resume) => session.resuming(resume.page, resume.total, resume.opening),
            None => session,
        };

        Ok(Self { session })
    }

    pub fn nodes(&mut self, json: &[u8]) -> Result<(), JobError> {
        self.feed(Chunk::Nodes(parse(json)?))
    }

    pub fn open_table(&mut self, json: &[u8]) -> Result<(), JobError> {
        self.feed(Chunk::OpenTable(parse(json)?))
    }

    pub fn rows(&mut self, json: &[u8]) -> Result<(), JobError> {
        self.feed(Chunk::Rows(parse(json)?))
    }

    /// Adds rows this instance already measured. See
    /// [`imprenta_pdf::session::Session::feed_measured`].
    pub fn rows_measured(
        &mut self,
        rows: &[imprenta_pdf::build::MeasuredRow],
    ) -> Result<(), JobError> {
        self.session
            .feed_measured(rows)
            .map_err(|e| JobError::Build(e.to_string()))
    }

    /// Measures and adds rows against `head`, for the page that straddles the
    /// seam between two fragments.
    pub fn rows_in(&mut self, head: &ir::TableHead, json: &[u8]) -> Result<(), JobError> {
        self.feed(Chunk::OpenTable(head.clone()))?;
        self.rows(json)?;
        self.feed(Chunk::CloseTable)
    }

    pub fn close_table(&mut self) -> Result<(), JobError> {
        self.feed(Chunk::CloseTable)
    }

    /// Atoms the engine is still holding: about a page's worth, whatever has
    /// been fed. Flat is the whole point, so it is worth being able to look.
    pub fn pending(&self) -> usize {
        self.session.pending()
    }

    pub fn finish(self) -> Result<Outcome, JobError> {
        let built = self
            .session
            .finish()
            .map_err(|e| JobError::Build(e.to_string()))?;
        Ok(Outcome {
            pdf: built.pdf,
            pages: built.pages,
            diagnostics: built.diagnostics,
        })
    }

    fn feed(&mut self, chunk: Chunk) -> Result<(), JobError> {
        self.session
            .feed(&chunk)
            .map_err(|e| JobError::Build(e.to_string()))
    }
}

fn parse<T: for<'de> Deserialize<'de>>(json: &[u8]) -> Result<T, JobError> {
    serde_json::from_slice(json).map_err(|e| JobError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{FontInput, run};

    const ROBOTO: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Regular.ttf");

    fn roman() -> Library {
        Library {
            fonts: vec![FontInput {
                weight: "regular".into(),
                italic: false,
                data: ROBOTO.to_vec(),
            }],
            images: vec![],
        }
    }

    const SETUP: &[u8] = br#"{ "page": { "width": 595, "height": 842 } }"#;

    fn row(i: usize) -> String {
        format!(
            r#"{{"cells":[{{"text":"Prestación de servicios, asiento {i}"}},{{"text":"1.200,00"}}]}}"#
        )
    }

    /// The same ledger declared whole, so the two routes can be compared.
    fn declared(rows: usize) -> Vec<u8> {
        let body: Vec<String> = (0..rows).map(row).collect();
        format!(
            r#"{{"page":{{"width":595,"height":842}},"children":[{{"t":"table","columns":[{{"width":{{"unit":"percent","value":0.6}}}},{{"width":{{"unit":"percent","value":0.4}}}}],"rows":[{}]}}]}}"#,
            body.join(",")
        )
        .into_bytes()
    }

    const COLUMNS: &[u8] = br#"{"columns":[{"width":{"unit":"percent","value":0.6}},{"width":{"unit":"percent","value":0.4}}]}"#;

    /// The same ledger fed in batches of `batch`.
    fn streamed(rows: usize, batch: usize) -> Outcome {
        let mut printer = Printer::open(SETUP, &roman()).unwrap();
        printer.open_table(COLUMNS).unwrap();
        let mut sent = 0;
        while sent < rows {
            let take = batch.min(rows - sent);
            let body: Vec<String> = (sent..sent + take).map(row).collect();
            printer
                .rows(format!("[{}]", body.join(",")).as_bytes())
                .unwrap();
            sent += take;
        }
        printer.close_table().unwrap();
        printer.finish().unwrap()
    }

    #[test]
    fn a_streamed_document_is_the_one_the_same_content_declares() {
        // The rule this crate is most likely to break silently. If the two
        // routes ever disagree, a document depends on how it was fed — and
        // nothing about the output would look wrong enough to notice.
        let whole = run(&declared(400), &roman()).unwrap();

        let fed = streamed(400, 50);

        assert_eq!(fed.pages, whole.pages);
        assert_eq!(fed.pdf, whole.pdf);
    }

    #[test]
    fn how_the_batches_are_cut_makes_no_difference() {
        let by_one = streamed(120, 1);
        let by_seven = streamed(120, 7);
        let in_one_go = streamed(120, 120);

        assert_eq!(by_one.pdf, by_seven.pdf);
        assert_eq!(by_seven.pdf, in_one_go.pdf);
    }

    #[test]
    fn a_long_document_never_holds_more_than_a_page_or_two() {
        // The property the whole design exists for. If this grows with the
        // document, streaming has stopped working and only memory will say so.
        let mut printer = Printer::open(SETUP, &roman()).unwrap();
        printer.open_table(COLUMNS).unwrap();

        let mut worst = 0;
        for batch in 0..40 {
            let body: Vec<String> = (batch * 100..batch * 100 + 100).map(row).collect();
            printer
                .rows(format!("[{}]", body.join(",")).as_bytes())
                .unwrap();
            worst = worst.max(printer.pending());
        }

        assert!(
            worst < 600,
            "held {worst} atoms at once over four thousand rows"
        );
    }

    #[test]
    fn a_document_with_no_table_is_a_document() {
        let mut printer = Printer::open(SETUP, &roman()).unwrap();

        printer
            .nodes(br#"[{"t":"text","runs":[{"text":"Hola"}]}]"#)
            .unwrap();
        let out = printer.finish().unwrap();

        assert_eq!(&out.pdf[..5], b"%PDF-");
        assert_eq!(out.pages, 1);
    }

    #[test]
    fn a_malformed_batch_is_an_error_and_not_a_panic() {
        let mut printer = Printer::open(SETUP, &roman()).unwrap();

        let err = printer.rows(b"[ not json").unwrap_err();

        assert!(matches!(err, JobError::Malformed(_)), "{err:?}");
    }

    #[test]
    fn rows_with_no_table_open_are_refused_rather_than_dropped() {
        let mut printer = Printer::open(SETUP, &roman()).unwrap();

        assert!(printer.rows(b"[]").is_err());
    }

    #[test]
    fn a_setup_that_is_not_a_page_says_so() {
        let opened = Printer::open(br#"{"page":"A4"}"#, &roman());

        assert!(matches!(opened, Err(JobError::Malformed(_))));
    }

    #[test]
    fn a_header_takes_its_room_on_every_page_and_not_only_the_first() {
        // Deliberately a tall band. A short one costs a few points a page and
        // four hundred rows can absorb that without spilling, so the test
        // passes whether or not the header repeated — which is the failure it
        // is supposed to catch. Three hundred points cannot be absorbed: the
        // page count has to move, and it can only move if every page paid.
        const WITH_BAND: &[u8] = br#"{
            "page": { "width": 595, "height": 842 },
            "header": { "height": 300, "children": [
                { "t": "text", "runs": [{ "text": "Libro mayor" }] }
            ] }
        }"#;

        let mut printer = Printer::open(WITH_BAND, &roman()).unwrap();
        printer.open_table(COLUMNS).unwrap();
        let body: Vec<String> = (0..400).map(row).collect();
        printer
            .rows(format!("[{}]", body.join(",")).as_bytes())
            .unwrap();
        printer.close_table().unwrap();
        let out = printer.finish().unwrap();

        let without = streamed(400, 400);
        assert!(
            out.pages >= without.pages * 3 / 2,
            "{} pages with a 300 pt header, {} without — the band cannot have \
             been on every page",
            out.pages,
            without.pages
        );
    }
}
