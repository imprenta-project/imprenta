//! Writes a PDF page by page, holding none of them.
//!
//! # Why this exists
//!
//! A PDF is a sequence of numbered objects followed by a cross-reference
//! table that says where each one starts. Nothing in the format requires the
//! objects to be built before they are written, or to be written in any
//! particular order — which means a page can be serialised, compressed and
//! forgotten the moment it is finished, and all that has to survive it is
//! **one xref entry**.
//!
//! The writer this replaced did not do that. It kept every finished page in a
//! `Vec` until the document closed, then walked the whole collection twice —
//! once to renumber every object reference, once to write them — into a
//! buffer it preallocated the size of the entire file. Measured on a ten
//! thousand page ledger that was **5.25 KB retained per page against 2.22 KB
//! of output**: the pages once, the file again, and the engine's own working
//! set lost in the noise between them.
//!
//! Here a page costs its bytes in the output and sixteen bytes of offset.
//!
//! # What it deliberately does not do
//!
//! Everything a document engine does not need. There is no transparency
//! group, no blend mode, no pattern, no shading, no clip path, no tagged
//! structure tree, no PDF/A conformance and no encryption. Each of those is a
//! real feature of the format and none of them is reachable from this
//! engine's IR, so carrying the code for them would be carrying a second
//! thing to keep working.
//!
//! What it does do is the whole of what a page of this engine can contain:
//! glyph runs in embedded subset fonts with a working `ToUnicode` map, filled
//! and stroked paths with per-colour opacity, PNG and JPEG images, and link
//! annotations.
//!
//! # Order of objects in the file
//!
//! Pages first, as they close. Fonts, images and the page tree last, because
//! only the end of the document knows the full set of glyphs a face was asked
//! for — and a subset built before that would be missing letters. Object
//! *order* in a PDF is free; only the xref has to be right.

mod blocks;
mod font;
mod image;

use blocks::Blocks;
use imprenta_core::color::Color;
use pdf_writer::{Chunk, Content, Finish, Name, Rect, Ref, Str};
use std::collections::HashMap;
use std::sync::Arc;

pub use font::{FaceId, Glyph};
pub use image::{ImageFormat, ImageId};

/// Knobs that change the bytes without changing the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Compressing content streams roughly halves the file. Turning it off
    /// makes the operators readable, which is how the tests check that a
    /// rectangle was actually drawn rather than merely intended.
    pub compress: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { compress: true }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("the font could not be read as an OpenType or TrueType file")]
    UnreadableFont,
    #[error("the font could not be subsetted: {0}")]
    Subset(String),
    #[error("a PDF must have at least one page")]
    NoPages,
}

/// One step of a path, in points from the top-left corner of the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CurveTo(f32, f32, f32, f32, f32, f32),
    Close,
}

/// A rectangle in the same coordinates, for a link's clickable region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A document being written.
pub struct Writer {
    /// The file so far, in pieces that are never moved. See [`blocks`].
    blocks: Blocks,
    /// Where each object starts, which is the whole of what a finished page
    /// leaves behind: sixteen bytes against the two kilobytes it weighed.
    offsets: Vec<(i32, usize)>,
    /// The next free object number. Everything is allocated from here in the
    /// order it is written, so the xref comes out dense.
    next: i32,
    /// Reserved before the first page, because every page names it as its
    /// parent and a page is written long before the tree it belongs to.
    page_tree: Ref,
    page_refs: Vec<Ref>,
    faces: Vec<font::Face>,
    images: Vec<image::Embedded>,
    /// Which registered image an already-seen buffer became, keyed on the
    /// address of that buffer. A logo on nine thousand pages is decoded once.
    ///
    /// Identity and not a hash of the contents: the shared buffer *is* the
    /// asset, and hashing fifty kilobytes of PNG once per page to discover
    /// that it is the same fifty kilobytes is work nobody needs. Sound only
    /// because the buffer is kept alive — a raw address whose allocation has
    /// been freed can be handed out again, and the second image would be
    /// drawn as the first.
    seen_images: HashMap<*const u8, ImageId>,
    settings: Settings,
}

impl Writer {
    pub fn new(settings: Settings) -> Self {
        let mut blocks = Blocks::default();
        // Identity-H encoding and CID fonts are PDF 1.4, and everything else
        // written here is older still. The four high bytes on the second line
        // are the convention that tells a program transferring the file that
        // it is not text.
        blocks.push(b"%PDF-1.7\n%\x80\x80\x80\x80\n\n");
        Self {
            blocks,
            offsets: Vec::new(),
            next: 2,
            page_tree: Ref::new(1),
            page_refs: Vec::new(),
            faces: Vec::new(),
            images: Vec::new(),
            seen_images: HashMap::new(),
            settings,
        }
    }

    fn alloc(&mut self) -> Ref {
        let id = Ref::new(self.next);
        self.next += 1;
        id
    }

    /// Writes one indirect object and records where it starts.
    ///
    /// A chunk of its own for each, which reads as wasteful and is not: a
    /// chunk is a kilobyte of scratch that is reused by the allocator the
    /// moment the object has been appended, and building objects into one
    /// growing buffer is exactly what this crate exists not to do.
    fn object(&mut self, id: Ref, write: impl FnOnce(&mut Chunk)) {
        let mut chunk = Chunk::new();
        write(&mut chunk);
        self.offsets.push((id.get(), self.blocks.len()));
        self.blocks.push(chunk.as_bytes());
    }

    /// Registers a typeface.
    ///
    /// The bytes are kept until the document closes, because the subset
    /// cannot be built until every page has been painted and asked for the
    /// glyphs it needed.
    pub fn add_face(&mut self, data: Vec<u8>) -> Result<FaceId, WriteError> {
        let id = FaceId(self.faces.len());
        self.faces.push(font::Face::new(data)?);
        Ok(id)
    }

    /// Registers an image, or hands back the one this buffer already became.
    ///
    /// `None` means the bytes could not be read. That is not an error here:
    /// the painter has no way to report one, and a logo nobody can decode
    /// must not take a nine thousand page render down with it.
    pub fn add_image(&mut self, data: &Arc<[u8]>, format: ImageFormat) -> Option<ImageId> {
        let key = Arc::as_ptr(data) as *const u8;
        if let Some(id) = self.seen_images.get(&key) {
            return Some(*id);
        }
        let mut embedded = image::Embedded::decode(data, format)?;
        embedded.source = Some(Arc::clone(data));
        let id = ImageId(self.images.len());
        self.images.push(embedded);
        self.seen_images.insert(key, id);
        Some(id)
    }

    /// Begins a page. It reaches the file when the page is finished.
    pub fn page(&mut self, width: f32, height: f32) -> PageWriter<'_> {
        let mut content = Content::new();
        // One flip for the whole page rather than one per drawing operation.
        // PDF's origin is the bottom left and every coordinate this engine
        // has is measured from the top, so somebody has to turn the page
        // over; doing it once means the operators in the stream read as the
        // painter wrote them.
        content.save_state();
        content.transform([1.0, 0.0, 0.0, -1.0, 0.0, height]);
        PageWriter {
            writer: self,
            content,
            width,
            height,
            fonts: Vec::new(),
            images: Vec::new(),
            alphas: Vec::new(),
            links: Vec::new(),
            fill: None,
            stroke: None,
            alpha: 255,
        }
    }

    pub fn pages(&self) -> usize {
        self.page_refs.len()
    }

    /// Writes the fonts, the images, the page tree and the catalogue, then
    /// the cross-reference table.
    pub fn finish(mut self) -> Result<Vec<u8>, WriteError> {
        if self.page_refs.is_empty() {
            return Err(WriteError::NoPages);
        }

        for index in 0..self.faces.len() {
            let Some(font) = self.faces[index].reference else {
                // Registered and never drawn with. A `/Font` entry pointing
                // at an object that does not exist is a broken file, so an
                // unused face is named nowhere and written not at all.
                continue;
            };
            let refs = font::Refs {
                font,
                cid: self.alloc(),
                descriptor: self.alloc(),
                file: self.alloc(),
                to_unicode: self.alloc(),
            };
            let face = std::mem::replace(&mut self.faces[index], font::Face::empty());
            let compress = self.settings.compress;
            let mut written = Ok(());
            self.object_group(|chunk| written = font::write(chunk, &face, refs, compress));
            written?;
        }

        for index in 0..self.images.len() {
            let Some(reference) = self.images[index].reference else {
                continue;
            };
            let mask = self.images[index].alpha.is_some().then(|| self.alloc());
            let embedded = std::mem::take(&mut self.images[index]);
            let compress = self.settings.compress;
            self.object_group(|chunk| image::write(chunk, &embedded, reference, mask, compress));
        }

        let tree = self.page_tree;
        let count = self.page_refs.len() as i32;
        let kids = std::mem::take(&mut self.page_refs);
        self.object(tree, |chunk| {
            chunk.pages(tree).count(count).kids(kids.iter().copied());
        });

        let catalog = self.alloc();
        self.object(catalog, |chunk| {
            let mut dict: pdf_writer::writers::Catalog<'_> = chunk.indirect(catalog).start();
            dict.pages(tree);
            dict.finish();
        });

        Ok(self.trailer(catalog))
    }

    /// Writes several objects that have to be built together, recording where
    /// each of them starts.
    ///
    /// A font is five objects that reference each other and an image is two,
    /// and splitting them would mean threading a chunk through every writer.
    /// The offsets are recovered by walking the chunk for each object's
    /// header, in the order the chunk says it wrote them.
    fn object_group(&mut self, write: impl FnOnce(&mut Chunk)) {
        let mut chunk = Chunk::new();
        write(&mut chunk);
        let base = self.blocks.len();
        let bytes = chunk.as_bytes();
        let mut at = 0usize;
        for id in chunk.refs() {
            let head = format!("{} 0 obj", id.get());
            let found = find(&bytes[at..], head.as_bytes()).expect("a chunk holds what it says");
            self.offsets.push((id.get(), base + at + found));
            at += found + head.len();
        }
        self.blocks.push(bytes);
    }

    /// The cross-reference table, the trailer, and the file's last line.
    ///
    /// Written by hand because everything above it has been: the offsets are
    /// this writer's own, and a table entry is twenty bytes.
    fn trailer(mut self, catalog: Ref) -> Vec<u8> {
        self.offsets.sort_unstable();
        let size = 1 + self.offsets.last().map_or(0, |(id, _)| *id);
        let start = self.blocks.len();

        let mut table = format!("xref\n0 {size}\n0000000000 65535 f\r\n");
        // Object numbers are handed out densely and every one of them is
        // written, so the table runs straight through. A gap would still make
        // a valid file — the entry says free — and this loop is the only
        // thing that would notice if the allocator stopped being dense.
        let mut expected = 1;
        for (id, offset) in &self.offsets {
            while expected < *id {
                table.push_str("0000000000 65535 f\r\n");
                expected += 1;
            }
            table.push_str(&format!("{offset:010} 00000 n\r\n"));
            expected += 1;
        }
        table.push_str(&format!(
            "trailer\n<<\n  /Size {size}\n  /Root {} 0 R\n>>\nstartxref\n{start}\n%%EOF",
            catalog.get()
        ));
        self.blocks.push(table.as_bytes());

        self.blocks.into_vec()
    }
}

/// Where `needle` first appears in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A page being painted.
///
/// Everything drawn goes straight into one content stream, and nothing is
/// kept once [`finish`](Self::finish) has written it.
pub struct PageWriter<'a> {
    writer: &'a mut Writer,
    content: Content,
    width: f32,
    height: f32,
    /// The faces this page names, in the order it first used them — which is
    /// what `/f0` and `/f1` mean, and why they can mean something else on the
    /// next page.
    fonts: Vec<FaceId>,
    images: Vec<ImageId>,
    /// The distinct opacities this page asked for. Anything short of opaque
    /// needs a graphics state object, and one per value per page is enough.
    alphas: Vec<u8>,
    links: Vec<(Region, String)>,
    /// The paint state as the stream stands, so a colour is set once rather
    /// than before every glyph run of a table.
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    alpha: u8,
}

impl PageWriter<'_> {
    /// Fills `path` in `color`, honouring its alpha.
    pub fn fill(&mut self, path: &[PathOp], color: Color) {
        if path.is_empty() {
            return;
        }
        self.set_alpha(color.a);
        self.set_fill(color);
        self.trace(path);
        self.content.fill_nonzero();
    }

    /// Strokes `path` in `color` at `width` points.
    pub fn stroke(&mut self, path: &[PathOp], color: Color, width: f32) {
        if path.is_empty() {
            return;
        }
        self.set_alpha(color.a);
        self.set_stroke(color, width);
        self.trace(path);
        self.content.stroke();
    }

    /// Draws one run of glyphs with its baseline starting at `(x, y)`.
    ///
    /// `text` is the string the glyphs were shaped from, and the ranges in
    /// them index it. That is what becomes the `ToUnicode` map, and without a
    /// correct one the page looks perfect and its text cannot be copied,
    /// searched or read aloud.
    #[allow(clippy::too_many_arguments)]
    pub fn glyphs(
        &mut self,
        face: FaceId,
        size: f32,
        x: f32,
        y: f32,
        glyphs: &[Glyph],
        text: &str,
        color: Color,
    ) {
        if glyphs.is_empty() || size <= 0.0 {
            return;
        }
        self.set_alpha(color.a);
        self.set_fill(color);

        let slot = self.font_slot(face);
        let next = &mut self.writer.next;
        let state = &mut self.writer.faces[face.0];
        if state.reference.is_none() {
            state.reference = Some(Ref::new(*next));
            *next += 1;
        }
        let run = state.encode(glyphs, text, size);

        self.content.begin_text();
        self.content
            .set_font(Name(format!("f{slot}").as_bytes()), size);
        // The text matrix carries the flip back out again: the page's own
        // transform turned the world upside down, and glyphs drawn in it
        // would be too.
        self.content.set_text_matrix([1.0, 0.0, 0.0, -1.0, x, y]);

        // One `TJ` for the run, with a numeric nudge wherever the shaper's
        // advance differs from the font's own. Without those the viewer
        // advances by the widths in the font, and every kerned pair drifts.
        let mut show = self.content.show_positioned();
        let mut items = show.items();
        let mut start = 0usize;
        for (index, adjustment) in &run.adjustments {
            items.show(Str(&run.encoded[start..(index + 1) * 2]));
            items.adjust(-adjustment * 1000.0 / size);
            start = (index + 1) * 2;
        }
        if start < run.encoded.len() {
            items.show(Str(&run.encoded[start..]));
        }
        items.finish();
        show.finish();
        self.content.end_text();
    }

    /// Draws `image` with its top-left corner at `(x, y)`.
    pub fn image(&mut self, image: ImageId, x: f32, y: f32, width: f32, height: f32) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let slot = self.image_slot(image);
        let state = &mut self.writer.images[image.0];
        if state.reference.is_none() {
            state.reference = Some(Ref::new(self.writer.next));
            self.writer.next += 1;
        }

        self.content.save_state();
        // An image XObject is drawn into the unit square with its origin at
        // the bottom left, so it is placed by a transform and never by
        // coordinates. The negative height is the page's flip again.
        self.content
            .transform([width, 0.0, 0.0, -height, x, y + height]);
        self.content.x_object(Name(format!("i{slot}").as_bytes()));
        self.content.restore_state();
        // `Q` restored whatever was set before `q`, so what this page thought
        // it knew about the paint state is no longer true.
        self.fill = None;
        self.stroke = None;
        self.alpha = 255;
    }

    /// Registers `data` if this is the first sight of it, then draws it.
    ///
    /// The painter meets a picture rather than choosing one, so this is where
    /// a buffer becomes an object — once, however many pages carry it.
    pub fn image_data(
        &mut self,
        data: &Arc<[u8]>,
        format: ImageFormat,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let Some(id) = self.writer.add_image(data, format) else {
            return;
        };
        self.image(id, x, y, width, height);
    }

    /// Marks a region of the page as a link to `url`.
    ///
    /// An annotation rather than part of the content stream, which is why it
    /// is collected here and written with the page.
    pub fn link(&mut self, region: Region, url: &str) {
        self.links.push((region, url.to_string()));
    }

    /// Writes the page and everything on it, then drops it.
    pub fn finish(self) {
        let PageWriter {
            writer,
            mut content,
            width,
            height,
            fonts,
            images,
            alphas,
            links,
            ..
        } = self;
        content.restore_state();

        let body = content.finish();
        let stream_id = writer.alloc();
        let page_id = writer.alloc();
        let link_ids: Vec<Ref> = links.iter().map(|_| writer.alloc()).collect();

        // The stream before the dictionary that points at it: a reader that
        // takes the file in order meets the ink first, which is the order a
        // linearised document wants anyway.
        let compress = writer.settings.compress;
        writer.object(stream_id, |chunk| {
            if compress {
                let deflated = miniz_oxide::deflate::compress_to_vec_zlib(body.as_slice(), 6);
                let mut stream = chunk.stream(stream_id, &deflated);
                stream.filter(pdf_writer::Filter::FlateDecode);
                stream.finish();
            } else {
                chunk.stream(stream_id, body.as_slice()).finish();
            }
        });

        let tree = writer.page_tree;
        let faces: Vec<Ref> = fonts
            .iter()
            .map(|face| {
                writer.faces[face.0]
                    .reference
                    .expect("a face this page drew with has a reference")
            })
            .collect();
        let pictures: Vec<Ref> = images
            .iter()
            .map(|image| {
                writer.images[image.0]
                    .reference
                    .expect("an image this page drew has a reference")
            })
            .collect();

        writer.object(page_id, |chunk| {
            let mut page = chunk.page(page_id);
            page.parent(tree);
            page.media_box(Rect::new(0.0, 0.0, width, height));
            page.contents(stream_id);
            if !link_ids.is_empty() {
                page.annotations(link_ids.iter().copied());
            }
            let mut resources = page.resources();
            if !faces.is_empty() {
                let mut dict = resources.fonts();
                for (slot, reference) in faces.iter().enumerate() {
                    dict.pair(Name(format!("f{slot}").as_bytes()), *reference);
                }
                dict.finish();
            }
            if !pictures.is_empty() {
                let mut dict = resources.x_objects();
                for (slot, reference) in pictures.iter().enumerate() {
                    dict.pair(Name(format!("i{slot}").as_bytes()), *reference);
                }
                dict.finish();
            }
            if !alphas.is_empty() {
                let mut dict = resources.ext_g_states();
                for (slot, alpha) in alphas.iter().enumerate() {
                    let value = f32::from(*alpha) / 255.0;
                    let mut state: pdf_writer::writers::ExtGraphicsState<'_> =
                        dict.insert(Name(format!("a{slot}").as_bytes())).start();
                    state.non_stroking_alpha(value).stroking_alpha(value);
                    state.finish();
                }
                dict.finish();
            }
            resources.finish();
            page.finish();
        });

        for (id, (region, url)) in link_ids.into_iter().zip(links) {
            writer.object(id, |chunk| {
                let mut annotation = chunk.annotation(id);
                annotation.subtype(pdf_writer::types::AnnotationType::Link);
                // An annotation's rectangle is in the page's own coordinates,
                // which the content stream's flip never reaches.
                annotation.rect(Rect::new(
                    region.x,
                    height - region.y - region.height,
                    region.x + region.width,
                    height - region.y,
                ));
                annotation.border(0.0, 0.0, 0.0, None);
                annotation
                    .action()
                    .action_type(pdf_writer::types::ActionType::Uri)
                    .uri(Str(url.as_bytes()));
                annotation.finish();
            });
        }

        writer.page_refs.push(page_id);
    }

    // ── the paint state ─────────────────────────────────────────────────

    fn set_fill(&mut self, color: Color) {
        if self.fill == Some(color) {
            return;
        }
        self.fill = Some(color);
        let (r, g, b) = channels(color);
        self.content.set_fill_rgb(r, g, b);
    }

    fn set_stroke(&mut self, color: Color, width: f32) {
        if self.stroke == Some((color, width)) {
            return;
        }
        self.stroke = Some((color, width));
        let (r, g, b) = channels(color);
        self.content.set_stroke_rgb(r, g, b);
        self.content.set_line_width(width);
    }

    fn set_alpha(&mut self, alpha: u8) {
        if self.alpha == alpha {
            return;
        }
        self.alpha = alpha;
        let slot = match self.alphas.iter().position(|a| *a == alpha) {
            Some(slot) => slot,
            None => {
                self.alphas.push(alpha);
                self.alphas.len() - 1
            }
        };
        self.content
            .set_parameters(Name(format!("a{slot}").as_bytes()));
    }

    fn trace(&mut self, path: &[PathOp]) {
        for op in path {
            match *op {
                PathOp::MoveTo(x, y) => {
                    self.content.move_to(x, y);
                }
                PathOp::LineTo(x, y) => {
                    self.content.line_to(x, y);
                }
                PathOp::CurveTo(c1x, c1y, c2x, c2y, x, y) => {
                    self.content.cubic_to(c1x, c1y, c2x, c2y, x, y);
                }
                PathOp::Close => {
                    self.content.close_path();
                }
            }
        }
    }

    fn font_slot(&mut self, face: FaceId) -> usize {
        match self.fonts.iter().position(|f| *f == face) {
            Some(slot) => slot,
            None => {
                self.fonts.push(face);
                self.fonts.len() - 1
            }
        }
    }

    fn image_slot(&mut self, image: ImageId) -> usize {
        match self.images.iter().position(|i| *i == image) {
            Some(slot) => slot,
            None => {
                self.images.push(image);
                self.images.len() - 1
            }
        }
    }
}

fn channels(color: Color) -> (f32, f32, f32) {
    (
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
    )
}
