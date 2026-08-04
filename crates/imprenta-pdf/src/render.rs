//! Phase C — painting packed pages to PDF.
//!
//! Everything here is per-page and independent, so it parallelises: in the
//! prototype painting was 82% of the run time and 40% of *that* was Flate
//! compression, both of which are embarrassingly parallel. Nothing does that
//! yet — no benchmark asks for it.

use crate::content::{CanvasContent, Content, ImageContent, ImageFormat, LinkTarget, PathOp};
use crate::decoration::{Decoration, fitted_radius};
use crate::pack::Page;
use crate::shape::Face;
use crate::shape::Line;
use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Pt};
use std::collections::HashMap;

/// The page box and its margins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub width: Pt,
    pub height: Pt,
    pub margin: Edges<Pt>,
    /// Room set aside at the top and bottom of every page.
    pub bands: Bands,
}

/// How much of each page a header and a footer take.
///
/// Taken out of the content box rather than out of the margin: a band that
/// ate into the margin could overlap the last line, and finding that out
/// means finding it on paper. Declaring a forty point header means forty
/// points less content per page, which is arithmetic an author can do in
/// their head.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bands {
    pub header: Pt,
    pub footer: Pt,
}

impl Geometry {
    /// A4 portrait with 12 mm margins — a sane default, not a limit.
    pub fn a4() -> Self {
        Self {
            width: Pt::mm(210.0),
            height: Pt::mm(297.0),
            margin: Edges::all(Pt::mm(12.0)),
            bands: Bands::default(),
        }
    }

    /// Height available to content once margins are taken out. This is the
    /// budget the packer is given.
    pub fn content_height(&self) -> Pt {
        self.height - self.margin.vertical() - self.bands.header - self.bands.footer
    }

    /// Where the content box starts, below any header.
    pub fn content_top(&self) -> Pt {
        self.margin.top + self.bands.header
    }

    /// Where a footer's own box starts.
    pub fn footer_top(&self) -> Pt {
        self.height - self.margin.bottom - self.bands.footer
    }
}

/// Knobs that change the bytes without changing the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Compressing content streams roughly halves the file. Turning it off
    /// makes the operators readable, which is how the tests check that a
    /// rectangle was actually drawn rather than merely intended.
    pub compress: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { compress: true }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the font could not be read as an OpenType or TrueType file")]
    UnreadableFont,
    #[error("failed to serialise the PDF: {0}")]
    Serialise(String),
}

/// Paints packed pages to a PDF.
///
/// `lines[i]` is what [`crate::pack::Placement::atom`] `i` refers to — the
/// parallel arrays that [`crate::measure::Measured`] keeps together.
pub fn render(
    pages: &[Page],
    lines: &[Line],
    font: &[u8],
    geometry: Geometry,
) -> Result<Vec<u8>, RenderError> {
    let contents: Vec<Content> = lines.iter().cloned().map(Content::from).collect();
    render_with(pages, &contents, font, geometry, Options::default())
}

/// Paints packed pages using every face a shaper registered.
///
/// Prefer this over [`render_with`] whenever a document sets anything bold or
/// italic: a line records the face it was shaped in, and drawing those glyph
/// ids against the regular outlines produces the wrong letters.
pub fn render_faces(
    pages: &[Page],
    contents: &[Content],
    fonts: &Fonts,
    geometry: Geometry,
    options: Options,
) -> Result<Vec<u8>, RenderError> {
    render_inner(pages, contents, fonts, geometry, options)
}

/// Paints packed pages, with control over serialisation.
pub fn render_with(
    pages: &[Page],
    contents: &[Content],
    font: &[u8],
    geometry: Geometry,
    options: Options,
) -> Result<Vec<u8>, RenderError> {
    if pages.is_empty() {
        return Err(RenderError::Serialise(
            "a PDF must have at least one page".into(),
        ));
    }

    let krilla_font = krilla::text::Font::new(krilla::Data::from(font.to_vec()), 0)
        .ok_or(RenderError::UnreadableFont)?;
    render_inner(
        pages,
        contents,
        &Fonts::single(krilla_font),
        geometry,
        options,
    )
}

fn render_inner(
    pages: &[Page],
    contents: &[Content],
    fonts: &Fonts,
    geometry: Geometry,
    options: Options,
) -> Result<Vec<u8>, RenderError> {
    let mut sink = PageSink::new(geometry, options)?;
    for page in pages {
        sink.paint_page(page, contents, fonts, 0);
    }
    sink.finish()
}

/// Paints pages into one PDF, one at a time.
///
/// Exists so a caller can hand pages over as they are finished and drop the
/// content behind them, rather than assembling the whole document first —
/// see [`crate::compose`]. krilla still keeps each painted page until the
/// end, but that is 15.5 KB apiece against the 405 KB of atoms and shaped
/// lines a caller would otherwise be holding.
pub struct PageSink {
    doc: krilla::Document,
    settings: krilla::page::PageSettings,
    geometry: Geometry,
    images: ImageCache,
    pages: usize,
}

impl PageSink {
    pub fn new(geometry: Geometry, options: Options) -> Result<Self, RenderError> {
        let settings =
            krilla::page::PageSettings::from_wh(geometry.width.get(), geometry.height.get())
                .ok_or_else(|| RenderError::Serialise("page size must be positive".into()))?;
        Ok(Self {
            doc: krilla::Document::new_with(krilla::SerializeSettings {
                compress_content_streams: options.compress,
                ..Default::default()
            }),
            settings,
            geometry,
            images: HashMap::new(),
            pages: 0,
        })
    }

    /// Paints one page, resolving each placement through `lookup`.
    ///
    /// A lookup rather than a slice because a caller that releases content as
    /// it goes still has to answer for a repeated table header whose atom was
    /// dropped four hundred pages ago.
    pub fn paint_page_with<'c>(
        &mut self,
        page: &Page,
        fonts: &Fonts,
        lookup: impl Fn(usize) -> Option<&'c Content>,
        bands: &crate::compose::Painted,
    ) {
        let geometry = self.geometry;
        let available = geometry.width - geometry.margin.horizontal();
        let mut pdf_page = self.doc.start_page_with(self.settings.clone());
        let mut links: Vec<LinkRect> = Vec::new();
        let mut surface = pdf_page.surface();

        let mut draw =
            |content: &Content, top: Pt, images: &mut ImageCache, links: &mut Vec<LinkRect>| {
                paint(
                    &mut surface,
                    content,
                    fonts,
                    images,
                    links,
                    geometry.margin.left,
                    top,
                    available,
                );
            };

        // The bands, in the room the geometry set aside for them. A header
        // sits against the top margin and a footer against the bottom, so
        // neither can drift as a page fills or empties.
        if let Some(header) = &bands.header {
            draw(header, geometry.margin.top, &mut self.images, &mut links);
        }
        if let Some(footer) = &bands.footer {
            draw(footer, geometry.footer_top(), &mut self.images, &mut links);
        }

        // Repeated prefixes first: a table's header sits above the rows that
        // continue under it.
        for continuation in &page.continuations {
            if let Some(content) = lookup(continuation.atom) {
                draw(
                    content,
                    geometry.content_top() + continuation.y,
                    &mut self.images,
                    &mut links,
                );
            }
        }
        for placement in &page.placements {
            if let Some(content) = lookup(placement.atom) {
                draw(
                    content,
                    geometry.content_top() + placement.y,
                    &mut self.images,
                    &mut links,
                );
            }
        }

        surface.finish();
        attach_links(&mut pdf_page, links);
        pdf_page.finish();
        self.pages += 1;
    }

    /// Paints one page from a contiguous slice starting at absolute `base`.
    pub fn paint_page(&mut self, page: &Page, contents: &[Content], fonts: &Fonts, base: usize) {
        self.paint_page_with(
            page,
            fonts,
            |atom| atom.checked_sub(base).and_then(|i| contents.get(i)),
            &Default::default(),
        );
    }

    pub fn pages(&self) -> usize {
        self.pages
    }

    pub fn finish(self) -> Result<Vec<u8>, RenderError> {
        if self.pages == 0 {
            return Err(RenderError::Serialise(
                "a PDF must have at least one page".into(),
            ));
        }
        self.doc
            .finish()
            .map_err(|e| RenderError::Serialise(format!("{e:?}")))
    }
}

fn attach_links(page: &mut krilla::page::Page, links: Vec<LinkRect>) {
    for link in links {
        let LinkTarget::Url(url) = &link.target;
        if let Some(rect) = krilla::geom::Rect::from_xywh(
            link.x.get(),
            link.y.get(),
            link.width.get(),
            link.height.get(),
        ) {
            page.add_annotation(
                krilla::annotation::LinkAnnotation::new(
                    rect,
                    krilla::annotation::Target::Action(
                        krilla::action::LinkAction::new(url.clone()).into(),
                    ),
                )
                .into(),
            );
        }
    }
}

/// Decoded images, keyed on the address of the shared buffer they came from.
type ImageCache = HashMap<*const u8, Option<krilla::image::Image>>;

/// The faces available to the painter, by the face a stretch names.
///
/// A stretch of a line records which face it was shaped against; drawing bold
/// glyph ids with the regular outlines would produce the wrong letters, not
/// merely lighter ones.
#[derive(Clone)]
pub struct Fonts {
    faces: HashMap<Face, krilla::text::Font>,
    fallback: krilla::text::Font,
}

impl Fonts {
    fn single(font: krilla::text::Font) -> Self {
        Self {
            faces: HashMap::new(),
            fallback: font,
        }
    }

    /// Loads every face a shaper registered.
    pub fn from_shaper(shaper: &crate::shape::Shaper) -> Result<Self, RenderError> {
        let mut faces = HashMap::new();
        let mut fallback = None;
        for (face, bytes) in shaper.faces() {
            let font = krilla::text::Font::new(krilla::Data::from(bytes.to_vec()), 0)
                .ok_or(RenderError::UnreadableFont)?;
            if *face == Face::REGULAR || fallback.is_none() {
                fallback = Some(font.clone());
            }
            faces.insert(*face, font);
        }
        Ok(Self {
            faces,
            fallback: fallback.ok_or(RenderError::UnreadableFont)?,
        })
    }

    fn get(&self, face: Face) -> &krilla::text::Font {
        self.faces.get(&face).unwrap_or(&self.fallback)
    }
}

/// A clickable region gathered while painting, attached to the page after.
struct LinkRect {
    target: LinkTarget,
    x: Pt,
    y: Pt,
    width: Pt,
    height: Pt,
}

/// Draws `content` with its top-left corner at `(x, y)` on the page.
///
/// Recursive: a box paints its decoration first, then its children over it.
/// Painting a child before the next sibling's background is exactly what
/// makes text inside a row survive.
#[allow(clippy::too_many_arguments)]
fn paint(
    surface: &mut krilla::surface::Surface,
    content: &Content,
    fonts: &Fonts,
    images: &mut ImageCache,
    links: &mut Vec<LinkRect>,
    x: Pt,
    y: Pt,
    available: Pt,
) {
    match content {
        Content::Text(line) => paint_line(surface, line, fonts, x, y),
        Content::Box(b) => {
            // A box with no width of its own fills what it is offered. Two
            // panels side by side must each declare one, or both are painted
            // across the whole content box and overlap.
            let width = b.width.unwrap_or(available);
            paint_box(surface, &b.decoration, x, y, width, b.height());

            let inner = width - b.padding.horizontal();
            for child in &b.children {
                paint(
                    surface,
                    &child.content,
                    fonts,
                    images,
                    links,
                    x + child.x,
                    y + child.y,
                    inner,
                );
            }
        }
        Content::Image(image) => paint_image(surface, image, images, x, y),
        Content::Canvas(canvas) => paint_canvas(surface, canvas, x, y),
        Content::Link(link) => {
            // The clickable region is an annotation on the page, not part of
            // the content stream, so it is collected here and attached once
            // the page is finished.
            let width = link.width.unwrap_or(available);
            links.push(LinkRect {
                target: link.target.clone(),
                x,
                y,
                width,
                height: link.content.height(),
            });
            paint(surface, &link.content, fonts, images, links, x, y, width);
        }
        Content::Empty => {}
    }
}

/// Draws a canvas's path at `(x, y)`.
fn paint_canvas(surface: &mut krilla::surface::Surface, canvas: &CanvasContent, x: Pt, y: Pt) {
    if canvas.ops.is_empty() || (canvas.fill.is_none() && canvas.stroke.is_none()) {
        return;
    }

    let (ox, oy) = (x.get(), y.get());
    let mut path = krilla::geom::PathBuilder::new();
    for op in &canvas.ops {
        match *op {
            PathOp::MoveTo(px, py) => path.move_to(ox + px.get(), oy + py.get()),
            PathOp::LineTo(px, py) => path.line_to(ox + px.get(), oy + py.get()),
            PathOp::CurveTo(c1x, c1y, c2x, c2y, px, py) => path.cubic_to(
                ox + c1x.get(),
                oy + c1y.get(),
                ox + c2x.get(),
                oy + c2y.get(),
                ox + px.get(),
                oy + py.get(),
            ),
            PathOp::Close => path.close(),
        }
    }

    let Some(path) = path.finish() else { return };
    surface.set_fill(canvas.fill.map(fill));
    surface.set_stroke(canvas.stroke.map(|(colour, width)| krilla::paint::Stroke {
        paint: rgb(colour).into(),
        width: width.get(),
        ..Default::default()
    }));
    surface.draw_path(&path);
    surface.set_fill(None);
    surface.set_stroke(None);
}

/// How far a cubic control point sits from the corner to draw a quarter
/// circle: four thirds of the tangent of an eighth turn.
///
/// The classic approximation. It is off by about a fifty-thousandth of the
/// radius at its worst, which at a four point corner is a hundredth of a
/// printer dot.
const KAPPA: f32 = 0.552_284_8;

/// The box's outline, rounded if it has a radius.
///
/// One path for both the fill and the stroke, so a background and the border
/// round it cannot drift apart by a fraction of a point.
fn outline(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
) -> Option<krilla::geom::Path> {
    let mut path = krilla::geom::PathBuilder::new();

    if radius <= 0.0 {
        path.push_rect(krilla::geom::Rect::from_ltrb(left, top, right, bottom)?);
        return path.finish();
    }

    let c = radius * KAPPA;
    path.move_to(left + radius, top);
    path.line_to(right - radius, top);
    path.cubic_to(
        right - radius + c,
        top,
        right,
        top + radius - c,
        right,
        top + radius,
    );
    path.line_to(right, bottom - radius);
    path.cubic_to(
        right,
        bottom - radius + c,
        right - radius + c,
        bottom,
        right - radius,
        bottom,
    );
    path.line_to(left + radius, bottom);
    path.cubic_to(
        left + radius - c,
        bottom,
        left,
        bottom - radius + c,
        left,
        bottom - radius,
    );
    path.line_to(left, top + radius);
    path.cubic_to(
        left,
        top + radius - c,
        left + radius - c,
        top,
        left + radius,
        top,
    );
    path.close();
    path.finish()
}

/// Fills and rules the box occupying `(x, y)` to `(x + width, y + height)`.
fn paint_box(
    surface: &mut krilla::surface::Surface,
    decoration: &Decoration,
    x: Pt,
    y: Pt,
    width: Pt,
    height: Pt,
) {
    if decoration.is_empty() {
        return;
    }

    let left = x.get();
    let right = (x + width).get();
    let top = y.get();
    let bottom = top + height.get();

    let radius = fitted_radius(decoration.radius, width, height).get();

    if let Some(colour) = decoration.background
        && let Some(path) = outline(left, top, right, bottom, radius)
    {
        surface.set_stroke(None);
        surface.set_fill(Some(fill(colour)));
        surface.draw_path(&path);
    }

    // A border that runs all the way round in one width and colour can
    // follow the rounded outline as a single stroke. Anything else is drawn
    // side by side, straight: where two sides differ the corner between them
    // belongs to neither, and an arc through it would be invented.
    if radius > 0.0
        && let Some(side) = decoration.uniform_border()
        && let Some(path) = outline(left, top, right, bottom, radius)
    {
        surface.set_fill(None);
        surface.set_stroke(Some(krilla::paint::Stroke {
            paint: rgb(side.color).into(),
            width: side.width.get(),
            ..Default::default()
        }));
        surface.draw_path(&path);
        surface.set_stroke(None);
        surface.set_fill(None);
        return;
    }

    // Each side is its own subpath: a rule under a row is one line, not a
    // rectangle with three invisible sides.
    for (side, from, to) in [
        (decoration.border.top, (left, top), (right, top)),
        (decoration.border.right, (right, top), (right, bottom)),
        (decoration.border.bottom, (left, bottom), (right, bottom)),
        (decoration.border.left, (left, top), (left, bottom)),
    ] {
        let Some(side) = side else { continue };
        let mut path = krilla::geom::PathBuilder::new();
        path.move_to(from.0, from.1);
        path.line_to(to.0, to.1);
        if let Some(path) = path.finish() {
            surface.set_fill(None);
            surface.set_stroke(Some(krilla::paint::Stroke {
                paint: rgb(side.color).into(),
                width: side.width.get(),
                ..Default::default()
            }));
            surface.draw_path(&path);
        }
    }

    surface.set_stroke(None);
    surface.set_fill(None);
}

/// Draws `image` with its top-left corner at `(x, y)`.
///
/// A decode failure is silent here: the painter has no way to report, and a
/// missing logo must not take a 9,000-page render down with it. The measure
/// phase is where an unreadable image becomes a build diagnostic.
fn paint_image(
    surface: &mut krilla::surface::Surface,
    image: &ImageContent,
    images: &mut ImageCache,
    x: Pt,
    y: Pt,
) {
    let key = image.data.as_ptr();
    let decoded = images
        .entry(key)
        .or_insert_with(|| {
            let data = krilla::Data::from(image.data.to_vec());
            match image.format {
                ImageFormat::Png => krilla::image::Image::from_png(data, false),
                ImageFormat::Jpeg => krilla::image::Image::from_jpeg(data, false),
            }
            .ok()
        })
        .clone();

    let (Some(decoded), Some(size)) = (
        decoded,
        krilla::geom::Size::from_wh(image.width.get(), image.height.get()),
    ) else {
        return;
    };

    surface.push_transform(&krilla::geom::Transform::from_translate(x.get(), y.get()));
    surface.draw_image(decoded, size);
    surface.pop();
}

fn rgb(colour: Color) -> krilla::color::rgb::Color {
    krilla::color::rgb::Color::new(colour.r, colour.g, colour.b)
}

fn fill(colour: Color) -> krilla::paint::Fill {
    krilla::paint::Fill {
        paint: rgb(colour).into(),
        opacity: krilla::num::NormalizedF32::new(colour.a as f32 / 255.0)
            .unwrap_or(krilla::num::NormalizedF32::ONE),
        ..Default::default()
    }
}

/// Draws one line's glyphs, stretch by stretch, at `(x, y)`.
///
/// Each stretch is drawn in its own face and colour, so a sentence that turns
/// bold or changes ink half-way through comes out that way.
fn paint_line(surface: &mut krilla::surface::Surface, line: &Line, fonts: &Fonts, x: Pt, y: Pt) {
    for segment in &line.segments {
        if segment.glyphs.is_empty() {
            continue;
        }

        let glyphs: Vec<krilla::text::KrillaGlyph> = segment
            .glyphs
            .iter()
            .map(|g| krilla::text::KrillaGlyph {
                glyph_id: krilla::text::GlyphId::new(g.id),
                // krilla wants advances normalised to the em; a line's are
                // absolute, so they divide back out by the size it was set at.
                x_advance: g.x_advance / line.size.get(),
                x_offset: 0.0,
                y_offset: 0.0,
                y_advance: 0.0,
                // The source range is what krilla turns into the ToUnicode
                // map. Without it the page looks right and the text cannot
                // be copied.
                text_range: g.text_range.start as usize..g.text_range.end as usize,
                location: None,
            })
            .collect();

        // Set explicitly every time: paint state is global to the content
        // stream, so a fill left over from a box background — or from the
        // previous stretch — would otherwise colour these glyphs.
        surface.set_stroke(None);
        surface.set_fill(Some(fill(segment.color)));

        surface.draw_glyphs(
            krilla::geom::Point::from_xy((x + segment.x).get(), (y + line.baseline).get()),
            &glyphs,
            fonts.get(segment.face).clone(),
            &line.text,
            line.size.get(),
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::{TextStyle, measure_text};
    use crate::pack::{Flow, pack};
    use crate::shape::Shaper;

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
    const PROSE: &str = "Prestación de servicios profesionales periodo 3 para el \
                         cliente comercial, según contrato marco vigente";

    /// Measures and packs `text`, then paints it — the whole pipeline.
    ///
    /// The column is deliberately narrow so the sample breaks into enough
    /// lines that the default 2/2 widow and orphan limits leave somewhere to
    /// break: a paragraph of three lines with those limits is one
    /// unbreakable run, which is correct but makes for a poor fixture.
    fn pipeline(text: &str, budget: Pt) -> (Vec<u8>, usize) {
        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let m = measure_text(&mut shaper, text, TextStyle::new(Pt(9.0)), Pt(70.0));
        let pages = pack(&Flow::new(&m.atoms), budget);
        let pdf = render(&pages, &m.lines, ROBOTO, Geometry::a4()).expect("render");
        (pdf, pages.len())
    }

    fn count(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }

    #[test]
    fn the_output_is_a_pdf() {
        let (pdf, _) = pipeline(PROSE, Pt(400.0));

        assert_eq!(&pdf[..5], b"%PDF-", "no PDF header");
        assert!(pdf.ends_with(b"%%EOF\n") || pdf.ends_with(b"%%EOF"));
    }

    #[test]
    fn one_pdf_page_is_written_per_packed_page() {
        let (pdf, packed) = pipeline(PROSE, Pt(30.0));

        assert!(packed > 1, "the sample must span pages");
        assert_eq!(count(&pdf, b"/Type /Page\n"), packed);
    }

    #[test]
    fn the_font_is_embedded_rather_than_merely_named() {
        // Referencing a font by name is what makes a PDF render differently
        // on another machine, and it is disallowed outright by PDF/A.
        let (pdf, _) = pipeline(PROSE, Pt(400.0));

        assert!(count(&pdf, b"FontFile") > 0, "no embedded font programme");
    }

    #[test]
    fn a_tounicode_map_is_written_so_the_text_can_be_extracted() {
        // The defect the prototype shipped: the page looks perfect and the
        // text cannot be copied, searched or read by a screen reader.
        let (pdf, _) = pipeline(PROSE, Pt(400.0));

        assert!(count(&pdf, b"/ToUnicode") > 0, "no ToUnicode map");
        assert!(count(&pdf, b"beginbfchar") > 0, "the map is empty");
    }

    #[test]
    fn the_tounicode_map_covers_exactly_the_characters_that_were_drawn() {
        // Not just "a map exists" — the right map. Krilla subsets the font
        // and renumbers the glyphs, so a PDF CID is *not* the shaper's glyph
        // id (Roboto's 'P' is 51; in the subset it is 1). What must hold is
        // that the map resolves to exactly the characters the page contains,
        // multi-byte ones included.
        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let text = "Prestación";
        let m = measure_text(&mut shaper, text, TextStyle::new(Pt(9.0)), Pt(400.0));
        let pages = pack(&Flow::new(&m.atoms), Pt(400.0));
        let pdf = render(&pages, &m.lines, ROBOTO, Geometry::a4()).expect("render");

        let mut mapped: Vec<String> = tounicode_entries(&pdf)
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        mapped.sort();
        mapped.dedup();

        let mut expected: Vec<String> = text.chars().map(String::from).collect();
        expected.sort();
        expected.dedup();

        assert_eq!(mapped, expected, "the map does not match the page's text");
        assert!(mapped.iter().any(|s| s == "ó"), "multi-byte char lost");
    }

    #[test]
    fn rendering_the_same_document_twice_yields_identical_bytes() {
        // Determinism is what makes golden-PDF diffing in CI possible.
        let (first, _) = pipeline(PROSE, Pt(120.0));
        let (second, _) = pipeline(PROSE, Pt(120.0));

        assert_eq!(first, second);
    }

    #[test]
    fn a_blank_page_produced_by_a_parity_break_still_reaches_the_pdf() {
        use crate::atom::{Atom, Break};

        let atoms = vec![
            Atom::new(Pt(10.0)),
            Atom::new(Pt(10.0)).break_before(Break::Odd),
        ];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        assert_eq!(pages.len(), 3, "middle page is the blank one");

        let pdf = render(&pages, &[], ROBOTO, Geometry::a4()).expect("render");

        assert_eq!(count(&pdf, b"/Type /Page\n"), 3);
    }

    #[test]
    fn a_document_with_no_pages_is_rejected_rather_than_producing_a_broken_file() {
        // A PDF whose page tree is empty is malformed. Better a named error
        // than a file that some viewers open and others refuse.
        let err = render(&[], &[], ROBOTO, Geometry::a4());

        assert!(err.is_err());
    }

    #[test]
    fn an_unreadable_font_is_reported_not_panicked_on() {
        let pages = pack(&Flow::new(&[crate::atom::Atom::new(Pt(10.0))]), Pt(400.0));

        let err = render(&pages, &[], b"not a font at all", Geometry::a4());

        assert!(matches!(err, Err(RenderError::UnreadableFont)));
    }

    // ── boxes ───────────────────────────────────────────────────────────
    // Until now the painter only drew glyphs. A document also needs fills and
    // rules — a shaded total row, a line under a heading, a ruled table.

    use crate::atom::Atom;
    use crate::content::BoxContent;
    use crate::decoration::{BorderSide, Decoration};
    use imprenta_core::color::Color;

    /// The page's drawing operators, uncompressed.
    ///
    /// Assertions run against these rather than against the whole file, and
    /// against coordinates rather than against krilla's choice of operator:
    /// it spells a rectangle as an explicit `m`/`l`/`h` path, and that is its
    /// business. Where the box lands is ours.
    fn operators(pdf: &[u8]) -> String {
        let text = String::from_utf8_lossy(pdf);
        text.split("stream\n")
            .find(|block| block.starts_with("q\n"))
            .and_then(|block| block.split("endstream").next())
            .unwrap_or_default()
            .to_string()
    }

    /// Packs one atom of `height` carrying `content`, and paints it
    /// uncompressed so the operators can be read back.
    fn paint_one(height: f32, content: Content) -> String {
        let atoms = [Atom::new(Pt(content.height().get().max(height)))];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let pdf = render_with(
            &pages,
            &[content],
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .expect("render");
        operators(&pdf)
    }

    /// The content box edges of an A4 page with 12 mm margins.
    const LEFT: &str = "34.015747";
    const RIGHT: &str = "561.2599";

    /// A box of the given height, made out of padding — an empty box is as
    /// tall as its padding, which is the only way to size one until content
    /// or an explicit height gives it a size.
    fn sized(height: f32, decoration: Decoration) -> Content {
        Content::Box(
            BoxContent::new(decoration).with_padding(Edges::symmetric(Pt(height / 2.0), Pt(0.0))),
        )
    }

    fn navy() -> Color {
        Color::parse_hex("#1F4E79").unwrap()
    }

    #[test]
    fn a_box_with_a_background_paints_a_filled_rectangle() {
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    ..Default::default()
                },
            ),
        );

        assert!(pdf.contains("\nf\n"), "the path was never filled:\n{pdf}");
        assert_eq!(pdf.matches(" m\n").count(), 1, "one box, one subpath");
        assert!(pdf.contains("\nh\n"), "the rectangle was not closed");
    }

    #[test]
    fn a_background_is_painted_in_the_colour_it_was_given() {
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    ..Default::default()
                },
            ),
        );

        // #1F4E79 as PDF device RGB, to three decimals.
        assert!(
            pdf.contains("0.121") && pdf.contains("0.305") && pdf.contains("0.474"),
            "navy is missing from the operators"
        );
    }

    #[test]
    fn a_box_with_no_decoration_paints_nothing() {
        let bare = paint_one(20.0, Content::Box(BoxContent::new(Decoration::default())));
        let empty = paint_one(20.0, Content::Empty);

        assert!(!bare.contains(" m\n"), "an undecorated box drew a path");
        assert_eq!(bare.len(), empty.len());
    }

    #[test]
    fn a_bottom_border_is_stroked_and_the_other_sides_are_not() {
        // The commonest rule in a document: a line under a row, nothing else.
        let rule = BorderSide {
            width: Pt(1.0),
            color: navy(),
        };
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    border: Edges {
                        bottom: Some(rule),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        );

        assert!(
            pdf.contains("\nS\n"),
            "the border was never stroked:\n{pdf}"
        );
        assert!(!pdf.contains("\nf\n"), "a border-only box must not fill");
        assert_eq!(
            pdf.matches(" m\n").count(),
            1,
            "one side asked for a rule, so exactly one subpath should start"
        );
        // The rule belongs at the bottom of the 20pt box, not the top.
        assert!(
            pdf.contains("54.015747"),
            "the rule is not at the bottom edge"
        );
        assert!(
            !pdf.contains(&format!("{LEFT} {LEFT} m")),
            "the rule was drawn along the top edge"
        );
    }

    #[test]
    fn each_side_of_a_border_can_be_drawn_independently() {
        let rule = BorderSide {
            width: Pt(1.0),
            color: navy(),
        };
        let all = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: None,
                    border: Edges::all(Some(rule)),
                    ..Default::default()
                },
            ),
        );

        assert_eq!(all.matches(" m\n").count(), 4, "four sides, four subpaths");
    }

    #[test]
    fn a_box_spans_the_full_content_width_at_its_own_height() {
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    ..Default::default()
                },
            ),
        );

        // Absolute edges, not a width: the box must start at the left margin
        // and stop at the right one.
        assert!(
            pdf.contains(LEFT),
            "does not start at the left margin:\n{pdf}"
        );
        assert!(
            pdf.contains(RIGHT),
            "does not stop at the right margin:\n{pdf}"
        );
        // 12mm margin + 20pt tall.
        assert!(pdf.contains("54.015747"), "the box is not 20pt tall");
    }

    #[test]
    fn text_and_boxes_coexist_on_one_page() {
        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let line = shaper
            .break_lines("Total asiento", Pt(9.0), Pt(400.0))
            .remove(0);

        let atoms = [Atom::new(Pt(20.0)), Atom::new(line.height)];
        let contents = [
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    ..Default::default()
                },
            ),
            Content::Text(line),
        ];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let pdf = render_with(
            &pages,
            &contents,
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .expect("render");
        let pdf = operators(&pdf);

        assert!(pdf.contains("\nf\n"), "the box vanished:\n{pdf}");
        assert!(
            pdf.contains("Tj") || pdf.contains("TJ"),
            "the text vanished:\n{pdf}"
        );
    }

    #[test]
    fn text_is_painted_in_the_colour_its_line_carries() {
        // Without this, white text on a navy header renders in the default
        // ink and is unreadable — visible only by looking at the page.
        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let white = Color::parse_hex("#FFFFFF").unwrap();
        let line = shaper
            .break_lines("Cuenta", Pt(8.0), Pt(400.0))
            .remove(0)
            .with_color(white);

        let boxed = BoxContent::new(Decoration {
            background: Some(navy()),
            ..Default::default()
        })
        .with_padding(Edges::all(Pt(3.0)))
        .stack(Content::Text(line));

        let atoms = [Atom::new(boxed.height())];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let pdf = render_with(
            &pages,
            &[Content::Box(boxed)],
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .unwrap();
        let ops = operators(&pdf);

        assert!(
            ops.contains("1 1 1 rg"),
            "the text was not set to white:\n{ops}"
        );
    }

    #[test]
    fn text_defaults_to_black_ink() {
        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let line = shaper.break_lines("Total", Pt(8.0), Pt(400.0)).remove(0);
        let atoms = [Atom::new(line.height)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let pdf = render_with(
            &pages,
            &[Content::Text(line)],
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .unwrap();

        assert!(
            operators(&pdf).contains("0 0 0 rg"),
            "black was not set explicitly"
        );
    }

    #[test]
    fn a_box_fill_does_not_bleed_into_the_text_drawn_after_it() {
        // Paint state is global to the content stream; a fill left set by the
        // background would silently colour the glyphs that follow.
        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let line = shaper.break_lines("Total", Pt(8.0), Pt(400.0)).remove(0);

        let boxed = BoxContent::new(Decoration {
            background: Some(navy()),
            ..Default::default()
        })
        .with_padding(Edges::all(Pt(3.0)))
        .stack(Content::Text(line));

        let atoms = [Atom::new(boxed.height())];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let pdf = render_with(
            &pages,
            &[Content::Box(boxed)],
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .unwrap();
        let ops = operators(&pdf);

        let navy_at = ops.find("0.12156863").expect("no navy fill");
        let black_at = ops.find("0 0 0 rg").expect("text ink was never set");
        assert!(black_at > navy_at, "the text inherited the box fill");
    }

    #[test]
    fn an_image_is_embedded_in_the_pdf() {
        use crate::content::{ImageContent, ImageFormat};
        const LOGO: &[u8] = include_bytes!("../tests/images/logo.png");

        let image = ImageContent::scaled_to_width(LOGO, ImageFormat::Png, (240, 80), Pt(120.0));
        let atoms = [Atom::new(image.height)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let pdf = render_with(
            &pages,
            &[Content::Image(image)],
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .unwrap();

        assert!(count(&pdf, b"/Subtype /Image") > 0, "no image object");
        assert!(count(&pdf, b"/Width 240") > 0, "wrong pixel width");
        assert!(
            operators(&pdf).contains(" Do\n"),
            "the image was never drawn"
        );
    }

    #[test]
    fn an_unreadable_image_is_skipped_rather_than_bringing_the_render_down() {
        use crate::content::{ImageContent, ImageFormat};

        let broken = ImageContent {
            data: std::sync::Arc::from(&b"not a png"[..]),
            format: ImageFormat::Png,
            width: Pt(100.0),
            height: Pt(30.0),
        };
        let atoms = [Atom::new(Pt(30.0))];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let pdf = render_with(
            &pages,
            &[Content::Image(broken)],
            ROBOTO,
            Geometry::a4(),
            Options::default(),
        );

        assert!(pdf.is_ok(), "a bad logo took the whole document down");
    }

    #[test]
    fn the_same_image_used_many_times_is_embedded_once() {
        // A letterhead on every page of a long report.
        use crate::content::{ImageContent, ImageFormat};
        const LOGO: &[u8] = include_bytes!("../tests/images/logo.png");

        let image = ImageContent::scaled_to_width(LOGO, ImageFormat::Png, (240, 80), Pt(120.0));

        // Compared against a single use rather than against a fixed number:
        // an RGBA png costs two objects, the colour data and its alpha mask,
        // and that is krilla's business. What matters is that twenty uses
        // cost no more than one.
        let render_n = |n: usize| {
            let atoms: Vec<Atom> = (0..n).map(|_| Atom::new(image.height)).collect();
            let contents: Vec<Content> = (0..n).map(|_| Content::Image(image.clone())).collect();
            let pages = pack(&Flow::new(&atoms), Pt(200.0));
            let pdf = render_with(
                &pages,
                &contents,
                ROBOTO,
                Geometry::a4(),
                Options::default(),
            )
            .unwrap();
            count(&pdf, b"/Subtype /Image")
        };

        assert!(render_n(1) > 0, "the logo was not embedded at all");
        assert_eq!(
            render_n(20),
            render_n(1),
            "the logo was embedded again for every use"
        );
    }

    #[test]
    fn a_box_with_its_own_width_does_not_span_the_page() {
        use crate::content::BoxContent;

        let panel = BoxContent::new(Decoration {
            background: Some(navy()),
            ..Default::default()
        })
        .with_width(Pt(200.0))
        .with_padding(Edges::all(Pt(5.0)));

        let atoms = [Atom::new(panel.height())];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let pdf = render_with(
            &pages,
            &[Content::Box(panel)],
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .unwrap();
        let ops = operators(&pdf);

        // Left margin 34.02 + 200 = 234.02, not the 561.26 right margin.
        assert!(
            ops.contains("234.01575"),
            "the panel is not 200pt wide:\n{ops}"
        );
        assert!(!ops.contains(RIGHT), "the panel spanned the whole page");
    }

    #[test]
    fn two_panels_side_by_side_do_not_overlap() {
        use crate::content::BoxContent;

        let panel = |w: f32| {
            BoxContent::new(Decoration {
                background: Some(navy()),
                ..Default::default()
            })
            .with_width(Pt(w))
            .with_padding(Edges::all(Pt(5.0)))
        };
        let row = BoxContent::default()
            .place(Pt(0.0), Content::Box(panel(240.0)))
            .place(Pt(260.0), Content::Box(panel(240.0)));

        let atoms = [Atom::new(row.height())];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let pdf = render_with(
            &pages,
            &[Content::Box(row)],
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .unwrap();
        let ops = operators(&pdf);

        // First panel 34.02..274.02, second 294.02..534.02 — no overlap.
        assert!(ops.contains("274.01575"), "first panel is the wrong width");
        assert!(
            ops.contains("294.01575"),
            "second panel starts in the wrong place"
        );
    }

    #[test]
    fn a_repeated_header_is_actually_painted_on_the_continuation_page() {
        // The packer recorded continuations long before the painter drew
        // them, and a table crossing a page simply lost its header — visible
        // only by looking at page two.
        use crate::content::BoxContent;
        use crate::pack::{Group, Repeat};

        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let header_line = shaper.break_lines("CABECERA", Pt(9.0), Pt(400.0)).remove(0);
        let header = BoxContent::default().stack(Content::Text(header_line));
        let header_height = header.height();

        let mut atoms = vec![Atom::new(header_height)];
        let mut contents = vec![Content::Box(header)];
        for _ in 0..40 {
            let line = shaper.break_lines("fila", Pt(9.0), Pt(400.0)).remove(0);
            atoms.push(Atom::new(line.height));
            contents.push(Content::Text(line));
        }

        let groups = [Group {
            atoms: 0..atoms.len(),
            repeat_prefix: Some(Repeat {
                atom: 0,
                height: header_height,
            }),
        }];
        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(120.0));
        assert!(pages.len() > 2, "the table must span several pages");

        let per_page: Vec<usize> = pages
            .iter()
            .map(|page| {
                render_with(
                    std::slice::from_ref(page),
                    &contents,
                    ROBOTO,
                    Geometry::a4(),
                    Options { compress: false },
                )
                .map(|pdf| {
                    operators(&pdf).matches("Tj").count() + operators(&pdf).matches("TJ").count()
                })
                .unwrap_or(0)
            })
            .collect();

        // Every continuation page draws one more text run than its rows,
        // because the header comes back with it.
        for (i, page) in pages.iter().enumerate().skip(1) {
            assert_eq!(
                per_page[i],
                page.placements.len() + 1,
                "page {} drew {} runs for {} rows — the header is missing",
                i + 1,
                per_page[i],
                page.placements.len()
            );
        }
    }

    #[test]
    fn a_document_that_mixes_faces_embeds_each_one() {
        // Two faces shaped, two font programmes embedded. One would mean the
        // bold glyph ids were drawn against the regular outlines — different
        // letters, not merely a lighter weight.
        use crate::shape::Face;
        const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");

        let mut shaper = Shaper::with_faces([
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, BOLD.to_vec()),
        ]);
        let regular = shaper
            .break_lines_in("importe", Pt(9.0), Pt(400.0), Face::REGULAR)
            .remove(0);
        let bold = shaper
            .break_lines_in("TOTAL", Pt(9.0), Pt(400.0), Face::BOLD)
            .remove(0);

        let atoms = [Atom::new(regular.height), Atom::new(bold.height)];
        let contents = [Content::Text(regular), Content::Text(bold)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let fonts = Fonts::from_shaper(&shaper).unwrap();

        let pdf = render_faces(
            &pages,
            &contents,
            &fonts,
            Geometry::a4(),
            Options::default(),
        )
        .unwrap();

        assert_eq!(count(&pdf, b"FontFile"), 2, "both faces should be embedded");
    }

    #[test]
    fn a_document_in_one_face_embeds_only_that_one() {
        use crate::shape::Face;
        const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");

        let mut shaper = Shaper::with_faces([
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, BOLD.to_vec()),
        ]);
        let line = shaper
            .break_lines("solo regular", Pt(9.0), Pt(400.0))
            .remove(0);

        let atoms = [Atom::new(line.height)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));
        let fonts = Fonts::from_shaper(&shaper).unwrap();

        let pdf = render_faces(
            &pages,
            &[Content::Text(line)],
            &fonts,
            Geometry::a4(),
            Options::default(),
        )
        .unwrap();

        assert_eq!(
            count(&pdf, b"FontFile"),
            1,
            "an unused face must not be embedded"
        );
    }

    #[test]
    fn a_canvas_draws_the_path_it_was_given() {
        use crate::content::CanvasContent;

        let canvas = CanvasContent::new(Pt(100.0), Pt(40.0))
            .rect(Pt(0.0), Pt(0.0), Pt(50.0), Pt(20.0))
            .filled(navy());
        let atoms = [Atom::new(canvas.height)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let ops = operators(
            &render_with(
                &pages,
                &[Content::Canvas(canvas)],
                ROBOTO,
                Geometry::a4(),
                Options { compress: false },
            )
            .unwrap(),
        );

        assert_eq!(ops.matches(" m\n").count(), 1, "one subpath");
        assert_eq!(ops.matches(" l\n").count(), 3, "three lines close a rect");
        assert!(ops.contains("\nf\n"), "the shape was never filled");
    }

    #[test]
    fn a_canvas_is_positioned_where_its_atom_landed() {
        use crate::content::CanvasContent;

        let canvas = CanvasContent::new(Pt(100.0), Pt(40.0))
            .move_to(Pt(0.0), Pt(0.0))
            .line_to(Pt(10.0), Pt(0.0))
            .stroked(navy(), Pt(1.0));
        let atoms = [Atom::new(Pt(40.0)), Atom::new(canvas.height)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let ops = operators(
            &render_with(
                &pages,
                &[Content::Empty, Content::Canvas(canvas)],
                ROBOTO,
                Geometry::a4(),
                Options { compress: false },
            )
            .unwrap(),
        );

        // 12mm top margin + a 40pt spacer above it.
        // krilla trims trailing digits, so the prefix is what can be asserted.
        assert!(
            ops.contains("74.0157"),
            "the canvas ignored its position:\n{ops}"
        );
    }

    #[test]
    fn a_canvas_with_no_paint_draws_nothing() {
        use crate::content::CanvasContent;

        let canvas =
            CanvasContent::new(Pt(100.0), Pt(40.0)).rect(Pt(0.0), Pt(0.0), Pt(50.0), Pt(20.0));
        let atoms = [Atom::new(canvas.height)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let ops = operators(
            &render_with(
                &pages,
                &[Content::Canvas(canvas)],
                ROBOTO,
                Geometry::a4(),
                Options { compress: false },
            )
            .unwrap(),
        );

        assert!(!ops.contains(" m\n"), "an unpainted path was drawn");
    }

    #[test]
    fn a_link_becomes_a_clickable_annotation() {
        use crate::content::LinkContent;

        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let line = shaper
            .break_lines("imprenta.dev", Pt(9.0), Pt(400.0))
            .remove(0);
        let link = LinkContent::url("https://imprenta.dev", Content::Text(line));
        let atoms = [Atom::new(link.content.height())];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let pdf = render_with(
            &pages,
            &[Content::Link(Box::new(link))],
            ROBOTO,
            Geometry::a4(),
            Options::default(),
        )
        .unwrap();

        assert!(count(&pdf, b"/Subtype /Link") > 0, "no link annotation");
        assert!(
            count(&pdf, b"https://imprenta.dev") > 0,
            "the target was lost"
        );
    }

    #[test]
    fn a_link_covers_the_content_it_wraps_and_no_more() {
        use crate::content::LinkContent;

        let mut shaper = Shaper::new(ROBOTO.to_vec());
        let line = shaper.break_lines("pulsa", Pt(9.0), Pt(400.0)).remove(0);
        let height = line.height;
        let link =
            LinkContent::url("https://example.org", Content::Text(line)).with_width(Pt(80.0));
        let atoms = [Atom::new(height)];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let pdf = String::from_utf8_lossy(
            &render_with(
                &pages,
                &[Content::Link(Box::new(link))],
                ROBOTO,
                Geometry::a4(),
                Options::default(),
            )
            .unwrap(),
        )
        .into_owned();

        // 34.02 left margin, 80pt wide.
        assert!(
            pdf.contains("114.01575"),
            "the hit area is the wrong width:\n{pdf}"
        );
    }

    #[test]
    fn text_without_a_link_produces_no_annotation() {
        let (pdf, _) = pipeline(PROSE, Pt(400.0));

        assert_eq!(count(&pdf, b"/Subtype /Link"), 0);
    }

    #[test]
    fn compression_changes_the_bytes_but_not_the_page() {
        let atoms = [Atom::new(Pt(20.0))];
        let contents = [sized(
            20.0,
            Decoration {
                background: Some(navy()),
                ..Default::default()
            },
        )];
        let pages = pack(&Flow::new(&atoms), Pt(400.0));

        let small = render_with(
            &pages,
            &contents,
            ROBOTO,
            Geometry::a4(),
            Options { compress: true },
        )
        .unwrap();
        let plain = render_with(
            &pages,
            &contents,
            ROBOTO,
            Geometry::a4(),
            Options { compress: false },
        )
        .unwrap();

        assert!(small.len() < plain.len(), "compression did nothing");
        assert_eq!(
            count(&small, b"/Type /Page\n"),
            count(&plain, b"/Type /Page\n")
        );
    }

    // ── helpers ─────────────────────────────────────────────────────────

    /// Pulls `(glyph id, text)` pairs out of the PDF's ToUnicode CMap.
    ///
    /// Hand-rolled rather than pulling in a PDF reader: krilla writes the
    /// CMap uncompressed, the format is a handful of hex pairs, and a test
    /// that parses the bytes we actually emit is a stronger check than one
    /// mediated by another library's tolerance for malformed input.
    fn tounicode_entries(pdf: &[u8]) -> Vec<(u32, String)> {
        let text = String::from_utf8_lossy(pdf);
        let mut out = Vec::new();

        for block in text.split("beginbfchar").skip(1) {
            let Some(block) = block.split("endbfchar").next() else {
                continue;
            };
            for line in block.lines() {
                let hex: Vec<&str> = line
                    .split(['<', '>'])
                    .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit()))
                    .collect();
                if hex.len() != 2 {
                    continue;
                }
                let Ok(cid) = u32::from_str_radix(hex[0], 16) else {
                    continue;
                };
                let units: Vec<u16> = hex[1]
                    .as_bytes()
                    .chunks(4)
                    .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
                    .collect();
                if let Ok(s) = String::from_utf16(&units) {
                    out.push((cid, s));
                }
            }
        }
        out
    }

    // ── rounded corners ─────────────────────────────────────────────────

    /// How many curve operators the content stream holds.
    fn curves(pdf: &str) -> usize {
        pdf.matches(" c\n").count()
    }

    #[test]
    fn a_square_box_is_drawn_without_a_single_curve() {
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    ..Default::default()
                },
            ),
        );

        assert_eq!(curves(&pdf), 0, "a square box curved:\n{pdf}");
    }

    #[test]
    fn a_rounded_background_is_drawn_with_a_curve_at_each_corner() {
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    radius: Pt(4.0),
                    ..Default::default()
                },
            ),
        );

        assert_eq!(curves(&pdf), 4, "expected four corners:\n{pdf}");
        assert!(pdf.contains("\nf\n"), "the rounded path was never filled");
        assert!(pdf.contains("\nh\n"), "the outline was not closed");
    }

    #[test]
    fn a_rounded_box_stays_inside_the_space_it_was_given() {
        // Rounding takes ink away from the corners; it must not add any
        // outside, or a panel would overlap whatever sits beside it.
        let square = coordinates(&paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    ..Default::default()
                },
            ),
        ));
        let rounded = coordinates(&paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    radius: Pt(4.0),
                    ..Default::default()
                },
            ),
        ));

        let bounds = |v: &[f32]| {
            (
                v.iter().cloned().fold(f32::MAX, f32::min),
                v.iter().cloned().fold(f32::MIN, f32::max),
            )
        };
        let (square_min, square_max) = bounds(&square);
        let (rounded_min, rounded_max) = bounds(&rounded);

        assert!(
            rounded_min >= square_min - 0.01,
            "{rounded_min} < {square_min}"
        );
        assert!(
            rounded_max <= square_max + 0.01,
            "{rounded_max} > {square_max}"
        );
    }

    #[test]
    fn a_border_all_the_way_round_follows_the_rounded_outline() {
        let rule = BorderSide {
            width: Pt(1.0),
            color: navy(),
        };
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: None,
                    border: Edges::all(Some(rule)),
                    radius: Pt(4.0),
                },
            ),
        );

        assert_eq!(curves(&pdf), 4, "the border did not follow the corners");
        assert!(pdf.contains("\nS\n"), "the border was never stroked");
        assert_eq!(
            pdf.matches(" m\n").count(),
            1,
            "a rounded border is one outline, not four lines:\n{pdf}"
        );
    }

    #[test]
    fn a_rule_under_a_rounded_box_stays_a_straight_line() {
        // A radius with a border on one side only: the background rounds,
        // and the rule stays what it is. Curving it towards a corner that
        // has no other side to meet would be the engine inventing a shape.
        let rule = BorderSide {
            width: Pt(1.0),
            color: navy(),
        };
        let pdf = paint_one(
            20.0,
            sized(
                20.0,
                Decoration {
                    background: Some(navy()),
                    border: Edges {
                        bottom: Some(rule),
                        ..Default::default()
                    },
                    radius: Pt(4.0),
                },
            ),
        );

        assert_eq!(curves(&pdf), 4, "the background should still be rounded");
        assert!(pdf.contains("\nS\n"), "the rule was never stroked");
        assert_eq!(pdf.matches(" m\n").count(), 2, "one outline and one rule");
    }

    #[test]
    fn a_radius_too_big_for_the_box_is_brought_down_rather_than_crossing_over() {
        let pdf = paint_one(
            10.0,
            sized(
                10.0,
                Decoration {
                    background: Some(navy()),
                    radius: Pt(500.0),
                    ..Default::default()
                },
            ),
        );

        let xs = coordinates(&pdf);
        let span = xs.iter().cloned().fold(f32::MIN, f32::max)
            - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!(span < 600.0, "the outline escaped the box: {span}");
        assert_eq!(curves(&pdf), 4);
    }

    /// Every number that appears as a path coordinate.
    fn coordinates(pdf: &str) -> Vec<f32> {
        pdf.lines()
            .filter(|line| line.ends_with(" m") || line.ends_with(" l") || line.ends_with(" c"))
            .flat_map(|line| {
                line.split_whitespace()
                    .filter_map(|token| token.parse::<f32>().ok())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}
