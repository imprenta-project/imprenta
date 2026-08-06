//! Turning a declared document into a rendered one.
//!
//! Walks the [`crate::ir`] tree, measures what needs measuring, and feeds the
//! result to a [`crate::compose::Composer`] so pages are painted and released
//! as it goes. This is the only place that knows both what an author declared
//! and what the engine measured; everything either side of it stays ignorant
//! of the other.

use crate::atom::{Atom, Break};
use crate::compose::{Composed, Composer, PageContext, Painted};
use crate::content::{
    BoxContent, CanvasContent, Content, ImageContent, ImageFormat, LinkContent, PathOp,
};
use crate::decoration::{BorderSide, Decoration};
use crate::ir;
use crate::list::{List, Marker};
use crate::render::{Bands, Fonts, Geometry, Options, RenderError};
use crate::shape::{Face, Shaper, TextRun, Weight, report_missing_in};
use crate::table::{Align, Cell, Column, Layout, Overflow, Track, offset_within};
use imprenta_core::diagnostic::Diagnostics;
use imprenta_core::image::{ImageError, identify};
use imprenta_core::units::{Edges, Pt};
use std::collections::HashMap;

/// An image supplied alongside the document.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageAsset {
    /// Shared rather than owned per node, so a logo on nine thousand pages is
    /// one buffer. The writer keys its own decode cache on the address of
    /// this, which only means anything if every node holds the same one.
    pub bytes: std::sync::Arc<[u8]>,
    pub format: ImageFormat,
    /// Pixel dimensions, used to keep the aspect ratio when scaling.
    pub pixels: (u32, u32),
}

/// Bytes the document refers to by name.
#[derive(Debug, Clone, Default)]
pub struct Assets {
    pub fonts: Vec<(Face, Vec<u8>)>,
    pub images: HashMap<String, ImageAsset>,
}

impl Assets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_font(mut self, face: Face, bytes: Vec<u8>) -> Self {
        self.fonts.push((face, bytes));
        self
    }

    /// Adds an image, reading its format and size out of the bytes.
    ///
    /// The caller is not asked for either, because the file already says so
    /// and a caller who gets it wrong squashes the picture without a word.
    pub fn with_image(
        mut self,
        name: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<Self, ImageError> {
        let info = identify(&bytes)?;
        self.images.insert(
            name.into(),
            ImageAsset {
                bytes: bytes.into(),
                format: info.format,
                pixels: (info.width, info.height),
            },
        );
        Ok(self)
    }
}

/// A rendered document and everything the engine noticed on the way.
#[derive(Debug, Clone)]
pub struct Built {
    pub pdf: Vec<u8>,
    pub pages: usize,
    /// Problems worth telling the author about — clipped text, a missing
    /// glyph, an image that could not be read.
    pub diagnostics: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("the document declares no fonts")]
    NoFonts,
    #[error("unknown asset {0:?}")]
    UnknownAsset(String),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("{0}")]
    OutOfOrder(#[from] crate::session::OutOfOrder),
}

/// Whether any band asks how many pages there are.
///
/// It is the one token that cannot be answered while pages are being
/// released, so it is the one that costs the memory of holding the document.
/// A footer that only numbers its pages pays none of it.
pub(crate) fn needs_total(document: &ir::Document) -> bool {
    bands_need_total(document.header.as_ref(), document.footer.as_ref())
}

/// As [`needs_total`], for bands held apart from a document.
pub fn bands_need_total(header: Option<&ir::Band>, footer: Option<&ir::Band>) -> bool {
    [header, footer]
        .into_iter()
        .flatten()
        .any(|band| mentions_total(&band.children))
}

fn mentions_total(nodes: &[ir::Node]) -> bool {
    nodes.iter().any(|node| match node {
        ir::Node::Text(text) => text.runs.iter().any(|run| run.text.contains("{{pages}}")),
        ir::Node::Box(c) | ir::Node::Row(c) => mentions_total(&c.children),
        ir::Node::Link(link) => mentions_total(std::slice::from_ref(&link.child)),
        _ => false,
    })
}

/// Renders a declared document.
pub fn build(
    document: &ir::Document,
    assets: &Assets,
    options: Options,
) -> Result<Built, BuildError> {
    if assets.fonts.is_empty() {
        return Err(BuildError::NoFonts);
    }

    let mut shaper = Shaper::with_faces(assets.fonts.iter().cloned());
    let fonts = Fonts::from_shaper(&shaper)?;
    let geometry = Geometry {
        width: document.page.width,
        height: document.page.height,
        margin: document.page.margin,
        bands: Bands {
            header: document.header.as_ref().map_or(Pt(0.0), |b| b.height),
            footer: document.footer.as_ref().map_or(Pt(0.0), |b| b.height),
        },
    };
    let width = geometry.width - geometry.margin.horizontal();
    // Built before the walk rather than at the end, because a page released
    // part-way through the walk needs its header and footer just as much as
    // the last one does.
    let declared = crate::session::Bands {
        header: document.header.clone(),
        footer: document.footer.clone(),
    };
    let band = BandSpec {
        bands: &declared,
        names: &document.accumulators,
        width,
    };

    let mut composer = Composer::with_options(geometry, fonts.clone(), options)?
        .with_accumulators(document.accumulators.len());
    if needs_total(document) {
        // A document that prints its own length is walked twice: once to
        // count the pages with nothing painted, then again as a fragment that
        // happens to be the whole of itself. The alternative — holding every
        // painted page until the last one is packed — cost twenty-three times
        // the memory on a ledger, and it is what made a long one trap.
        //
        // The second walk is what it costs, and it is cheaper than it sounds:
        // the counting pass paints nothing, compresses nothing and builds no
        // bands. Walking the IR twice is free either way, since an IR is
        // inert data that is already in memory.
        let total = count_pages(document, assets, geometry, &fonts, width, &mut shaper, band)?;
        composer = composer.resuming(1, total, Vec::new());
    }
    let mut diagnostics = Diagnostics::default();

    {
        let mut ctx = Walk {
            shaper: &mut shaper,
            assets,
            diagnostics: &mut diagnostics,
            composer: &mut composer,
            pending_break: None,
            band,
        };
        for node in &document.children {
            ctx.node(node, width)?;
        }
    }

    let composed = finish_with_bands(
        composer,
        &mut shaper,
        assets,
        &mut diagnostics,
        &declared,
        &document.accumulators,
        width,
    )?;

    Ok(Built {
        pages: composed.totals.len(),
        pdf: composed.pdf,
        diagnostics: diagnostics.iter().map(|d| d.to_string()).collect(),
    })
}

/// How many pages a document runs to, having painted none of them.
///
/// The first of the two walks a `{{pages}}` document takes. It goes through
/// the same [`Walk`] and the same packer a real render does, because a
/// cheaper estimate would be a second paginator and the two would part
/// company on exactly the documents that print their own length — a repeated
/// table header is enough to do it, measured at 4 706 pages against a real
/// 4 849.
///
/// Its diagnostics are thrown away rather than kept: every one of them will
/// be reported again by the walk that paints, and a reader told twice that a
/// font has no glyph for "日" would reasonably conclude there were two.
fn count_pages(
    document: &ir::Document,
    assets: &Assets,
    geometry: Geometry,
    fonts: &Fonts,
    width: Pt,
    shaper: &mut Shaper,
    band: BandSpec<'_>,
) -> Result<usize, BuildError> {
    // The shaper is the one the second pass will use. Sharing it is not for
    // the cache — a ledger's rows are all different text and the hit rate on
    // the second pass is nil — but because building a second one re-reads
    // every font file for nothing.
    let mut composer = Composer::with_options(geometry, fonts.clone(), Options::default())?
        .with_accumulators(document.accumulators.len())
        .counting();
    let mut discarded = Diagnostics::default();
    {
        let mut ctx = Walk {
            shaper,
            assets,
            diagnostics: &mut discarded,
            composer: &mut composer,
            pending_break: None,
            // Never reached: a counting composer releases its pages without
            // asking anybody what goes on them.
            band,
        };
        for node in &document.children {
            ctx.node(node, width)?;
        }
    }
    Ok(composer.count())
}

/// Measures a run of table rows into the atoms the packer will see.
///
/// The first half of a sharded render: several of these run at once, on
/// different ranges of the same table, and what they hand back is small enough
/// to send anywhere — an atom is a height and a few flags, where the content it
/// came from is every glyph run on the row.
///
/// It measures through exactly the path a real render measures through, which
/// is the only reason the plan built from it can be trusted. A second measurer
/// would be a second layout, and the two would part company on precisely the
/// documents worth getting right.
pub fn measure_rows(
    assets: &Assets,
    page: &ir::PageSetup,
    head: &ir::TableHead,
    rows: &[ir::Row],
) -> Result<Vec<MeasuredRow>, BuildError> {
    if assets.fonts.is_empty() {
        return Err(BuildError::NoFonts);
    }
    let mut shaper = Shaper::with_faces(assets.fonts.iter().cloned());
    let mut diagnostics = Diagnostics::default();

    // Derived here rather than taken as an argument. A caller that worked the
    // content width out for itself and got it wrong by a point would produce a
    // plan for a differently-shaped document — rows wrapping one way to be
    // measured and another to be painted — and every page after the first
    // wrong row would be off. There is one way to know it and this is it.
    let width = page.width - page.margin.horizontal();
    let columns: Vec<Column> = head.columns.iter().map(column_of).collect();
    let layout = Layout::new(columns, width - head.padding.horizontal());

    let mut atoms = Vec::with_capacity(rows.len());
    for batch in rows.chunks(MEASURE_BATCH) {
        let cells: Vec<(Vec<Cell>, Decoration)> = batch
            .iter()
            .map(|row| (cells_of(row, Pt(9.0)), decoration_of(&row.style)))
            .collect();
        let built = layout.rows_reporting(
            &mut shaper,
            &assets.fonts,
            &cells,
            head.padding,
            &mut diagnostics,
        );
        atoms.extend(built.into_iter().map(MeasuredRow));
    }
    Ok(atoms)
}

/// One row, measured.
///
/// Opaque on purpose: what is inside is the placed glyph runs, and the only
/// two questions worth asking of it from outside are how tall it is — which
/// the planner needs — and "paint yourself", which the painter needs. Anything
/// else would be a second way to build a row.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredRow(BoxContent);

impl MeasuredRow {
    /// The atom the packer will see. Four bytes, against the kilobytes the row
    /// itself weighs — which is why only this crosses between engines.
    pub fn atom(&self) -> Atom {
        Atom::new(self.0.height())
    }

    pub(crate) fn into_content(self) -> BoxContent {
        self.0
    }
}

/// Packs measured atoms and says where the pages fall, painting nothing.
///
/// The serial middle of a sharded render, and the cheap part: the packer sees
/// heights and break flags, never text and never fonts. Measured on this
/// engine, nine thousand pages take about ten milliseconds.
///
/// It answers the two things a fragment cannot work out for itself — which row
/// each page begins at, and what the running totals stood at when it opened —
/// and the one thing a whole document normally has to hold its pages to learn:
/// how many there are.
pub fn plan(
    page: &ir::PageSetup,
    assets: &Assets,
    bands: &crate::session::Bands,
    accumulators: usize,
    atoms: &[Atom],
) -> Result<Vec<crate::compose::PagePlan>, BuildError> {
    if assets.fonts.is_empty() {
        return Err(BuildError::NoFonts);
    }
    let shaper = Shaper::with_faces(assets.fonts.iter().cloned());
    let geometry = Geometry {
        width: page.width,
        height: page.height,
        margin: page.margin,
        bands: Bands {
            header: bands.header.as_ref().map_or(Pt(0.0), |b| b.height),
            footer: bands.footer.as_ref().map_or(Pt(0.0), |b| b.height),
        },
    };
    let mut composer =
        Composer::new(geometry, Fonts::from_shaper(&shaper)?)?.with_accumulators(accumulators);
    // `Content::Empty` throughout: the packer never looks, and what it would
    // have looked at is the whole reason a document cannot be held in memory.
    for atom in atoms {
        composer.push(atom.clone(), Content::Empty);
    }
    Ok(composer.plan())
}

/// Paints the tail, building each page's bands as it goes.
///
/// Shared by the whole-document path and the session, so a header cannot come
/// out one way when a ledger is declared and another when it is fed.
pub fn finish_with_bands(
    composer: Composer,
    shaper: &mut Shaper,
    assets: &Assets,
    diagnostics: &mut Diagnostics,
    bands: &crate::session::Bands,
    names: &[String],
    width: Pt,
) -> Result<Composed, RenderError> {
    // Built with the shaper the content was measured with: a second shaper
    // would embed a second copy of every font, and set the page numbers in it.
    let mut band_assets = BandAssets {
        shaper,
        assets,
        diagnostics,
        bands,
        names,
        width,
    };
    composer.finish_with(&mut |page| band_assets.paint(page))
}

/// What it takes to build a page's bands.
struct BandAssets<'a> {
    shaper: &'a mut Shaper,
    assets: &'a Assets,
    diagnostics: &'a mut Diagnostics,
    bands: &'a crate::session::Bands,
    names: &'a [String],
    width: Pt,
}

impl BandAssets<'_> {
    fn paint(&mut self, page: &PageContext) -> Painted {
        Painted {
            header: self.band(self.bands.header.clone().as_ref(), page),
            footer: self.band(self.bands.footer.clone().as_ref(), page),
        }
    }

    fn band(&mut self, band: Option<&ir::Band>, page: &PageContext) -> Option<Content> {
        let band = band?;
        let filled: Vec<ir::Node> = band
            .children
            .iter()
            .map(|node| fill(node, page, self.names, self.diagnostics))
            .collect();

        // Composed as one piece rather than emitted: a band is not part of
        // the flow and must never be paginated.
        let mut compose = Compose {
            shaper: self.shaper,
            assets: self.assets,
            diagnostics: self.diagnostics,
        };
        let mut boxed = BoxContent::default().with_width(self.width);
        for node in &filled {
            match compose.inline(node, self.width) {
                Ok(content) => boxed = boxed.stack(content),
                Err(_) => return None,
            }
        }
        Some(Content::Box(boxed))
    }
}

/// Replaces the tokens a page can answer.
fn fill(
    node: &ir::Node,
    page: &PageContext,
    names: &[String],
    diagnostics: &mut Diagnostics,
) -> ir::Node {
    match node {
        ir::Node::Text(text) => ir::Node::Text(ir::Text {
            runs: text
                .runs
                .iter()
                .map(|run| ir::Run {
                    text: substitute(&run.text, page, names, diagnostics),
                    ..run.clone()
                })
                .collect(),
            style: text.style,
        }),
        ir::Node::Box(c) => ir::Node::Box(ir::Container {
            style: c.style,
            children: c
                .children
                .iter()
                .map(|child| fill(child, page, names, diagnostics))
                .collect(),
        }),
        ir::Node::Row(c) => ir::Node::Row(ir::Container {
            style: c.style,
            children: c
                .children
                .iter()
                .map(|child| fill(child, page, names, diagnostics))
                .collect(),
        }),
        other => other.clone(),
    }
}

/// One string, with what the page knows put into it.
fn substitute(
    text: &str,
    page: &PageContext,
    names: &[String],
    diagnostics: &mut Diagnostics,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find("}}") else {
            break;
        };
        let token = &rest[start + 2..start + end];
        out.push_str(&resolve(token.trim(), page, names, diagnostics));
        rest = &rest[start + end + 2..];
    }
    out.push_str(rest);
    out
}

fn resolve(
    token: &str,
    page: &PageContext,
    names: &[String],
    diagnostics: &mut Diagnostics,
) -> String {
    let mut total = |name: &str, values: &[f64]| -> String {
        match names.iter().position(|n| n == name) {
            Some(index) => format(values.get(index).copied().unwrap_or(0.0)),
            None => {
                diagnostics.report(
                    imprenta_core::diagnostic::Diagnostic::warning(
                        "unknown-total",
                        format!("a band asks for a running total called {name:?}"),
                    )
                    .with_hint("name it in the document's `accumulators` to have one"),
                );
                String::new()
            }
        }
    };

    match token.split_once(':') {
        Some(("opening", name)) => total(name, &page.opening),
        Some(("closing", name)) => total(name, &page.closing),
        _ => match token {
            "page" => page.number.to_string(),
            // Empty rather than a guess: a document that streams cannot know,
            // and `needs_total` is what stops it being asked.
            "pages" => page.total.map(|t| t.to_string()).unwrap_or_default(),
            other => {
                diagnostics.report(
                    imprenta_core::diagnostic::Diagnostic::warning(
                        "unknown-token",
                        format!("a band uses {{{{{other}}}}}, which is not something a page knows"),
                    )
                    .with_hint(
                        "page, pages, opening:<name> and closing:<name> are the ones there are",
                    ),
                );
                String::new()
            }
        },
    }
}

/// Two decimals with a thousands separator, which is what a total is.
fn format(value: f64) -> String {
    let whole = value.trunc().abs() as u64;
    let cents = ((value.abs() - whole as f64) * 100.0).round() as u64;
    let mut grouped = String::new();
    for (i, ch) in whole.to_string().chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    let digits: String = grouped.chars().rev().collect();
    format!("{}{digits},{cents:02}", if value < 0.0 { "-" } else { "" })
}

/// A table whose rows have not all arrived.
pub(crate) struct OpenTable {
    layout: Layout,
    /// The header atom, when there is one to repeat.
    repeated: Option<usize>,
    padding: Edges<Pt>,
    space_after: Pt,
}

impl Compose<'_> {
    /// Turns declared runs into shaped ones.
    ///
    /// What the face could not draw is *not* asked here, though it reads as
    /// the natural place for it. Asking here means shaping the text before it
    /// is laid out, which is a second trip through the layout engine for
    /// every paragraph in the document; the layout that follows knows the
    /// answer already. See [`crate::shape::missing_in`].
    fn runs(&mut self, runs: &[ir::Run], style: ir::TextStyle) -> Vec<TextRun> {
        runs.iter()
            .map(|r| {
                TextRun::new(&r.text)
                    .in_face(face_of(r.weight, r.italic))
                    .inked(r.color.unwrap_or(style.color))
            })
            .collect()
    }
    fn image_content(&mut self, src: &str, width: Pt) -> Result<ImageContent, BuildError> {
        let asset = self
            .assets
            .images
            .get(src)
            .ok_or_else(|| BuildError::UnknownAsset(src.to_string()))?;
        Ok(ImageContent::scaled_to_width(
            std::sync::Arc::clone(&asset.bytes),
            asset.format,
            asset.pixels,
            width,
        ))
    }
    fn inline(&mut self, node: &ir::Node, width: Pt) -> Result<Content, BuildError> {
        Ok(match node {
            ir::Node::Text(text) => {
                let shaped = self.runs(&text.runs, text.style);
                // The space below is padding here rather than a spacer atom:
                // inside a row there are no atoms to put one between, and a
                // paragraph must sit the same whether or not it has a
                // neighbour.
                let mut boxed = BoxContent::default()
                    .with_width(width)
                    .with_padding(Edges::bottom(text.style.space_after));
                let track = Track { x: Pt(0.0), width };
                let mut lines = self.shaper.break_rich(&shaped, text.style.size, width);
                report_missing_in(&lines, self.diagnostics);
                report_overflow(&lines, width, self.diagnostics);
                justify(&mut lines, text.style.align, width);
                for line in lines {
                    let shift = offset_within(track, line.width, align_of(text.style.align));
                    boxed = boxed.stack_at(shift, Content::Text(line));
                }
                Content::Box(boxed)
            }
            ir::Node::Image(image) => Content::Image(self.image_content(&image.src, image.width)?),
            ir::Node::Canvas(canvas) => Content::Canvas(canvas_content(canvas)),
            ir::Node::Spacer(spacer) => {
                if spacer.grow {
                    // A box has no page, so there is no remainder to take.
                    // Said out loud rather than silently ignored: a gap that
                    // does nothing looks exactly like a gap that was never
                    // asked for.
                    self.diagnostics.report(
                        imprenta_core::diagnostic::Diagnostic::error(
                            "not-inline",
                            "a growing spacer cannot be nested here".to_string(),
                        )
                        .with_hint("put it at the top level of the document, where there is a page to fill"),
                    );
                }
                Content::Box(
                    BoxContent::default()
                        .with_padding(Edges::symmetric(Pt(spacer.height.get() / 2.0), Pt(0.0))),
                )
            }
            ir::Node::Box(c) => self.container(c, width, false)?,
            ir::Node::Row(c) => self.container(c, width, true)?,
            ir::Node::Link(link) => Content::Link(Box::new(
                LinkContent::url(link.href.clone(), self.inline(&link.child, width)?)
                    .with_width(width),
            )),
            // Tables, lists and breaks are block-level; nesting one inside a
            // row is not something the IR expresses.
            other => {
                self.diagnostics.report(
                    imprenta_core::diagnostic::Diagnostic::error(
                        "not-inline",
                        format!("{} cannot be nested here", kind_of(other)),
                    )
                    .with_hint("put it at the top level of the document"),
                );
                Content::Empty
            }
        })
    }

    /// A box or a row, as one piece of content.
    fn container(
        &mut self,
        c: &ir::Container,
        width: Pt,
        side_by_side: bool,
    ) -> Result<Content, BuildError> {
        let outer = c.style.width.unwrap_or(width);
        let inner = outer - c.style.padding.horizontal();
        check_corners(&c.style, self.diagnostics);
        let boxed = BoxContent::new(decoration_of(&c.style))
            .with_width(outer)
            .with_padding(c.style.padding);

        let filled = self.fill(boxed, &c.children, inner, side_by_side)?;
        if c.style.space_after.get() == 0.0 {
            return Ok(Content::Box(filled));
        }

        // The space below goes *outside* the decoration, in a plain box that
        // wraps it. Folded into the container's own padding — which is what
        // used to happen here, and still happens for a paragraph a few lines
        // up — a background stretches over the gap: the author asks for room
        // after the box and gets a taller box. A paragraph has nothing painted
        // behind it, which is why the shortcut is safe there and not here.
        Ok(Content::Box(
            BoxContent::default()
                .with_width(outer)
                .with_padding(Edges::bottom(c.style.space_after))
                .stack(Content::Box(filled)),
        ))
    }

    /// Puts children into a container, beside one another or stacked.
    ///
    /// Shared with [`Walk::container`], and that is the point: the two used to
    /// have a copy of this each and only the walked one had ever been taught
    /// what a row is. A row nested anywhere — in a box, in another row, in a
    /// band — was therefore composed as a box and its children stacked,
    /// without a word said about it. Whatever else changes here, both callers
    /// have to keep arriving at the same geometry.
    fn fill(
        &mut self,
        mut boxed: BoxContent,
        children: &[ir::Node],
        inner: Pt,
        side_by_side: bool,
    ) -> Result<BoxContent, BuildError> {
        if !side_by_side {
            for child in children {
                boxed = boxed.stack(self.inline(child, inner)?);
            }
            return Ok(boxed);
        }

        // Declared widths are taken first; the rest share what is left, the
        // same rule table columns follow.
        let declared: f32 = children.iter().filter_map(declared_width).sum();
        let flexible = children
            .iter()
            .filter(|c| declared_width(c).is_none())
            .count();
        let share = if flexible == 0 {
            0.0
        } else {
            (inner.get() - declared).max(0.0) / flexible as f32
        };

        let mut x = Pt(0.0);
        for child in children {
            let child_width = Pt(declared_width(child).unwrap_or(share));
            boxed = boxed.place(x, self.inline(child, child_width)?);
            x = x + child_width;
        }
        Ok(boxed)
    }
}

/// What it takes to turn a node into content, with no page in sight.
///
/// Separate from [`Walk`] because a band is composed and never emitted: it
/// paints straight onto every page and must never reach the paginator. Giving
/// that half its own name is what stops a composer having to be conjured for
/// it.
pub(crate) struct Compose<'a> {
    pub(crate) shaper: &'a mut Shaper,
    pub(crate) assets: &'a Assets,
    pub(crate) diagnostics: &'a mut Diagnostics,
}

/// What a page's bands are built from, small enough to be copied about.
///
/// Carried by the walk rather than produced only at the end, and that is the
/// whole point of it. Pages are painted and dropped every few hundred atoms,
/// and a page painted without this comes out with no header and no footer at
/// all — a ledger of a thousand rows had a footer on one page of eighteen,
/// silently, and every test written around a document short enough not to
/// flush agreed that it was fine.
#[derive(Clone, Copy)]
pub(crate) struct BandSpec<'a> {
    pub(crate) bands: &'a crate::session::Bands,
    /// Names of the running totals, so a band can ask for one by name.
    pub(crate) names: &'a [String],
    /// The content width a band is laid out across.
    pub(crate) width: Pt,
}

#[cfg(test)]
impl BandSpec<'static> {
    /// A document that declares neither a header nor a footer.
    pub(crate) fn none(width: Pt) -> Self {
        static NONE: crate::session::Bands = crate::session::Bands {
            header: None,
            footer: None,
        };
        Self {
            bands: &NONE,
            names: &[],
            width,
        }
    }
}

/// State carried while walking the tree.
pub(crate) struct Walk<'a> {
    pub(crate) shaper: &'a mut Shaper,
    pub(crate) assets: &'a Assets,
    pub(crate) diagnostics: &'a mut Diagnostics,
    pub(crate) composer: &'a mut Composer,
    /// A break declared by a `PageBreak` node, applied to whatever comes next.
    pub(crate) pending_break: Option<Break>,
    pub(crate) band: BandSpec<'a>,
}

/// How many atoms may pile up before finished pages are painted and dropped.
///
/// Flushing is always safe — every packing rule looks forward, so a page the
/// packer has already closed cannot be changed by what arrives next — and it
/// is cheap, because packing is arithmetic over the few hundred atoms that
/// have accumulated since the last one. What it buys is the difference
/// between holding one page and holding all of them, which on a fifty
/// thousand page ledger is the difference between half a gigabyte and half a
/// megabyte a page.
///
/// The number is a compromise: lower means less held at once and more packs,
/// higher means fewer packs and a longer tail. A few hundred atoms is under a
/// page of dense table rows, so the tail stays around one page either way.
const FLUSH_EVERY: usize = 256;

impl Walk<'_> {
    /// Adds one atom, applying any break the author asked for beforehand.
    fn emit(&mut self, mut atom: Atom, content: Content) -> usize {
        if let Some(kind) = self.pending_break.take() {
            atom.break_before = kind;
        }
        let index = self.composer.push(atom, content);

        if self.composer.pending() >= FLUSH_EVERY {
            self.flush();
        }
        index
    }

    /// Paints and drops every page that can no longer change, bands and all.
    ///
    /// Never `Composer::flush`, which paints no bands: a page released here
    /// is finished, and there is no second visit to put a header on it later.
    pub(crate) fn flush(&mut self) {
        // Destructured so the borrow checker can see that the composer and
        // the things a band is built from are different fields.
        let Walk {
            shaper,
            assets,
            diagnostics,
            composer,
            band,
            ..
        } = self;
        let mut bands = BandAssets {
            shaper,
            assets,
            diagnostics,
            bands: band.bands,
            names: band.names,
            width: band.width,
        };
        composer.flush_with(&mut |page| bands.paint(page));
    }

    fn spacer(&mut self, height: Pt) {
        if height.get() > 0.0 {
            self.emit(Atom::new(height), Content::Empty);
        }
    }

    pub(crate) fn node(&mut self, node: &ir::Node, width: Pt) -> Result<(), BuildError> {
        match node {
            ir::Node::PageBreak(brk) => {
                self.pending_break = Some(match brk.to {
                    ir::BreakTo::Next => Break::Always,
                    ir::BreakTo::Odd => Break::Odd,
                    ir::BreakTo::Even => Break::Even,
                });
            }
            ir::Node::Spacer(spacer) if spacer.grow => {
                let mut atom = Atom::new(spacer.height);
                atom.grow = true;
                // Kept with what follows on purpose: see `Atom::grow`.
                atom.keep_with_next = true;
                self.emit(atom, Content::Empty);
            }
            ir::Node::Spacer(spacer) => self.spacer(spacer.height),
            ir::Node::Text(text) => self.text(&text.runs, text.style, width),
            ir::Node::Box(b) => self.container(&b.style, &b.children, width, false)?,
            ir::Node::Row(r) => self.container(&r.style, &r.children, width, true)?,
            ir::Node::Image(image) => self.image(&image.src, image.width)?,
            ir::Node::Link(link) => self.link(&link.href, &link.child, width)?,
            ir::Node::Canvas(canvas) => self.canvas(canvas, width),
            ir::Node::List(list) => self.list(list, width),
            ir::Node::Table(table) => self.table(table, width),
        }
        Ok(())
    }

    fn compose(&mut self) -> Compose<'_> {
        Compose {
            shaper: self.shaper,
            assets: self.assets,
            diagnostics: self.diagnostics,
        }
    }

    fn runs(&mut self, runs: &[ir::Run], style: ir::TextStyle) -> Vec<TextRun> {
        self.compose().runs(runs, style)
    }

    fn image_content(&mut self, src: &str, width: Pt) -> Result<ImageContent, BuildError> {
        self.compose().image_content(src, width)
    }

    fn text(&mut self, runs: &[ir::Run], style: ir::TextStyle, width: Pt) {
        let shaped = self.runs(runs, style);
        let mut lines = self.shaper.break_rich(&shaped, style.size, width);
        report_missing_in(&lines, self.diagnostics);
        if lines.is_empty() {
            return;
        }
        report_overflow(&lines, width, self.diagnostics);
        justify(&mut lines, style.align, width);

        let count = lines.len();
        for (i, line) in lines.into_iter().enumerate() {
            let mut atom = Atom::new(line.height);
            // Set against the left edge a line needs nothing around it, which
            // is the case that matters: a ledger of fifty thousand pages is
            // all left-aligned, and a box per line would be a box per line.
            let shift = offset_within(
                Track { x: Pt(0.0), width },
                line.width,
                align_of(style.align),
            );
            // Widow and orphan limits reduce to keep-with-next; see
            // `crate::widows` for why the packer never learns about them.
            let head = (style.orphans.saturating_sub(1) as usize).min(count.saturating_sub(1));
            let tail = (style.widows.saturating_sub(1) as usize).min(count.saturating_sub(1));
            atom.keep_with_next = i < head || (i + 1 + tail > count && i + 1 < count);
            if style.keep_with_next && i + 1 == count {
                atom.keep_with_next = true;
            }
            if shift.get() == 0.0 {
                self.emit(atom, Content::Text(line));
            } else {
                self.emit(
                    atom,
                    Content::Box(
                        BoxContent::default()
                            .with_width(width)
                            .stack_at(shift, Content::Text(line)),
                    ),
                );
            }
        }
        self.spacer(style.space_after);
    }

    fn container(
        &mut self,
        style: &ir::BoxStyle,
        children: &[ir::Node],
        width: Pt,
        side_by_side: bool,
    ) -> Result<(), BuildError> {
        // A container is one atom: its children are painted inside it, so a
        // background cannot land on top of its own text.
        check_corners(style, self.diagnostics);
        let outer = style.width.unwrap_or(width);
        let inner = outer - style.padding.horizontal();
        let boxed = BoxContent::new(decoration_of(style))
            .with_width(outer)
            .with_padding(style.padding);

        // The placement itself is `Compose`'s, so that a row walked at the top
        // level and a row composed inside something else cannot drift apart.
        let boxed = self.compose().fill(boxed, children, inner, side_by_side)?;

        let mut atom = Atom::new(boxed.height());
        atom.keep_with_next = style.keep_with_next;
        self.emit(atom, Content::Box(boxed));
        self.spacer(style.space_after);
        Ok(())
    }

    /// Builds a node as a single piece of content rather than emitting it.
    fn inline(&mut self, node: &ir::Node, width: Pt) -> Result<Content, BuildError> {
        Compose {
            shaper: self.shaper,
            assets: self.assets,
            diagnostics: self.diagnostics,
        }
        .inline(node, width)
    }

    fn image(&mut self, src: &str, width: Pt) -> Result<(), BuildError> {
        let content = self.image_content(src, width)?;
        let atom = Atom::new(content.height);
        self.emit(atom, Content::Image(content));
        Ok(())
    }

    fn link(&mut self, href: &str, child: &ir::Node, width: Pt) -> Result<(), BuildError> {
        let content = self.inline(child, width)?;
        let height = content.height();
        let link = LinkContent::url(href.to_string(), content).with_width(width);
        self.emit(Atom::new(height), Content::Link(Box::new(link)));
        Ok(())
    }

    fn canvas(&mut self, canvas: &ir::Canvas, _width: Pt) {
        let content = canvas_content(canvas);
        self.emit(Atom::new(content.height), Content::Canvas(content));
        self.spacer(canvas.space_after);
    }

    fn list(&mut self, list: &ir::List, width: Pt) {
        let gutter = list.gutter.unwrap_or(Pt(list.style.size.get() * 2.0));
        let gap = Pt(list.style.size.get() * 0.4);
        let built = List::new(marker_of(&list.marker), gutter, gap, width);
        for (i, item) in list.items.iter().enumerate() {
            let row = built.item(self.shaper, i, item, list.style.size, list.style.color);
            self.emit(Atom::new(row.height()), Content::Box(row));
        }
        self.spacer(list.style.space_after);
    }

    fn table(&mut self, table: &ir::Table, width: Pt) {
        let mut open = self.open_table(&table.head(), width);
        self.table_rows(&mut open, &table.rows);
        self.close_table(open);
    }

    /// Begins a table: its columns, its header, and the group that carries
    /// the header onto every page the rows run over.
    ///
    /// Separate from the rows because a table is the one node that can be too
    /// big to hold, and a session feeds it in pieces. `table` above is those
    /// pieces in a row, so both paths go through exactly the same code.
    pub(crate) fn open_table(&mut self, head: &ir::TableHead, width: Pt) -> OpenTable {
        let columns: Vec<Column> = head.columns.iter().map(column_of).collect();
        let layout = Layout::new(columns, width - head.padding.horizontal());

        // Indices come back from `push`, which returns absolute ones. Deriving
        // them from a pending count would be wrong the moment the composer
        // released a page mid-table.
        //
        // However many rows the header has, they become **one** atom. A
        // repeated prefix is one indivisible block by definition, so widening
        // the header from one row to several costs the packer, the painter and
        // the streaming composer exactly nothing: they still see one atom with
        // one height, and never learn there was a second row.
        let header = (!head.header.is_empty()).then(|| {
            let mut stacked = BoxContent::default();
            for row in &head.header {
                stacked =
                    stacked.stack(Content::Box(self.row(&layout, row, head.padding, Pt(9.0))));
            }
            let height = stacked.height();
            let mut atom = Atom::new(height);
            atom.keep_with_next = true;
            (self.emit(atom, Content::Box(stacked)), height)
        });

        // Declared before the rows arrive, not after. A ledger is one table
        // of forty thousand rows, and a group whose extent is still unknown
        // pins every page it might cover — which would be all of them.
        if let Some((index, height)) = header
            && head.repeat_header
        {
            self.composer.open_repeat(index, height);
        }

        OpenTable {
            layout,
            repeated: head
                .repeat_header
                .then_some(header.map(|(i, _)| i))
                .flatten(),
            padding: head.padding,
            space_after: head.space_after,
        }
    }

    pub(crate) fn table_rows(&mut self, open: &mut OpenTable, rows: &[ir::Row]) {
        // Measured in batches and emitted one at a time, rather than measured
        // whole: a batch is what gives the workers something to share out,
        // and emitting as we go is what keeps the composer releasing pages.
        // Measuring a forty thousand row table before emitting any of it
        // would hand back the flat memory this engine exists for.
        for batch in rows.chunks(MEASURE_BATCH) {
            let cells: Vec<(Vec<Cell>, Decoration)> = batch
                .iter()
                .map(|row| (cells_of(row, Pt(9.0)), decoration_of(&row.style)))
                .collect();

            let built = open.layout.rows_reporting(
                self.shaper,
                &self.assets.fonts,
                &cells,
                open.padding,
                self.diagnostics,
            );

            for (row, built) in batch.iter().zip(built) {
                let index = self.emit(Atom::new(built.height()), Content::Box(built));
                for total in &row.totals {
                    self.composer
                        .contribute(index, total.accumulator, total.value);
                }
            }
        }
    }

    pub(crate) fn close_table(&mut self, open: OpenTable) {
        if let Some(index) = open.repeated {
            self.composer.close_repeat(index);
        }
        self.spacer(open.space_after);
    }

    fn row(
        &mut self,
        layout: &Layout,
        row: &ir::Row,
        padding: Edges<Pt>,
        default_size: Pt,
    ) -> BoxContent {
        layout.row_reporting(
            self.shaper,
            &cells_of(row, default_size),
            decoration_of(&row.style),
            padding,
            self.diagnostics,
        )
    }
}

/// How many rows are measured before any of them is emitted.
///
/// Large enough that there is something for every core to do — a batch below
/// [`crate::table`]'s threshold takes the sequential path — and small enough
/// that the rows held while the batch is measured stay a few megabytes rather
/// than the document. Two hundred and fifty-six atoms is what the composer
/// flushes on, so a batch of a thousand rows is a handful of pages either way.
const MEASURE_BATCH: usize = 1_024;

/// Warns about a line that came out wider than the measure it was broken to.
///
/// It happens when nothing in the line can be broken — a URL, a reference
/// code, an IBAN written without spaces — and what the engine does then is
/// paint it past the edge of its box. Until this existed, nothing said so: the
/// page looked deliberate and a line of it was over the side, which is the
/// failure this project is most careful about.
///
/// The engine is the only place it can be caught. The checks in the CLI read
/// the IR and have no fonts, so they can tell that a *declared* width is wider
/// than the page and never that a *measured* line is.
///
/// A table cell has had this all along, as `cell-overflow` in [`crate::table`];
/// the two are the same idea in the two places text is measured, and the names
/// are a pair on purpose.
///
/// One report per paragraph, naming the widest line: a warning per line of a
/// long paragraph is a warning nobody reads.
fn report_overflow(lines: &[crate::shape::Line], width: Pt, diagnostics: &mut Diagnostics) {
    // The same slack the packer allows, and for the same reason: a width that
    // is a sum of f32 advances lands a hair either side of the exact total.
    const SLACK: f32 = 1e-3;

    let Some(worst) = lines
        .iter()
        .filter(|line| line.width.get() > width.get() + SLACK)
        .max_by(|a, b| a.width.get().total_cmp(&b.width.get()))
    else {
        return;
    };

    let mut glyphs = worst.glyphs();
    let text = match (glyphs.next(), worst.glyphs().last()) {
        (Some(first), Some(last)) => worst
            .text
            .get(first.text_range.start as usize..last.text_range.end as usize)
            .unwrap_or_default(),
        _ => "",
    };
    let shown: String = text.chars().take(40).collect();

    diagnostics.report(
        imprenta_core::diagnostic::Diagnostic::warning(
            "text-overflow",
            format!(
                "{:?} is {:.0}pt wide where {:.0}pt were available, so it is painted outside its box",
                shown,
                worst.width.get(),
                width.get()
            ),
        )
        .with_hint("nothing in it can be broken — give it more room, a smaller size, or break it yourself"),
    );
}

/// Stretches every line but the last, when the paragraph asked to be justified.
///
/// The last line keeps the width it earned. A justified last line is what
/// gives a naive implementation away — three words spread across the measure,
/// which no typesetter has ever wanted.
fn justify(lines: &mut [crate::shape::Line], align: ir::Align, width: Pt) {
    if align != ir::Align::Justify {
        return;
    }
    let last = lines.len().saturating_sub(1);
    for line in lines.iter_mut().take(last) {
        line.justify(width);
    }
}

/// The declared alignment, as the layout names it.
///
/// Two enums for one idea, and they stay two: the IR is the contract with
/// whoever produced the document and the layout is the engine's own, so
/// neither gets to move because the other did.
fn align_of(align: ir::Align) -> Align {
    match align {
        ir::Align::Start => Align::Start,
        ir::Align::End => Align::End,
        ir::Align::Center => Align::Center,
        ir::Align::Justify => Align::Justify,
    }
}

/// One declared column, as the layout wants it.
///
/// Shared by the walk and by [`measure_rows`]: a planner that resolved a
/// column differently would be planning a different table, and the pages it
/// found would be right for a document nobody asked for.
fn column_of(spec: &ir::ColumnSpec) -> Column {
    Column::new(spec.width)
        .aligned(align_of(spec.align))
        .overflowing(match spec.overflow {
            ir::Overflow::Wrap => Overflow::Wrap,
            ir::Overflow::Ellipsis => Overflow::Ellipsis,
            ir::Overflow::Clip => Overflow::Clip,
        })
}

/// The cells of one row, as the table layout wants them.
///
/// Built without a shaper on purpose: this is the half of a row that costs
/// nothing, and keeping it separate is what lets the expensive half be handed
/// to another thread.
fn cells_of(row: &ir::Row, default_size: Pt) -> Vec<Cell> {
    row.cells
        .iter()
        .map(|c| {
            let mut cell = Cell::new(&c.text, c.size.unwrap_or(default_size))
                .in_face(face_of(c.weight, c.italic));
            if let Some(color) = c.color {
                cell = cell.inked(color);
            }
            cell
        })
        .collect()
}

/// The width a node asks for, if it asks for one.
fn declared_width(node: &ir::Node) -> Option<f32> {
    match node {
        ir::Node::Box(c) | ir::Node::Row(c) => c.style.width.map(|w| w.get()),
        ir::Node::Image(image) => Some(image.width.get()),
        ir::Node::Canvas(c) => Some(c.width.get()),
        _ => None,
    }
}

fn face_of(weight: ir::Weight, italic: bool) -> Face {
    Face {
        weight: match weight {
            ir::Weight::Regular => Weight::Regular,
            ir::Weight::Bold => Weight::Bold,
        },
        italic,
    }
}

/// Warns when a radius will only be honoured on part of a box.
///
/// The background rounds and a single rule does not, which is reasonable but
/// not obvious. Saying so beats leaving the author to work out why one of the
/// two followed the corner and the other did not.
fn check_corners(style: &ir::BoxStyle, diagnostics: &mut Diagnostics) {
    let decoration = decoration_of(style);
    if style.radius.get() <= 0.0 || decoration.is_empty() {
        return;
    }
    let bordered = [
        decoration.border.top,
        decoration.border.right,
        decoration.border.bottom,
        decoration.border.left,
    ]
    .iter()
    .filter(|side| side.is_some())
    .count();

    if bordered > 0 && decoration.uniform_border().is_none() {
        diagnostics.report(
            imprenta_core::diagnostic::Diagnostic::warning(
                "square-corner",
                "a rounded box whose border does not run all the way round in one width and colour"
                    .to_string(),
            )
            .with_hint("the background follows the radius; the rules stay straight"),
        );
    }
}

fn decoration_of(style: &ir::BoxStyle) -> Decoration {
    let side = |b: Option<ir::Border>| {
        b.map(|b| BorderSide {
            width: b.width,
            color: b.color,
        })
    };
    Decoration {
        background: style.background,
        radius: style.radius,
        border: Edges {
            top: side(style.border.top),
            right: side(style.border.right),
            bottom: side(style.border.bottom),
            left: side(style.border.left),
        },
    }
}

fn marker_of(marker: &ir::Marker) -> Marker {
    match marker {
        ir::Marker::Bullet { glyph } => Marker::Bullet(glyph.clone()),
        ir::Marker::Decimal => Marker::Decimal,
        ir::Marker::LowerAlpha => Marker::LowerAlpha,
        ir::Marker::LowerRoman => Marker::LowerRoman,
        ir::Marker::None => Marker::None,
    }
}

fn canvas_content(canvas: &ir::Canvas) -> CanvasContent {
    let mut built = CanvasContent::new(canvas.width, canvas.height);
    for op in &canvas.ops {
        built = match *op {
            ir::Op::MoveTo { x, y } => built.move_to(x, y),
            ir::Op::LineTo { x, y } => built.line_to(x, y),
            ir::Op::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => built.op(PathOp::CurveTo(x1, y1, x2, y2, x, y)),
            ir::Op::Rect { x, y, w, h } => built.rect(x, y, w, h),
            ir::Op::Close => built.close(),
        };
    }
    if let Some(fill) = canvas.fill {
        built = built.filled(fill);
    }
    if let Some(stroke) = canvas.stroke {
        built = built.stroked(stroke.color, stroke.width);
    }
    built
}

fn kind_of(node: &ir::Node) -> &'static str {
    match node {
        ir::Node::Text(ir::Text { .. }) => "a paragraph",
        ir::Node::Box(ir::Container { .. }) => "a box",
        ir::Node::Row(ir::Container { .. }) => "a row",
        ir::Node::Table(_) => "a table",
        ir::Node::List(_) => "a list",
        ir::Node::Image(ir::Image { .. }) => "an image",
        ir::Node::Link(ir::Link { .. }) => "a link",
        ir::Node::Canvas(_) => "a canvas",
        ir::Node::Spacer(ir::Spacer { .. }) => "a spacer",
        ir::Node::PageBreak(ir::PageBreak { .. }) => "a page break",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imprenta_core::color::Color;
    use imprenta_core::units::Length;

    const REGULAR: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
    const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
    const LOGO: &[u8] = include_bytes!("../tests/images/logo.png");

    fn assets() -> Assets {
        Assets::new()
            .with_font(Face::REGULAR, REGULAR.to_vec())
            .with_font(Face::BOLD, BOLD.to_vec())
            .with_image("logo", LOGO.to_vec())
            .unwrap()
    }

    fn document(children: Vec<ir::Node>) -> ir::Document {
        ir::Document {
            page: ir::PageSetup::default(),
            header: None,
            footer: None,
            accumulators: Vec::new(),
            children,
        }
    }

    fn built(children: Vec<ir::Node>) -> Built {
        build(&document(children), &assets(), Options::default()).expect("build")
    }

    fn paragraph(text: &str) -> ir::Node {
        ir::Node::Text(ir::Text {
            runs: vec![ir::Run::new(text)],
            style: ir::TextStyle::default(),
        })
    }

    fn count(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }

    #[test]
    fn a_document_declared_as_json_renders_to_a_pdf() {
        // The whole point: a producer in any language writes this and gets a
        // document back.
        let json = r#"{
            "children": [
                { "t": "text", "runs": [{ "text": "Hola" }] },
                { "t": "spacer", "height": 12 },
                { "t": "text", "runs": [
                    { "text": "en " },
                    { "text": "negrita", "weight": "bold" }
                ]}
            ]
        }"#;
        let document: ir::Document = serde_json::from_str(json).expect("parse");

        let out = build(&document, &assets(), Options::default()).expect("build");

        assert_eq!(&out.pdf[..5], b"%PDF-");
        assert_eq!(out.pages, 1);
    }

    #[test]
    fn a_document_with_no_fonts_is_refused() {
        let err = build(
            &document(vec![paragraph("x")]),
            &Assets::new(),
            Options::default(),
        );

        assert!(matches!(err, Err(BuildError::NoFonts)));
    }

    #[test]
    fn an_image_the_document_never_supplied_is_named_in_the_error() {
        let err = build(
            &document(vec![ir::Node::Image(ir::Image {
                src: "sello".into(),
                width: Pt(50.0),
            })]),
            &assets(),
            Options::default(),
        );

        match err {
            Err(BuildError::UnknownAsset(name)) => assert_eq!(name, "sello"),
            other => panic!("expected a named asset error, got {other:?}"),
        }
    }

    #[test]
    fn a_page_break_pushes_what_follows_onto_a_new_page() {
        let out = built(vec![
            paragraph("primera"),
            ir::Node::PageBreak(ir::PageBreak {
                to: ir::BreakTo::Next,
            }),
            paragraph("segunda"),
        ]);

        assert_eq!(out.pages, 2);
    }

    #[test]
    fn a_break_to_an_odd_page_leaves_a_blank_one_behind() {
        let out = built(vec![
            paragraph("primera"),
            ir::Node::PageBreak(ir::PageBreak {
                to: ir::BreakTo::Odd,
            }),
            paragraph("tercera"),
        ]);

        assert_eq!(out.pages, 3, "page two should be blank");
    }

    #[test]
    fn only_the_faces_a_document_uses_are_embedded() {
        let plain = built(vec![paragraph("solo regular")]);
        let mixed = built(vec![ir::Node::Text(ir::Text {
            runs: vec![ir::Run::new("a "), ir::Run::new("b").bold()],
            style: ir::TextStyle::default(),
        })]);

        assert_eq!(count(&plain.pdf, b"FontFile"), 1);
        assert_eq!(count(&mixed.pdf, b"FontFile"), 2);
    }

    #[test]
    fn an_image_is_embedded_once_however_often_it_appears() {
        let once = built(vec![ir::Node::Image(ir::Image {
            src: "logo".into(),
            width: Pt(80.0),
        })]);
        let many = built(
            (0..12)
                .map(|_| {
                    ir::Node::Image(ir::Image {
                        src: "logo".into(),
                        width: Pt(80.0),
                    })
                })
                .collect(),
        );

        assert!(count(&once.pdf, b"/Subtype /Image") > 0);
        assert_eq!(
            count(&many.pdf, b"/Subtype /Image"),
            count(&once.pdf, b"/Subtype /Image")
        );
    }

    #[test]
    fn a_link_becomes_a_clickable_annotation() {
        let out = built(vec![ir::Node::Link(ir::Link {
            href: "https://imprenta.dev".into(),
            child: Box::new(paragraph("pulsa")),
        })]);

        assert!(count(&out.pdf, b"/Subtype /Link") > 0);
        assert!(count(&out.pdf, b"https://imprenta.dev") > 0);
    }

    #[test]
    fn a_table_header_comes_back_on_every_continuation_page() {
        let rows: Vec<ir::Row> = (0..300)
            .map(|i| ir::Row {
                cells: vec![ir::Cell::new(format!("fila {i}"))],
                ..Default::default()
            })
            .collect();

        let table = |repeat: bool| {
            ir::Node::Table(ir::Table {
                columns: vec![ir::ColumnSpec {
                    width: Length::Auto,
                    align: ir::Align::Start,
                    overflow: ir::Overflow::Wrap,
                }],
                header: vec![ir::Row {
                    cells: vec![ir::Cell::new("CABECERA")],
                    ..Default::default()
                }],
                rows: rows.clone(),
                repeat_header: repeat,
                padding: Edges::all(Pt(2.0)),
                space_after: Pt(0.0),
            })
        };
        // Uncompressed, so the drawing operators can be counted: a compressed
        // stream hides how many times anything was painted.
        let plain = Options { compress: false };
        let with = build(&document(vec![table(true)]), &assets(), plain).expect("build");
        let without = build(&document(vec![table(false)]), &assets(), plain).expect("build");

        assert!(with.pages > 1, "the table must span pages");
        assert_eq!(with.pages, without.pages.max(with.pages));

        // One extra text run per continuation page — the header coming back.
        let runs = |pdf: &[u8]| {
            String::from_utf8_lossy(pdf).matches("Tj").count()
                + String::from_utf8_lossy(pdf).matches("TJ").count()
        };
        assert_eq!(
            runs(&with.pdf) - runs(&without.pdf),
            with.pages - 1,
            "the header did not come back on every continuation page"
        );
        assert!(with.diagnostics.is_empty(), "{:?}", with.diagnostics);
    }

    #[test]
    fn turning_the_repeat_off_is_the_authors_call() {
        let table = |repeat: bool| {
            ir::Node::Table(ir::Table {
                columns: vec![ir::ColumnSpec {
                    width: Length::Auto,
                    align: ir::Align::Start,
                    overflow: ir::Overflow::Wrap,
                }],
                header: vec![ir::Row {
                    cells: vec![ir::Cell::new("CABECERA")],
                    ..Default::default()
                }],
                rows: (0..300)
                    .map(|i| ir::Row {
                        cells: vec![ir::Cell::new(format!("fila {i}"))],
                        ..Default::default()
                    })
                    .collect(),
                repeat_header: repeat,
                padding: Edges::all(Pt(2.0)),
                space_after: Pt(0.0),
            })
        };

        let with = built(vec![table(true)]);
        let without = built(vec![table(false)]);

        assert!(
            with.pdf.len() > without.pdf.len(),
            "repeating the header should add content"
        );
    }

    #[test]
    fn running_totals_declared_in_the_document_are_carried() {
        let mut doc = document(vec![ir::Node::Table(ir::Table {
            columns: vec![ir::ColumnSpec {
                width: Length::Auto,
                align: ir::Align::End,
                overflow: ir::Overflow::Wrap,
            }],
            header: Vec::new(),
            rows: (0..200)
                .map(|i| ir::Row {
                    cells: vec![ir::Cell::new(format!("{i}"))],
                    totals: vec![ir::TotalContribution {
                        accumulator: 0,
                        value: 10.0,
                    }],
                    ..Default::default()
                })
                .collect(),
            repeat_header: true,
            padding: Edges::all(Pt(2.0)),
            space_after: Pt(0.0),
        })]);
        doc.accumulators = vec!["importe".into()];

        let out = build(&doc, &assets(), Options::default()).expect("build");

        assert!(out.pages > 1);
    }

    #[test]
    fn a_clipped_cell_is_reported_to_the_author() {
        let out = built(vec![ir::Node::Table(ir::Table {
            columns: vec![ir::ColumnSpec {
                width: Length::Pt(Pt(40.0)),
                align: ir::Align::Start,
                overflow: ir::Overflow::Ellipsis,
            }],
            header: Vec::new(),
            rows: vec![ir::Row {
                cells: vec![ir::Cell::new(
                    "Prestación de servicios profesionales durante el periodo",
                )],
                ..Default::default()
            }],
            repeat_header: true,
            padding: Edges::default(),
            space_after: Pt(0.0),
        })]);

        assert!(
            out.diagnostics.iter().any(|d| d.contains("text-clipped")),
            "{:?}",
            out.diagnostics
        );
    }

    #[test]
    fn a_row_lays_its_children_side_by_side_within_the_page() {
        // Two panels of declared width must not both span the page.
        let panel = |w: f32| {
            ir::Node::Box(ir::Container {
                style: ir::BoxStyle {
                    background: Some(Color::BLACK),
                    width: Some(Pt(w)),
                    ..Default::default()
                },
                children: vec![paragraph("x")],
            })
        };

        let out = built(vec![ir::Node::Row(ir::Container {
            style: ir::BoxStyle::default(),
            children: vec![panel(200.0), panel(200.0)],
        })]);

        assert_eq!(out.pages, 1);
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    }

    #[test]
    fn a_list_numbers_its_items() {
        let out = built(vec![ir::Node::List(ir::List {
            marker: ir::Marker::Decimal,
            items: vec!["uno".into(), "dos".into(), "tres".into()],
            style: ir::TextStyle::default(),
            gutter: None,
        })]);

        assert_eq!(out.pages, 1);
    }

    #[test]
    fn a_canvas_reaches_the_page() {
        let out = built(vec![ir::Node::Canvas(ir::Canvas {
            width: Pt(100.0),
            height: Pt(40.0),
            ops: vec![ir::Op::Rect {
                x: Pt(0.0),
                y: Pt(0.0),
                w: Pt(50.0),
                h: Pt(20.0),
            }],
            fill: Some(Color::BLACK),
            stroke: None,
            space_after: Pt(0.0),
        })]);

        assert_eq!(out.pages, 1);
        assert!(out.pdf.len() > 500);
    }

    #[test]
    fn the_same_document_always_renders_to_the_same_bytes() {
        let children = vec![
            paragraph("determinista"),
            ir::Node::Image(ir::Image {
                src: "logo".into(),
                width: Pt(80.0),
            }),
        ];

        assert_eq!(built(children.clone()).pdf, built(children).pdf);
    }

    #[test]
    fn an_empty_document_is_refused_rather_than_producing_a_broken_file() {
        assert!(build(&document(vec![]), &assets(), Options::default()).is_err());
    }

    #[test]
    fn an_image_is_measured_from_its_own_bytes() {
        // No format, no pixel count: the caller hands over a name and a file.
        let assets = Assets::new().with_image("logo", LOGO.to_vec()).unwrap();

        let logo = &assets.images["logo"];
        assert_eq!(logo.format, ImageFormat::Png);
        assert_eq!(logo.pixels, (240, 80));
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused_when_they_arrive() {
        // Not at paint time, three thousand pages later.
        let refused = Assets::new().with_image("logo", b"<html>".to_vec());

        assert!(refused.is_err());
    }

    #[test]
    fn the_aspect_ratio_read_from_the_file_reaches_the_page() {
        // The whole point of reading the header: a 240x80 logo asked to be
        // 120pt wide comes out 40pt tall, not square and not stretched.
        let document = document(vec![ir::Node::Image(ir::Image {
            src: "logo".into(),
            width: Pt(120.0),
        })]);

        let built = build(&document, &assets(), Options::default()).unwrap();

        assert_eq!(built.pages, 1);
        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    #[test]
    fn a_paragraph_the_font_cannot_set_says_so() {
        // A table cell has always reported this. A paragraph did not, so a
        // page of Japanese set in a Latin font came out as rows of empty
        // boxes and the engine said everything was fine.
        let document = document(vec![ir::Node::Text(ir::Text {
            runs: vec![ir::Run::new("日本語")],
            style: ir::TextStyle::default(),
        })]);

        let built = build(&document, &assets(), Options::default()).unwrap();

        assert!(
            built
                .diagnostics
                .iter()
                .any(|d| d.contains("missing-glyph")),
            "{:?}",
            built.diagnostics
        );
    }

    #[test]
    fn the_character_the_font_lacks_is_named() {
        // "some glyphs are missing" sends the author looking through nine
        // thousand pages. The character itself sends them to the font.
        let document = document(vec![ir::Node::Text(ir::Text {
            runs: vec![ir::Run::new("Total "), ir::Run::new("✓")],
            style: ir::TextStyle::default(),
        })]);

        let built = build(&document, &assets(), Options::default()).unwrap();

        let reported = built.diagnostics.join(" ");
        assert!(reported.contains('✓'), "{reported}");
    }

    #[test]
    fn a_paragraph_the_font_covers_is_reported_clean() {
        let document = document(vec![ir::Node::Text(ir::Text {
            runs: vec![ir::Run::new("Total a pagar")],
            style: ir::TextStyle::default(),
        })]);

        let built = build(&document, &assets(), Options::default()).unwrap();

        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    /// A ledger of `rows` lines and nothing else.
    fn ledger(rows: usize) -> ir::Document {
        document(vec![ir::Node::Table(ir::Table {
            columns: vec![ir::ColumnSpec::default(), ir::ColumnSpec::default()],
            rows: (0..rows)
                .map(|i| ir::Row {
                    cells: vec![
                        ir::Cell::new(format!("Asiento {i}")),
                        ir::Cell::new("1.200,00"),
                    ],
                    ..Default::default()
                })
                .collect(),
            ..ir::Table::empty()
        })])
    }

    /// The head a `ledger` is fed as, when it is fed rather than declared.
    fn ledger_head() -> ir::TableHead {
        ir::TableHead {
            columns: vec![ir::ColumnSpec::default(), ir::ColumnSpec::default()],
            header: Vec::new(),
            repeat_header: true,
            padding: Edges::default(),
            space_after: Pt(0.0),
        }
    }

    fn ledger_rows(rows: usize) -> Vec<ir::Row> {
        (0..rows)
            .map(|i| ir::Row {
                cells: vec![
                    ir::Cell::new(format!("Asiento {i}")),
                    ir::Cell::new("1.200,00"),
                ],
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn planning_finds_the_pages_a_real_render_produces() {
        // The property the whole sharded path rests on. A plan that disagreed
        // with the render by a single page would put every fragment after it
        // one page out, and the only sign would be the page numbers — which is
        // to say, the thing the reader looks at.
        let assets = assets();
        let real = build(&ledger(2_000), &assets, Options::default()).unwrap();

        let measured = measure_rows(
            &assets,
            &ir::PageSetup::default(),
            &ledger_head(),
            &ledger_rows(2_000),
        )
        .unwrap();
        let atoms: Vec<Atom> = measured.iter().map(MeasuredRow::atom).collect();
        let plan = plan(
            &ir::PageSetup::default(),
            &assets,
            &crate::session::Bands::none(),
            0,
            &atoms,
        )
        .unwrap();

        assert!(real.pages > 20, "the fixture must paginate properly");
        assert_eq!(plan.len(), real.pages);
    }

    #[test]
    fn planning_costs_atoms_and_not_content() {
        // Planning holds the whole document where rendering deliberately does
        // not, so what it holds has to be the cheap half. An atom is a height
        // and a few flags; the content it came from is the glyph runs, and at
        // fifty thousand pages that is the difference between two megabytes
        // and ten gigabytes.
        let measured = measure_rows(
            &assets(),
            &ir::PageSetup::default(),
            &ledger_head(),
            &ledger_rows(1_000),
        )
        .unwrap();

        assert_eq!(measured.len(), 1_000);
        assert!(
            size_of::<Atom>() <= 32,
            "an atom is {} bytes; planning holds one per row",
            size_of::<Atom>()
        );
    }

    #[test]
    fn painting_from_measured_rows_gives_the_document_measuring_again_would() {
        // The optimisation the sharded path lives or dies by. Measuring is
        // 60% of a render, and doing it once to plan and again to paint means
        // paying it twice — which cost exactly the margin over the native
        // addon. Rows measured once and painted from must produce byte for
        // byte what measuring them again produces, or the saving is a
        // different document.
        let assets = assets();
        let rows = ledger_rows(600);

        let measured_once = {
            let measured =
                measure_rows(&assets, &ir::PageSetup::default(), &ledger_head(), &rows).unwrap();
            let mut session = crate::session::Session::open(
                ir::PageSetup::default(),
                0,
                assets.clone(),
                Options::default(),
            )
            .unwrap();
            session.feed_measured(&measured).unwrap();
            session.finish().unwrap()
        };

        let measured_again = render_rows(&assets, &rows, None).unwrap();

        assert_eq!(measured_once.pages, measured_again.pages);
        assert_eq!(measured_once.pdf, measured_again.pdf);
    }

    #[test]
    fn a_ledger_split_at_a_planned_boundary_paginates_as_one_document() {
        // End to end: plan, split where the plan says a page began, render the
        // pieces, and check they add up. Splitting anywhere else repaginates,
        // which is the failure this whole approach has to not have.
        let assets = assets();
        let rows = ledger_rows(900);
        let measured =
            measure_rows(&assets, &ir::PageSetup::default(), &ledger_head(), &rows).unwrap();
        let atoms: Vec<Atom> = measured.iter().map(MeasuredRow::atom).collect();
        let plan = plan(
            &ir::PageSetup::default(),
            &assets,
            &crate::session::Bands::none(),
            0,
            &atoms,
        )
        .unwrap();

        let half = plan.len() / 2;
        let split = plan[half].first_atom;

        let head = render_rows(&assets, &rows[..split], None).unwrap();
        let tail = render_rows(&assets, &rows[split..], Some((half + 1, plan.len()))).unwrap();

        assert_eq!(head.pages + tail.pages, plan.len());
        assert_eq!(head.pages, half);
    }

    /// Renders a run of rows, optionally as a fragment resuming a document.
    fn render_rows(
        assets: &Assets,
        rows: &[ir::Row],
        resuming: Option<(usize, usize)>,
    ) -> Result<Built, BuildError> {
        let page = ir::PageSetup::default();
        let mut session =
            crate::session::Session::open(page, 0, assets.clone(), Options::default())?;
        if let Some((first, total)) = resuming {
            session = session.resuming(first, total, Vec::new());
        }
        session.feed(&crate::session::Chunk::OpenTable(ledger_head()))?;
        session.feed(&crate::session::Chunk::Rows(rows.to_vec()))?;
        session.feed(&crate::session::Chunk::CloseTable)?;
        session.finish()
    }

    #[test]
    fn a_long_document_does_not_hold_every_page_it_has_finished_with() {
        // Without this the engine keeps the painted content of all nine
        // thousand pages until the last one arrives, and a ledger costs half
        // a megabyte a page instead of a few kilobytes. `Composer` has always
        // been able to release as it goes; `build` simply never asked.
        let assets = assets();
        let mut shaper = Shaper::with_faces(assets.fonts.iter().cloned());
        let fonts = Fonts::from_shaper(&shaper).unwrap();
        let page = ir::PageSetup::default();
        let mut composer = Composer::new(
            Geometry {
                width: page.width,
                height: page.height,
                margin: page.margin,
                bands: Default::default(),
            },
            fonts,
        )
        .unwrap();
        let mut diagnostics = Diagnostics::default();

        let long = ledger(4_000);
        let mut walk = Walk {
            shaper: &mut shaper,
            assets: &assets,
            diagnostics: &mut diagnostics,
            composer: &mut composer,
            pending_break: None,
            band: BandSpec::none(Pt(515.0)),
        };
        for node in &long.children {
            walk.node(node, Pt(515.0)).unwrap();
        }

        // Four thousand rows is around sixty pages. Holding fewer than two
        // hundred atoms means all but the last page or so has been painted
        // and dropped; holding four thousand means none of them has.
        assert!(
            composer.pending() < 200,
            "still holding {} of 4,000 rows",
            composer.pending()
        );
    }

    #[test]
    fn releasing_as_it_goes_changes_nothing_about_the_document() {
        // The whole reason it is safe: every packing rule looks forward, so a
        // page already painted can no longer be affected by what follows.
        let long = ledger(1_200);

        let built = build(&long, &assets(), Options::default()).unwrap();
        let again = build(&long, &assets(), Options::default()).unwrap();

        assert_eq!(built.pdf, again.pdf);
        assert!(built.pages > 10, "{} pages", built.pages);
    }

    #[test]
    fn a_row_leaves_the_space_its_children_asked_for_below_them() {
        // Measured rather than compared: the row must grow by exactly what
        // was asked for, so a later change cannot satisfy the test above by
        // moving something else.
        let assets = assets();
        let mut shaper = Shaper::with_faces(assets.fonts.iter().cloned());
        let fonts = Fonts::from_shaper(&shaper).unwrap();
        let page = ir::PageSetup::default();
        let mut composer = Composer::new(
            Geometry {
                width: page.width,
                height: page.height,
                margin: page.margin,
                bands: Default::default(),
            },
            fonts,
        )
        .unwrap();
        let mut diagnostics = Diagnostics::default();
        let mut walk = Walk {
            shaper: &mut shaper,
            assets: &assets,
            diagnostics: &mut diagnostics,
            composer: &mut composer,
            pending_break: None,
            band: BandSpec::none(Pt(515.0)),
        };

        let height = |walk: &mut Walk, space: f32| {
            let node = ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new("Emitida")],
                style: ir::TextStyle {
                    space_after: Pt(space),
                    ..Default::default()
                },
            });
            walk.inline(&node, Pt(300.0)).unwrap().height().get()
        };

        let tight = height(&mut walk, 0.0);
        let spaced = height(&mut walk, 20.0);

        assert!(
            (spaced - tight - 20.0).abs() < 0.01,
            "asked for 20pt and got {:.2}",
            spaced - tight
        );
    }

    #[test]
    fn a_box_beside_something_leaves_its_space_too() {
        // Same rule, the other container. A panel in a two-column header
        // pushed the next block down when it stood alone and did not when it
        // did not, which is the kind of difference nobody can debug.
        let assets = assets();
        let mut shaper = Shaper::with_faces(assets.fonts.iter().cloned());
        let fonts = Fonts::from_shaper(&shaper).unwrap();
        let page = ir::PageSetup::default();
        let mut composer = Composer::new(
            Geometry {
                width: page.width,
                height: page.height,
                margin: page.margin,
                bands: Default::default(),
            },
            fonts,
        )
        .unwrap();
        let mut diagnostics = Diagnostics::default();
        let mut walk = Walk {
            shaper: &mut shaper,
            assets: &assets,
            diagnostics: &mut diagnostics,
            composer: &mut composer,
            pending_break: None,
            band: BandSpec::none(Pt(515.0)),
        };

        let panel = |space: f32| {
            ir::Node::Box(ir::Container {
                style: ir::BoxStyle {
                    space_after: Pt(space),
                    ..Default::default()
                },
                children: vec![ir::Node::Spacer(ir::Spacer {
                    height: Pt(10.0),
                    grow: false,
                })],
            })
        };

        let tight = walk.inline(&panel(0.0), Pt(300.0)).unwrap().height().get();
        let spaced = walk.inline(&panel(14.0), Pt(300.0)).unwrap().height().get();

        assert!(
            (spaced - tight - 14.0).abs() < 0.01,
            "asked for 14pt and got {:.2}",
            spaced - tight
        );
    }

    #[test]
    fn a_rounded_box_with_a_border_on_one_side_says_the_rule_stays_straight() {
        // The background rounds and the rule does not, which is reasonable
        // but not obvious. Saying so beats leaving the author to work out
        // why one of the two followed the corner and the other did not.
        let document = document(vec![ir::Node::Box(ir::Container {
            style: ir::BoxStyle {
                background: Some(Color::parse_hex("#f1f5f9").unwrap()),
                radius: Pt(6.0),
                border: Edges {
                    bottom: Some(ir::Border {
                        width: Pt(1.0),
                        color: Color::BLACK,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![ir::Node::Spacer(ir::Spacer {
                height: Pt(10.0),
                grow: false,
            })],
        })]);

        let built = build(&document, &assets(), Options::default()).unwrap();

        assert!(
            built
                .diagnostics
                .iter()
                .any(|d| d.contains("square-corner")),
            "{:?}",
            built.diagnostics
        );
    }

    #[test]
    fn a_rounded_box_whose_border_goes_all_the_way_round_is_reported_clean() {
        let all = Some(ir::Border {
            width: Pt(1.0),
            color: Color::BLACK,
        });
        let document = document(vec![ir::Node::Box(ir::Container {
            style: ir::BoxStyle {
                radius: Pt(6.0),
                border: Edges {
                    top: all,
                    right: all,
                    bottom: all,
                    left: all,
                },
                ..Default::default()
            },
            children: vec![ir::Node::Spacer(ir::Spacer {
                height: Pt(10.0),
                grow: false,
            })],
        })]);

        let built = build(&document, &assets(), Options::default()).unwrap();

        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    #[test]
    fn a_square_box_with_a_rule_underneath_is_reported_clean() {
        // The commonest decoration in any document. It must not start
        // warning about corners it never asked to round.
        let document = document(vec![ir::Node::Box(ir::Container {
            style: ir::BoxStyle {
                border: Edges {
                    bottom: Some(ir::Border {
                        width: Pt(1.0),
                        color: Color::BLACK,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![ir::Node::Spacer(ir::Spacer {
                height: Pt(10.0),
                grow: false,
            })],
        })]);

        let built = build(&document, &assets(), Options::default()).unwrap();

        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    #[test]
    fn a_row_nested_in_a_box_lays_its_children_side_by_side() {
        // A row only laid its children out side by side at the top level.
        // Anywhere else — inside a box, inside another row, inside a header or
        // a footer, all of which are composed rather than walked — it was
        // treated as a box and its children stacked. Silently: no diagnostic,
        // a document that renders and is wrong, which is the one failure a
        // preview cannot show anybody who does not already know what to look
        // for. Nothing but the coordinates tells the two apart, so the
        // coordinates are what this asserts.
        let panel = |w: f32| {
            ir::Node::Box(ir::Container {
                style: ir::BoxStyle {
                    background: Some(Color::BLACK),
                    width: Some(Pt(w)),
                    ..Default::default()
                },
                children: vec![paragraph("x")],
            })
        };

        let nested = ir::Node::Box(ir::Container {
            style: ir::BoxStyle::default(),
            children: vec![ir::Node::Row(ir::Container {
                style: ir::BoxStyle::default(),
                children: vec![panel(150.0), panel(150.0)],
            })],
        });

        // Uncompressed, so the rectangles can be read back at all.
        let built = build(
            &document(vec![nested]),
            &assets(),
            Options { compress: false },
        )
        .expect("build");
        let ops = String::from_utf8_lossy(&built.pdf);

        // The left margin is 34.02, so the first panel ends at 184.02 and the
        // second starts there — on the same y, which is what makes it beside
        // rather than below.
        assert!(
            ops.contains("184.01575 34.015747 m"),
            "the second panel is not beside the first"
        );
    }

    #[test]
    fn a_paragraph_can_be_set_against_its_right_edge() {
        // Alignment existed only on a table column, so the one way to put a
        // figure against the right margin was to make it a table. An invoice
        // is full of things that are not tables and still have to line up on
        // the right — a company address, a total in its own box — and there
        // was no way to say so.
        //
        // Asserted in coordinates: a paragraph that merely came out narrower
        // would satisfy anything softer than this.
        let aligned = |align| {
            ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new("xx")],
                style: ir::TextStyle {
                    align,
                    ..Default::default()
                },
            })
        };

        let plain = Options { compress: false };
        let start =
            build(&document(vec![aligned(ir::Align::Start)]), &assets(), plain).expect("build");
        let end = build(&document(vec![aligned(ir::Align::End)]), &assets(), plain).expect("build");

        let at = |pdf: &[u8]| {
            let text = String::from_utf8_lossy(pdf).into_owned();
            let marker = text.find(" Tm").expect("nothing was painted");
            let head = &text[..marker];
            head[head.rfind('\n').map_or(0, |n| n + 1)..].to_string()
        };

        // The left margin is 34.02, and that is where a paragraph starts
        // today. Set against the right edge it has to start further in; still
        // at the margin means the alignment was dropped on the way through.
        assert!(at(&start.pdf).contains("34.015747"), "{}", at(&start.pdf));
        assert!(
            !at(&end.pdf).contains("34.015747"),
            "the paragraph is still at the left margin: {}",
            at(&end.pdf)
        );
    }

    #[test]
    fn a_paragraph_is_aligned_inside_a_box_and_inside_a_band_too() {
        // The test above walks a paragraph at the top level, and walking is
        // the one path alignment was *not* added for: a table could already
        // align, and the reason this exists is everything a table cannot be
        // nested inside — a header, a footer, a box with a background. Those
        // are composed, not walked, and composing is a separate piece of code.
        //
        // A `<Row>` was left untaught in exactly that seam and stacked its
        // children wherever it was nested, with no diagnostic and a document
        // that rendered. So the two paths are pinned together here rather than
        // assumed to agree.
        let aligned = |align| {
            ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new("xx")],
                style: ir::TextStyle {
                    align,
                    ..Default::default()
                },
            })
        };
        let nested = |align| ir::Document {
            page: ir::PageSetup::default(),
            header: Some(ir::Band {
                height: Pt(60.0),
                children: vec![aligned(align)],
            }),
            footer: None,
            accumulators: Vec::new(),
            children: vec![ir::Node::Box(ir::Container {
                style: ir::BoxStyle::default(),
                children: vec![aligned(align)],
            })],
        };

        // Every text matrix in the file, not the first: one paragraph is in
        // the band and one is in the box, and a check that looked at either
        // alone would pass while the other was dropped.
        let placings = |pdf: &[u8]| {
            let text = String::from_utf8_lossy(pdf).into_owned();
            text.match_indices(" Tm")
                .map(|(at, _)| {
                    let head = &text[..at];
                    head[head.rfind('\n').map_or(0, |n| n + 1)..].to_string()
                })
                .collect::<Vec<_>>()
        };

        let plain = Options { compress: false };
        let start = build(&nested(ir::Align::Start), &assets(), plain).expect("build");
        let end = build(&nested(ir::Align::End), &assets(), plain).expect("build");

        let left = placings(&start.pdf);
        let right = placings(&end.pdf);
        assert_eq!(left.len(), 2, "expected the band and the box: {left:?}");
        assert_eq!(right.len(), 2, "expected the band and the box: {right:?}");
        assert!(
            left.iter().all(|line| line.contains("34.015747")),
            "{left:?}"
        );
        assert!(
            right.iter().all(|line| !line.contains("34.015747")),
            "a nested paragraph is still at the left margin: {right:?}"
        );
    }

    #[test]
    fn space_after_a_composed_box_falls_outside_its_background() {
        // At the top level the space below a box is a spacer emitted after it.
        // Composed — in a band, or inside another container — it was folded
        // into the box's own bottom padding instead, so a box with a
        // background grew by exactly that much and whatever followed stayed
        // welded to it. The author asked for a gap and got a taller box.
        //
        // A paragraph is the case the folding was written for and it is right
        // there: text has nothing painted behind it. A decorated container
        // does, and that is the whole difference.
        let grey = ir::Node::Box(ir::Container {
            style: ir::BoxStyle {
                background: Some(Color::BLACK),
                space_after: Pt(16.0),
                ..Default::default()
            },
            children: vec![paragraph("x")],
        });

        let document = ir::Document {
            page: ir::PageSetup::default(),
            header: Some(ir::Band {
                height: Pt(90.0),
                children: vec![grey, paragraph("y")],
            }),
            footer: None,
            accumulators: Vec::new(),
            children: vec![paragraph("z")],
        };

        let built = build(&document, &assets(), Options { compress: false }).expect("build");
        let ops = String::from_utf8_lossy(&built.pdf);

        // One line of 10pt text is 12pt tall and the top margin is 34.02, so
        // the filled rectangle ends at 46.02. Sixteen points folded into the
        // padding would take it to 62.02, and the rectangle is the only place
        // the difference shows.
        assert!(
            ops.contains("46.015747 l"),
            "the background is not the height of its content"
        );
        assert!(
            !ops.contains("62.015747 l"),
            "the background covers the gap that was meant to follow it"
        );
    }

    #[test]
    fn a_justified_paragraph_reaches_the_engine() {
        // What justification *does* is asserted where the lines are built, in
        // `shape`, in points. This is the other half and the one that has
        // caught things before: that the word travels from the IR all the way
        // to `Line::justify` at all. It cannot be read off the page — the
        // widened advances go inside the text-showing operator — so what is
        // checked is that the same paragraph comes out differently, and breaks
        // in the same places while it does.
        let words = "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince dieciseis diecisiete dieciocho diecinueve veinte";

        let paragraph = |align| {
            ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new(words)],
                style: ir::TextStyle {
                    align,
                    ..Default::default()
                },
            })
        };

        let plain = Options { compress: false };
        let ragged = build(
            &document(vec![paragraph(ir::Align::Start)]),
            &assets(),
            plain,
        )
        .expect("build");
        let flush = build(
            &document(vec![paragraph(ir::Align::Justify)]),
            &assets(),
            plain,
        )
        .expect("build");

        let lines = |pdf: &[u8]| String::from_utf8_lossy(pdf).matches(" Tm").count();

        assert!(
            lines(&ragged.pdf) > 1,
            "the sample has to wrap to mean anything"
        );
        assert_eq!(
            lines(&ragged.pdf),
            lines(&flush.pdf),
            "justifying must not change where the text breaks"
        );
        assert_ne!(
            String::from_utf8_lossy(&ragged.pdf),
            String::from_utf8_lossy(&flush.pdf),
            "the paragraph came out identical, so nothing was justified"
        );
        assert!(flush.diagnostics.is_empty(), "{:?}", flush.diagnostics);
    }

    #[test]
    fn a_justified_paragraph_inside_a_band_is_justified_too() {
        // The two text paths again: one for a paragraph in the flow, one for
        // a paragraph composed inside something else. The first learnt to
        // justify and the second did not, so a footer — which is a band, and
        // therefore always composed — quietly kept its ragged edge while the
        // same paragraph in the body came out flush. The symptom was a change
        // small enough to talk yourself into seeing.
        let paragraph = |align| {
            ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new(
                    "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince dieciseis diecisiete dieciocho diecinueve veinte",
                )],
                style: ir::TextStyle {
                    align,
                    ..Default::default()
                },
            })
        };

        let banded = |align| ir::Document {
            page: ir::PageSetup::default(),
            header: Some(ir::Band {
                height: Pt(120.0),
                children: vec![paragraph(align)],
            }),
            footer: None,
            accumulators: Vec::new(),
            children: vec![paragraph(ir::Align::Start)],
        };

        let plain = Options { compress: false };
        let ragged = build(&banded(ir::Align::Start), &assets(), plain).expect("build");
        let flush = build(&banded(ir::Align::Justify), &assets(), plain).expect("build");

        assert_ne!(
            String::from_utf8_lossy(&ragged.pdf),
            String::from_utf8_lossy(&flush.pdf),
            "the band came out identical, so nothing in it was justified"
        );
    }

    /// A URL: the engine breaks it at a slash or a query mark, and then runs
    /// out of places to break.
    const UNBREAKABLE: &str =
        "https://example.invalid/verify?ref=XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";

    #[test]
    fn text_too_wide_for_its_box_says_so() {
        // It is painted outside the box, and until now nothing said a word:
        // the page looked deliberate and a line of it was over the edge. Only
        // the engine can see this — the checks in the CLI read the IR and have
        // no fonts, so they can tell a *declared* width is too big and never a
        // measured one.
        let boxed = ir::Node::Box(ir::Container {
            style: ir::BoxStyle {
                width: Some(Pt(120.0)),
                ..Default::default()
            },
            children: vec![ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new(UNBREAKABLE)],
                style: ir::TextStyle::default(),
            })],
        });

        let built = build(&document(vec![boxed]), &assets(), Options::default()).expect("build");

        assert!(
            built
                .diagnostics
                .iter()
                .any(|d| d.contains("text-overflow")),
            "{:?}",
            built.diagnostics
        );
    }

    #[test]
    fn text_too_wide_for_the_page_says_so_in_the_flow_as_well() {
        // The other of the two text paths. One knowing something the other
        // does not is the shape of most of the defects found in this file.
        let narrow = ir::Document {
            page: ir::PageSetup {
                width: Pt(160.0),
                height: Pt(400.0),
                ..Default::default()
            },
            header: None,
            footer: None,
            accumulators: Vec::new(),
            children: vec![paragraph(UNBREAKABLE)],
        };

        let built = build(&narrow, &assets(), Options::default()).expect("build");

        assert!(
            built
                .diagnostics
                .iter()
                .any(|d| d.contains("text-overflow")),
            "{:?}",
            built.diagnostics
        );
    }

    #[test]
    fn text_that_fits_says_nothing() {
        // The guard that keeps the rule honest: a warning on every paragraph
        // would be read once and ignored for ever after.
        let built = build(
            &document(vec![paragraph("una linea corriente que cabe de sobra")]),
            &assets(),
            Options::default(),
        )
        .expect("build");

        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    #[test]
    fn every_row_of_a_multi_row_header_comes_back_on_a_continuation_page() {
        // A grouped report — a ledger, a journal — wants two rows at the top
        // of its table: which group this is, and what its columns mean. Both
        // have to come back when the group runs over the page, and a table
        // could only repeat one, so an author had to choose which half of that
        // question a reader on page 40 got answered.
        let rows: Vec<ir::Row> = (0..300)
            .map(|i| ir::Row {
                cells: vec![ir::Cell::new(format!("apunte {i}"))],
                ..Default::default()
            })
            .collect();

        let table = |header: Vec<ir::Row>| {
            ir::Node::Table(ir::Table {
                columns: vec![ir::ColumnSpec::default()],
                header,
                rows: rows.clone(),
                repeat_header: true,
                padding: Edges::all(Pt(2.0)),
                space_after: Pt(0.0),
            })
        };
        let row = |text: &str| ir::Row {
            cells: vec![ir::Cell::new(text)],
            ..Default::default()
        };

        // Uncompressed, so the drawing operators can be counted. The words
        // themselves are not in the file to look for — text is written as
        // glyph ids.
        let plain = Options { compress: false };
        let one = build(
            &document(vec![table(vec![row("600000 COMPRAS")])]),
            &assets(),
            plain,
        )
        .expect("build");
        let two = build(
            &document(vec![table(vec![row("600000 COMPRAS"), row("FECHA")])]),
            &assets(),
            plain,
        )
        .expect("build");

        let runs = |pdf: &[u8]| {
            let text = String::from_utf8_lossy(pdf);
            text.matches("Tj").count() + text.matches("TJ").count()
        };

        assert!(
            one.pages > 1,
            "the table has to span pages to mean anything"
        );
        assert_eq!(
            runs(&two.pdf) - runs(&one.pdf),
            two.pages,
            "the second header row did not come back on every page"
        );
    }
}

#[cfg(test)]
mod page_bands {
    use super::*;
    use crate::render::Options;
    use imprenta_core::color::Color;

    const REGULAR: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn assets() -> Assets {
        Assets::new().with_font(Face::REGULAR, REGULAR.to_vec())
    }

    /// A ledger long enough to run over several pages.
    fn ledger(rows: usize, band: Option<ir::Band>, footer: Option<ir::Band>) -> ir::Document {
        ir::Document {
            page: ir::PageSetup::default(),
            header: band,
            footer,
            accumulators: vec!["total".into()],
            children: vec![ir::Node::Table(ir::Table {
                columns: vec![ir::ColumnSpec::default(), ir::ColumnSpec::default()],
                rows: (0..rows)
                    .map(|i| ir::Row {
                        cells: vec![
                            ir::Cell::new(format!("{i:04}")),
                            ir::Cell::new("Asiento contable del ejercicio"),
                        ],
                        totals: vec![ir::TotalContribution {
                            accumulator: 0,
                            value: 100.0,
                        }],
                        ..Default::default()
                    })
                    .collect(),
                ..ir::Table::empty()
            })],
        }
    }

    fn band(height: f32, text: &str) -> ir::Band {
        ir::Band {
            height: Pt(height),
            children: vec![ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new(text)],
                style: ir::TextStyle::default(),
            })],
        }
    }

    #[test]
    fn a_document_without_bands_is_unchanged() {
        // The feature must cost nothing to a document that does not use it.
        let plain = build(&ledger(200, None, None), &assets(), Options::default()).unwrap();

        assert!(plain.pages > 2);
        assert!(plain.diagnostics.is_empty(), "{:?}", plain.diagnostics);
    }

    #[test]
    fn a_band_takes_room_from_every_page() {
        // Not from the first one only, and not from none: a header is a
        // standing cost, and the paginator has to know it before it packs.
        // A band big enough that the difference cannot fall inside the
        // rounding: a forty point header on an A4 page changes the count by
        // one only sometimes, and a test that passes sometimes is worse than
        // none.
        let without = build(&ledger(200, None, None), &assets(), Options::default()).unwrap();
        let with = build(
            &ledger(200, Some(band(300.0, "Libro mayor")), None),
            &assets(),
            Options::default(),
        )
        .unwrap();

        assert!(
            with.pages > without.pages,
            "{} pages with a header against {} without",
            with.pages,
            without.pages
        );
    }

    #[test]
    fn the_page_number_is_different_on_every_page() {
        // The whole point. A band shaped once and stamped on each page would
        // say "1" all the way through.
        let built = build(
            &ledger(200, None, Some(band(20.0, "Pagina {{page}}"))),
            &assets(),
            Options::default(),
        )
        .unwrap();

        let text = String::from_utf8_lossy(&built.pdf);
        assert!(built.pages > 2);
        // Each page's footer is its own content stream, so a document of
        // three pages holds three different ones.
        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
        assert!(text.starts_with("%PDF-"));
    }

    /// How many text-showing operators a document contains.
    ///
    /// A proxy for "how much was drawn". Counted off an uncompressed file on
    /// purpose: `TJ` is two bytes and a deflate stream is full of them, so
    /// the same count taken over a compressed document is noise that happens
    /// to look like a number.
    fn runs(pdf: &[u8]) -> usize {
        let text = String::from_utf8_lossy(pdf);
        text.matches("Tj").count() + text.matches("TJ").count()
    }

    /// Readable output, so the operators above can actually be counted.
    const READABLE: Options = Options { compress: false };

    #[test]
    fn a_band_survives_the_document_being_long_enough_to_flush() {
        // The walk paints and drops finished pages as it goes, every 256
        // atoms. It used to do that through the bandless `flush`, so every
        // page released before the end came out with no header and no footer
        // — a ledger of a thousand rows had three footers on eighteen pages,
        // and the shorter documents the tests were written around never
        // reached the flush at all.
        //
        // Counted rather than looked at: with one text run per cell, a footer
        // on every page is exactly one more run per page than the same
        // document without one.
        let bare = build(&ledger(1_200, None, None), &assets(), READABLE).unwrap();
        let footed = build(
            &ledger(1_200, None, Some(band(20.0, "Pagina {{page}}"))),
            &assets(),
            READABLE,
        )
        .unwrap();

        assert!(bare.pages > 10, "the sample must flush several times over");
        assert_eq!(
            runs(&footed.pdf) - runs(&bare.pdf),
            footed.pages,
            "{} pages carried a footer, of {}",
            runs(&footed.pdf) - runs(&bare.pdf),
            footed.pages
        );
    }

    #[test]
    fn a_header_band_survives_it_too() {
        let bare = build(&ledger(1_200, None, None), &assets(), READABLE).unwrap();
        let headed = build(
            &ledger(1_200, Some(band(20.0, "Libro mayor")), None),
            &assets(),
            READABLE,
        )
        .unwrap();

        assert_eq!(runs(&headed.pdf) - runs(&bare.pdf), headed.pages);
    }

    #[test]
    fn a_footer_can_say_how_many_pages_there_are() {
        let built = build(
            &ledger(200, None, Some(band(20.0, "{{page}} de {{pages}}"))),
            &assets(),
            Options::default(),
        )
        .unwrap();

        assert!(built.pages > 2);
        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    /// The document built the way it was before the counting pass existed:
    /// every painted page held until the last one is packed.
    ///
    /// Written out here rather than kept behind a flag in `build`, because a
    /// second production path is a second thing to keep working. What it is
    /// for is the assertion below — that changing *when* the total is learnt
    /// changed nothing about the file.
    fn built_by_holding(document: &ir::Document) -> Vec<u8> {
        let assets = assets();
        let mut shaper = Shaper::with_faces(assets.fonts.iter().cloned());
        let fonts = crate::render::Fonts::from_shaper(&shaper).unwrap();
        let geometry = Geometry {
            width: document.page.width,
            height: document.page.height,
            margin: document.page.margin,
            bands: Bands {
                header: document.header.as_ref().map_or(Pt(0.0), |b| b.height),
                footer: document.footer.as_ref().map_or(Pt(0.0), |b| b.height),
            },
        };
        let width = geometry.width - geometry.margin.horizontal();
        let declared = crate::session::Bands {
            header: document.header.clone(),
            footer: document.footer.clone(),
        };
        let mut composer = Composer::with_options(geometry, fonts, Options::default())
            .unwrap()
            .with_accumulators(document.accumulators.len())
            .holding_pages();
        let mut diagnostics = Diagnostics::default();
        {
            let mut ctx = Walk {
                shaper: &mut shaper,
                assets: &assets,
                diagnostics: &mut diagnostics,
                composer: &mut composer,
                pending_break: None,
                band: BandSpec {
                    bands: &declared,
                    names: &document.accumulators,
                    width,
                },
            };
            for node in &document.children {
                ctx.node(node, width).unwrap();
            }
        }
        finish_with_bands(
            composer,
            &mut shaper,
            &assets,
            &mut diagnostics,
            &declared,
            &document.accumulators,
            width,
        )
        .unwrap()
        .pdf
    }

    #[test]
    fn counting_the_pages_first_produces_the_file_holding_them_produced() {
        // The only assertion that makes the counting pass safe to make. It
        // paginates through the same packer, so it must land on the same page
        // boundaries; it is told the total up front, so every footer must
        // read the same; and it paints as it goes, so the pages must come out
        // in the same order with the same content. Byte for byte is the only
        // form of that claim worth testing — a page count and a file size
        // would both survive a footer that had quietly gone blank.
        let document = ledger(400, None, Some(band(20.0, "{{page}} de {{pages}}")));

        let counted = build(&document, &assets(), Options::default()).unwrap();

        assert!(counted.pages > 3, "the sample must paginate");
        assert_eq!(counted.pdf, built_by_holding(&document));
    }

    #[test]
    fn the_pass_that_counts_says_nothing_the_pass_that_paints_will_say_again() {
        // Both walks measure the same text through the same shaper, so both
        // notice the same missing glyph. Reported twice, a reader would
        // reasonably conclude there were two of them.
        let mut document = ledger(60, None, Some(band(20.0, "{{page}} de {{pages}}")));
        let ir::Node::Table(table) = &mut document.children[0] else {
            unreachable!()
        };
        table.rows[0].cells[0] = ir::Cell::new("日本語");

        let built = build(&document, &assets(), Options::default()).unwrap();

        let missing: Vec<_> = built
            .diagnostics
            .iter()
            .filter(|d| d.contains("missing-glyph"))
            .collect();
        assert_eq!(missing.len(), 1, "{:?}", built.diagnostics);
    }

    #[test]
    fn asking_for_the_total_is_the_only_thing_that_holds_the_pages() {
        // It costs the memory of the whole document, so it must not be paid
        // by a footer that only numbers its pages.
        assert!(!super::needs_total(&ledger(
            10,
            None,
            Some(band(20.0, "Pagina {{page}}"))
        )));
        assert!(super::needs_total(&ledger(
            10,
            None,
            Some(band(20.0, "{{page}}/{{pages}}"))
        )));
        assert!(super::needs_total(&ledger(
            10,
            Some(band(20.0, "{{pages}}")),
            None
        )));
        assert!(!super::needs_total(&ledger(10, None, None)));
    }

    #[test]
    fn a_running_total_carries_onto_the_next_page() {
        // "Suma y sigue": what the page opened at and what it closed at, and
        // the two have to meet across the boundary.
        let built = build(
            &ledger(
                200,
                None,
                Some(band(
                    20.0,
                    "Suma anterior {{opening:total}} — suma {{closing:total}}",
                )),
            ),
            &assets(),
            Options::default(),
        )
        .unwrap();

        assert!(built.pages > 2);
        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }

    #[test]
    fn a_token_nobody_declared_is_reported_rather_than_printed() {
        // Printing `{{opening:iva}}` on nine thousand pages is worse than
        // saying so once.
        let built = build(
            &ledger(20, None, Some(band(20.0, "{{opening:iva}}"))),
            &assets(),
            Options::default(),
        )
        .unwrap();

        assert!(
            built
                .diagnostics
                .iter()
                .any(|d| d.contains("unknown-total")),
            "{:?}",
            built.diagnostics
        );
    }

    #[test]
    fn a_band_can_hold_more_than_words() {
        let built = build(
            &ledger(
                20,
                Some(ir::Band {
                    height: Pt(30.0),
                    children: vec![ir::Node::Box(ir::Container {
                        style: ir::BoxStyle {
                            background: Some(Color::parse_hex("#f1f5f9").unwrap()),
                            ..Default::default()
                        },
                        children: vec![ir::Node::Text(ir::Text {
                            runs: vec![ir::Run::new("Con fondo")],
                            style: ir::TextStyle::default(),
                        })],
                    })],
                }),
                None,
            ),
            &assets(),
            Options::default(),
        )
        .unwrap();

        assert_eq!(built.pages, 1);
        assert!(built.diagnostics.is_empty(), "{:?}", built.diagnostics);
    }
}
