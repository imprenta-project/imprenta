//! What a placed atom draws.
//!
//! The packer works in heights alone; this is where an atom regains a shape.
//! A new primitive adds a variant here and nothing to the packer — the
//! property that keeps the hardest code in the engine untouched as the
//! surface grows.
//!
//! **A box and its content are one atom, not two stacked.** Two atoms would
//! be laid out one after the other, so the text would land below its own
//! background and the next row's fill would paint over it. Containment is the
//! difference between a decorated strip and a table row.

use crate::decoration::Decoration;
use crate::shape::Line;
use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Pt};
use std::sync::Arc;

/// Something drawn inside a box, at an offset from the box's own corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Child {
    pub x: Pt,
    pub y: Pt,
    pub content: Content,
}

/// A rectangle that paints a decoration and holds content inside it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoxContent {
    pub decoration: Decoration,
    pub padding: Edges<Pt>,
    pub children: Vec<Child>,
    /// Explicit width, or `None` to fill whatever is offered.
    ///
    /// A full-width row needs no width; two panels side by side do. Without
    /// it both are painted across the whole content box and overlap, which
    /// looks like a full-width rule where a panel edge should be.
    pub width: Option<Pt>,
    /// Total height including padding. Computed as children are added, so the
    /// atom the packer sees and the rectangle the painter draws can never
    /// disagree about how tall the box is.
    height: Pt,
}

impl BoxContent {
    pub fn new(decoration: Decoration) -> Self {
        Self {
            decoration,
            ..Default::default()
        }
    }

    pub fn with_width(mut self, width: Pt) -> Self {
        self.width = Some(width);
        self
    }

    /// Must be set before any child is added: padding shifts where children
    /// sit, and the offsets are resolved as they go in.
    pub fn with_padding(mut self, padding: Edges<Pt>) -> Self {
        debug_assert!(
            self.children.is_empty(),
            "padding must be set before children are added"
        );
        self.padding = padding;
        self.height = padding.vertical();
        self
    }

    /// Adds `content` below whatever is already inside.
    pub fn stack(mut self, content: Content) -> Self {
        let y = self.content_bottom();
        let height = content.height();
        self.children.push(Child {
            x: self.padding.left,
            y,
            content,
        });
        self.height = self.height + height;
        self
    }

    /// Stacks `content` below what is already inside, at an explicit
    /// horizontal offset — a wrapped cell whose lines are right-aligned.
    pub fn stack_at(mut self, x: Pt, content: Content) -> Self {
        let y = self.content_bottom();
        let height = content.height();
        self.children.push(Child {
            x: self.padding.left + x,
            y,
            content,
        });
        self.height = self.height + height;
        self
    }

    /// Adds `content` at an explicit horizontal offset, on the current row —
    /// how a table places a cell in its column.
    ///
    /// Unlike [`Self::stack`] it does not advance downwards, so a row of
    /// cells stays one row; the box grows only if this child is taller than
    /// what is already on the row.
    pub fn place(mut self, x: Pt, content: Content) -> Self {
        let y = self.row_top();
        let height = content.height();
        self.children.push(Child {
            x: self.padding.left + x,
            y,
            content,
        });
        let needed = y + height + self.padding.bottom;
        if needed.get() > self.height.get() {
            self.height = needed;
        }
        self
    }

    /// Where the next stacked child begins.
    fn content_bottom(&self) -> Pt {
        self.height - self.padding.bottom
    }

    /// The top of the row that [`Self::place`] is filling — the y of the last
    /// placed child, or the top of the content area if there is none.
    fn row_top(&self) -> Pt {
        self.children
            .last()
            .map(|c| c.y)
            .unwrap_or(self.padding.top)
    }

    pub fn height(&self) -> Pt {
        self.height
    }
}

/// A raster image and the box it is drawn into.
///
/// The bytes are shared rather than copied: a logo on every page of a
/// thousand-page report is one buffer, embedded once by the PDF writer.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageContent {
    pub data: Arc<[u8]>,
    pub format: ImageFormat,
    pub width: Pt,
    pub height: Pt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageContent {
    /// Sizes the image to `width`, keeping the aspect ratio of its pixels.
    ///
    /// Scaling from the pixel dimensions rather than asking for both means a
    /// logo cannot be squashed by a careless height.
    pub fn scaled_to_width(
        data: impl Into<Arc<[u8]>>,
        format: ImageFormat,
        pixels: (u32, u32),
        width: Pt,
    ) -> Self {
        let (px_w, px_h) = pixels;
        let ratio = if px_w == 0 {
            0.0
        } else {
            px_h as f32 / px_w as f32
        };
        Self {
            data: data.into(),
            format,
            width,
            height: Pt(width.get() * ratio),
        }
    }
}

/// One drawing instruction, in coordinates local to the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathOp {
    MoveTo(Pt, Pt),
    LineTo(Pt, Pt),
    /// Cubic Bézier: two control points and an end point.
    CurveTo(Pt, Pt, Pt, Pt, Pt, Pt),
    Close,
}

/// A shape drawn from raw path operations.
///
/// The escape hatch. Barcodes, QR codes, sparklines, signatures, seals,
/// watermarks and charts are all paths, and none of them needs the engine to
/// grow a primitive: they are built above it, in whatever language is driving
/// it, and arrive here as coordinates. Anything the engine cannot yet express
/// can be expressed anyway.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasContent {
    pub ops: Vec<PathOp>,
    pub fill: Option<Color>,
    pub stroke: Option<(Color, Pt)>,
    pub width: Pt,
    pub height: Pt,
}

impl CanvasContent {
    pub fn new(width: Pt, height: Pt) -> Self {
        Self {
            ops: Vec::new(),
            fill: None,
            stroke: None,
            width,
            height,
        }
    }

    pub fn filled(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    pub fn stroked(mut self, color: Color, width: Pt) -> Self {
        self.stroke = Some((color, width));
        self
    }

    pub fn op(mut self, op: PathOp) -> Self {
        self.ops.push(op);
        self
    }

    pub fn move_to(self, x: Pt, y: Pt) -> Self {
        self.op(PathOp::MoveTo(x, y))
    }

    pub fn line_to(self, x: Pt, y: Pt) -> Self {
        self.op(PathOp::LineTo(x, y))
    }

    pub fn close(self) -> Self {
        self.op(PathOp::Close)
    }

    /// A rectangle, as four lines. Convenient because bars and cells are the
    /// commonest thing anyone draws.
    pub fn rect(self, x: Pt, y: Pt, w: Pt, h: Pt) -> Self {
        self.move_to(x, y)
            .line_to(x + w, y)
            .line_to(x + w, y + h)
            .line_to(x, y + h)
            .close()
    }
}

/// What a placed atom draws.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// A laid-out line of text.
    Text(Line),
    /// A raster image.
    Image(ImageContent),
    /// A shape drawn from path operations.
    Canvas(CanvasContent),
    /// A clickable region over other content.
    Link(Box<LinkContent>),
    /// A decorated rectangle, with content inside it.
    Box(BoxContent),
    /// Occupies space and paints nothing — spacing, and the placeholder for
    /// an atom whose content has not been supplied.
    Empty,
}

impl Content {
    /// How tall this content is on its own.
    pub fn height(&self) -> Pt {
        match self {
            Self::Text(line) => line.height,
            Self::Box(b) => b.height(),
            Self::Image(i) => i.height,
            Self::Canvas(c) => c.height,
            Self::Link(l) => l.content.height(),
            Self::Empty => Pt(0.0),
        }
    }
}

impl From<Line> for Content {
    fn from(line: Line) -> Self {
        Self::Text(line)
    }
}

impl From<BoxContent> for Content {
    fn from(b: BoxContent) -> Self {
        Self::Box(b)
    }
}

/// Content made clickable.
///
/// Wrapping rather than a flag on every variant: anything can be a link, and
/// a link is not a kind of thing but something done to a thing.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkContent {
    pub target: LinkTarget,
    pub content: Content,
    /// Width of the clickable region. `None` fills what is offered.
    pub width: Option<Pt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// An address outside the document.
    Url(String),
}

impl LinkContent {
    pub fn url(target: impl Into<String>, content: Content) -> Self {
        Self {
            target: LinkTarget::Url(target.into()),
            content,
            width: None,
        }
    }

    pub fn with_width(mut self, width: Pt) -> Self {
        self.width = Some(width);
        self
    }
}

impl From<ImageContent> for Content {
    fn from(i: ImageContent) -> Self {
        Self::Image(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::Shaper;
    use imprenta_core::color::Color;

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn line(text: &str, size: f32) -> Line {
        Shaper::new(ROBOTO.to_vec())
            .break_lines(text, Pt(size), Pt(400.0))
            .remove(0)
    }

    /// Geometry that has been through additions and subtractions does not
    /// come back bit-identical — 8.0 + 10.8 - 4.0 is 14.799999. A tenth of a
    /// point is a thirtieth of a millimetre.
    #[track_caller]
    fn assert_close(actual: Pt, expected: Pt) {
        assert!(
            (actual.get() - expected.get()).abs() < 0.01,
            "expected {expected:?}, got {actual:?}"
        );
    }

    fn navy() -> Decoration {
        Decoration {
            background: Some(Color::parse_hex("#1F4E79").unwrap()),
            ..Default::default()
        }
    }

    #[test]
    fn an_empty_box_is_as_tall_as_its_padding() {
        let b = BoxContent::new(navy()).with_padding(Edges::symmetric(Pt(3.0), Pt(6.0)));

        assert_close(b.height(), Pt(6.0));
    }

    #[test]
    fn a_box_is_as_tall_as_its_content_plus_its_padding() {
        let l = line("Total asiento", 9.0);
        let expected = l.height + Pt(6.0);

        let b = BoxContent::new(navy())
            .with_padding(Edges::symmetric(Pt(3.0), Pt(6.0)))
            .stack(Content::Text(l));

        assert_close(b.height(), expected);
    }

    #[test]
    fn stacked_children_accumulate_height() {
        let a = line("uno", 9.0);
        let b = line("dos", 9.0);
        let expected = a.height + b.height;

        let boxed = BoxContent::new(navy())
            .stack(Content::Text(a))
            .stack(Content::Text(b));

        assert_close(boxed.height(), expected);
    }

    #[test]
    fn stacked_children_sit_one_below_the_other_inside_the_padding() {
        let a = line("uno", 9.0);
        let b = line("dos", 9.0);
        let first_height = a.height;

        let boxed = BoxContent::new(navy())
            .with_padding(Edges::all(Pt(4.0)))
            .stack(Content::Text(a))
            .stack(Content::Text(b));

        assert_close(boxed.children[0].y, Pt(4.0));
        assert_close(boxed.children[0].x, Pt(4.0));
        assert_close(boxed.children[1].y, Pt(4.0) + first_height);
    }

    #[test]
    fn stack_at_places_horizontally_while_still_advancing_down() {
        let a = line("uno", 9.0);
        let b = line("dos", 9.0);
        let first = a.height;

        let boxed = BoxContent::default()
            .stack_at(Pt(10.0), Content::Text(a))
            .stack_at(Pt(20.0), Content::Text(b));

        assert_close(boxed.children[0].x, Pt(10.0));
        assert_close(boxed.children[1].x, Pt(20.0));
        assert_close(boxed.children[1].y, first);
    }

    #[test]
    fn placed_children_share_a_row_rather_than_stacking() {
        // A table row: cells side by side, the row only as tall as one cell.
        let a = line("430000", 8.0);
        let b = line("Cliente", 8.0);
        let height = a.height;

        let row = BoxContent::new(navy())
            .place(Pt(0.0), Content::Text(a))
            .place(Pt(80.0), Content::Text(b));

        assert_close(row.height(), height);
        assert_eq!(row.children[0].x, Pt(0.0));
        assert_eq!(row.children[1].x, Pt(80.0));
        assert_eq!(row.children[0].y, row.children[1].y, "same baseline row");
    }

    #[test]
    fn a_placed_row_is_as_tall_as_its_tallest_cell() {
        let small = line("8pt", 8.0);
        let large = line("14pt", 14.0);
        let tallest = large.height;

        let row = BoxContent::new(navy())
            .place(Pt(0.0), Content::Text(small))
            .place(Pt(80.0), Content::Text(large));

        assert_close(row.height(), tallest);
    }

    #[test]
    fn a_box_fills_what_it_is_offered_unless_told_a_width() {
        assert_eq!(BoxContent::new(navy()).width, None);
        assert_eq!(
            BoxContent::new(navy()).with_width(Pt(200.0)).width,
            Some(Pt(200.0))
        );
    }

    #[test]
    fn a_box_can_hold_another_box() {
        let inner = BoxContent::new(navy())
            .with_padding(Edges::all(Pt(2.0)))
            .stack(Content::Text(line("dentro", 9.0)));
        let inner_height = inner.height();

        let outer = BoxContent::new(Decoration::default())
            .with_padding(Edges::all(Pt(5.0)))
            .stack(Content::Box(inner));

        assert_close(outer.height(), inner_height + Pt(10.0));
    }

    #[test]
    fn content_reports_its_own_height_whatever_it_is() {
        let l = line("x", 9.0);
        let h = l.height;

        assert_eq!(Content::Text(l).height(), h);
        assert_eq!(Content::Empty.height(), Pt(0.0));
        assert_close(
            Content::Box(BoxContent::new(navy()).with_padding(Edges::all(Pt(7.0)))).height(),
            Pt(14.0),
        );
    }

    // ── images ──────────────────────────────────────────────────────────

    const LOGO: &[u8] = include_bytes!("../tests/images/logo.png");

    fn logo(width: f32) -> ImageContent {
        ImageContent::scaled_to_width(LOGO, ImageFormat::Png, (240, 80), Pt(width))
    }

    #[test]
    fn an_image_keeps_the_aspect_ratio_of_its_pixels() {
        // 240x80 is 3:1, so 120pt wide must be 40pt tall. Asking for both
        // dimensions is how a logo ends up squashed.
        assert_close(logo(120.0).height, Pt(40.0));
    }

    #[test]
    fn an_image_reports_its_height_as_content() {
        assert_close(Content::Image(logo(120.0)).height(), Pt(40.0));
    }

    #[test]
    fn a_zero_width_image_does_not_divide_by_zero() {
        let degenerate = ImageContent::scaled_to_width(LOGO, ImageFormat::Png, (0, 80), Pt(100.0));

        assert_close(degenerate.height, Pt(0.0));
    }

    #[test]
    fn an_image_can_sit_inside_a_box_beside_text() {
        // A letterhead: mark on the left, company details to the right.
        let l = line("CONTAPRO S.L.", 11.0);
        let head = BoxContent::new(Decoration::default())
            .place(Pt(0.0), Content::Image(logo(120.0)))
            .place(Pt(140.0), Content::Text(l));

        assert_close(head.height(), Pt(40.0));
        assert_eq!(head.children.len(), 2);
    }

    // ── canvas and links ────────────────────────────────────────────────

    #[test]
    fn a_canvas_is_as_tall_as_it_was_declared() {
        let c = CanvasContent::new(Pt(60.0), Pt(24.0));

        assert_close(Content::Canvas(c).height(), Pt(24.0));
    }

    #[test]
    fn a_rectangle_is_four_lines_and_a_close() {
        let c = CanvasContent::new(Pt(60.0), Pt(24.0)).rect(Pt(0.0), Pt(0.0), Pt(10.0), Pt(5.0));

        assert_eq!(c.ops.len(), 5);
        assert!(matches!(c.ops[0], PathOp::MoveTo(..)));
        assert!(matches!(c.ops[4], PathOp::Close));
    }

    #[test]
    fn a_canvas_carries_its_own_paint() {
        let navy = Color::parse_hex("#1F4E79").unwrap();
        let c = CanvasContent::new(Pt(10.0), Pt(10.0))
            .filled(navy)
            .stroked(Color::BLACK, Pt(1.0));

        assert_eq!(c.fill, Some(navy));
        assert_eq!(c.stroke, Some((Color::BLACK, Pt(1.0))));
    }

    #[test]
    fn a_link_is_as_tall_as_what_it_wraps() {
        let l = line("pulsa aquí", 9.0);
        let height = l.height;

        let link = Content::Link(Box::new(LinkContent::url(
            "https://example.org",
            Content::Text(l),
        )));

        assert_close(link.height(), height);
    }

    #[test]
    fn anything_at_all_can_be_made_clickable() {
        // A logo that links to the company site is the common case.
        let logo = ImageContent::scaled_to_width(LOGO, ImageFormat::Png, (240, 80), Pt(120.0));
        let height = logo.height;

        let link = Content::Link(Box::new(LinkContent::url(
            "https://example.org",
            Content::Image(logo),
        )));

        assert_close(link.height(), height);
    }
}
