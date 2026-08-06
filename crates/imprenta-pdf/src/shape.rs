//! Phase A, first half — turning text into positioned glyphs.
//!
//! Advances are stored **normalised to the em**, not in points. Shaping the
//! same string at 7 pt and at 14 pt would otherwise be two cache entries for
//! one piece of work, and the cache is where the speed comes from: measuring
//! a 770-page ledger in the prototype hit 299,374 times against 61,397
//! misses, because a ledger is the same labels and digit shapes over and
//! over. A browser reshapes all 360,000.

use imprenta_core::color::Color;
use imprenta_core::units::Pt;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

/// Default leading, as a multiple of the font size.
///
/// Parley's own default is the font's metrics at 1.0×, which for Roboto works
/// out to exactly 1 em: the baseline sits at 0.77 em, leaving 0.23 em for
/// descenders that are themselves 0.24 em deep, so consecutive lines touch.
/// 1.2 is the "single spacing" every word processor and print engine uses.
///
/// Expressed as a multiple of the *font size* rather than of the font's
/// metrics because Tailwind's `leading-*` utilities are font-size multiples,
/// and the engine's model should be the one authors are typing.
const DEFAULT_LINE_HEIGHT: parley::LineHeight = parley::LineHeight::FontSizeRelative(1.2);

/// Longest text the single-line fast path will consider, in bytes.
///
/// The shortcut pays off when a string **repeats**, and length is the proxy
/// for that: field values repeat across thousands of rows — labels, account
/// codes, dates, amounts — while sentences do not. On a miss the shortcut is
/// slightly *more* expensive than a plain layout, because it also allocates a
/// cache entry that will never be hit; measured at ~9% on a ledger of long,
/// unique rows when this was set to 200.
///
/// 64 bytes is the line between a field value and a sentence. It only decides
/// which path to *try* — the path still verifies the text fits, and both
/// paths produce the same line, so a wrong guess costs time and never
/// correctness.
const FAST_PATH_MAX_CHARS: usize = 64;

/// Which face of a family a run is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Face {
    pub weight: Weight,
    pub italic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Weight {
    #[default]
    Regular,
    Bold,
}

impl Face {
    pub const REGULAR: Self = Self {
        weight: Weight::Regular,
        italic: false,
    };
    pub const BOLD: Self = Self {
        weight: Weight::Bold,
        italic: false,
    };
    pub const ITALIC: Self = Self {
        weight: Weight::Regular,
        italic: true,
    };

    fn parley_weight(self) -> parley::FontWeight {
        match self.weight {
            Weight::Regular => parley::FontWeight::NORMAL,
            Weight::Bold => parley::FontWeight::BOLD,
        }
    }

    fn parley_style(self) -> parley::FontStyle {
        if self.italic {
            parley::FontStyle::Italic
        } else {
            parley::FontStyle::Normal
        }
    }
}

/// One positioned glyph. `x_advance` is in em units — multiply by the font
/// size to get points.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    /// u32 to match parley, which produces it, even though OpenType glyph
    /// ids are 16-bit and the writer narrows it back down.
    pub id: u32,
    pub x_advance: f32,
    /// Byte range in the source string this glyph came from. Needed to emit a
    /// correct `ToUnicode` map, without which the PDF looks right but its
    /// text cannot be copied, searched or extracted. A range rather than an
    /// offset because one glyph can stand for several characters (a ligature)
    /// and several glyphs for one (a decomposed accent).
    pub text_range: Range<u32>,
}

/// A shaped run of text, independent of the size it will be drawn at.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    pub glyphs: Vec<Glyph>,
    /// Total advance in em units.
    pub advance: f32,
}

/// The glyph a font substitutes when it has nothing for a character.
///
/// Zero is `.notdef` by definition in every OpenType font — usually an empty
/// box. It renders without complaint, which is what makes it dangerous: the
/// page looks deliberate and a character is simply gone.
pub const NOTDEF: u32 = 0;

/// The characters no glyph was found for, read off text that has already
/// been laid out.
///
/// This is the cheap half of a lesson. The check used to shape the text a
/// second time to ask the question, which on a ledger was **half of all the
/// time spent measuring** — every cell went through the layout engine twice,
/// once for its size and once for this. The lines already hold the answer:
/// a glyph that came back as [`NOTDEF`] is a character the face could not
/// draw, and it carries the byte range it came from.
pub fn missing_in(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        for glyph in line
            .segments
            .iter()
            .flat_map(|s| &s.glyphs)
            .filter(|g| g.id == NOTDEF)
        {
            let range = glyph.text_range.start as usize..glyph.text_range.end as usize;
            for c in line.text.get(range).into_iter().flat_map(str::chars) {
                if !out.contains(c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Warns about characters the chosen face cannot draw.
///
/// Shared rather than written at each call site because silence is the whole
/// danger here: a face with no glyph for a character prints an empty box, and
/// an engine that only noticed inside table cells let a page of Japanese
/// through without a word.
pub fn report(missing: &str, diagnostics: &mut imprenta_core::diagnostic::Diagnostics) {
    if missing.is_empty() {
        return;
    }
    diagnostics.report(
        imprenta_core::diagnostic::Diagnostic::warning(
            "missing-glyph",
            format!("the font has no glyph for {missing:?}"),
        )
        .with_hint("those characters print as empty boxes; pick a font that covers them"),
    );
}

/// As [`report`], for text that has been laid out.
pub fn report_missing_in(lines: &[Line], diagnostics: &mut imprenta_core::diagnostic::Diagnostics) {
    report(&missing_in(lines), diagnostics);
}

/// What parley carries through shaping on our behalf: the index of the
/// author's run a stretch of glyphs came from.
///
/// Reading the face back off the resolved font would mean guessing from
/// weight numbers and synthesis flags. The brush is exact: parley hands back
/// the very value that was pushed for that byte range.
type Brush = u16;

impl ShapedRun {
    /// The characters the font had no glyph for, in source order.
    ///
    /// Empty for text the font covers. A non-empty result is always a defect
    /// worth reporting: nobody intends to print an empty box.
    pub fn missing(&self, text: &str) -> String {
        let mut out = String::new();
        for glyph in self.glyphs.iter().filter(|g| g.id == NOTDEF) {
            if let Some(slice) =
                text.get(glyph.text_range.start as usize..glyph.text_range.end as usize)
            {
                for c in slice.chars() {
                    if !out.contains(c) {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    /// The width this run occupies when drawn at `size`.
    pub fn width_at(&self, size: Pt) -> Pt {
        Pt(self.advance * size.get())
    }
}

/// Shapes text against a font, memoising the result.
pub struct Shaper {
    font_cx: parley::FontContext,
    layout_cx: parley::LayoutContext<Brush>,
    /// The family every face registered under. Layouts name it explicitly;
    /// see [`Shaper::new`].
    family: String,
    /// The registered faces, by the bytes they came from, so the painter can
    /// embed the same file that shaped the text.
    faces: HashMap<Face, Arc<[u8]>>,
    /// Ascent and leading as fractions of the font size, sampled once from
    /// the font. Both scale linearly with size, so the fast path can derive
    /// a line box without laying anything out.
    ascent_ratio: f32,
    leading_ratio: f32,
    cache: HashMap<(Face, String), ShapedRun>,
    hits: u64,
    misses: u64,
    /// How many times text has been handed to the layout engine.
    ///
    /// Not a curiosity. Every table cell used to be laid out twice — once to
    /// measure it and once to ask whether the font could draw it — and on a
    /// ledger that second layout was half of all the time spent measuring.
    /// It is waste that no assertion about a height can see, so it is counted
    /// and asserted on directly.
    layouts: u64,
}

impl Shaper {
    /// Builds a shaper over one regular face.
    pub fn new(font: Vec<u8>) -> Self {
        Self::with_faces([(Face::REGULAR, font)])
    }

    /// Builds a shaper over several faces of one family.
    ///
    /// Faces rather than families: a document sets a word bold, it does not
    /// switch typeface mid-sentence. Real family fallback — a CJK face behind
    /// a Latin one — is a separate concern and needs its own tests.
    pub fn with_faces(faces: impl IntoIterator<Item = (Face, Vec<u8>)>) -> Self {
        // System fonts are switched off deliberately. Registering a font adds
        // it to the collection but does not select it, so with them enabled a
        // layout that names no family silently resolves to whatever the
        // machine happens to have — and the ids that come back are for *that*
        // font, drawn later against ours. The output is a page of plausible
        // gibberish, and every width and line-count assertion still passes.
        //
        // Off, an unresolvable family is a visible failure instead. It also
        // makes output identical across machines, which is what determinism
        // and golden-PDF diffing depend on.
        let mut collection =
            parley::fontique::Collection::new(parley::fontique::CollectionOptions {
                system_fonts: false,
                ..Default::default()
            });
        let mut family = None;
        let mut face_bytes: HashMap<Face, Arc<[u8]>> = HashMap::new();
        for (face, bytes) in faces {
            let shared: Arc<[u8]> = Arc::from(bytes.as_slice());
            let registered = collection.register_fonts(bytes.into(), None);
            let id = registered
                .first()
                .map(|(id, _)| *id)
                .expect("the font contains no usable family");
            family.get_or_insert_with(|| {
                collection
                    .family_name(id)
                    .expect("a registered family always has a name")
                    .to_string()
            });
            face_bytes.insert(face, shared);
        }
        let family = family.expect("a shaper needs at least one face");

        let mut shaper = Self {
            font_cx: parley::FontContext {
                collection,
                source_cache: Default::default(),
            },
            layout_cx: parley::LayoutContext::new(),
            family,
            faces: face_bytes,
            ascent_ratio: 0.0,
            leading_ratio: 0.0,
            cache: HashMap::new(),
            hits: 0,
            misses: 0,
            layouts: 0,
        };
        shaper.sample_metrics();
        shaper
    }

    /// Records the font's vertical metrics once, as ratios of the font size.
    ///
    /// Sampled from a real layout rather than read from the font tables so
    /// that it matches exactly what the full path would compute — the two
    /// must agree to the last decimal or a document's line positions would
    /// depend on which path each line took.
    fn sample_metrics(&mut self) {
        const PROBE: f32 = 1000.0;
        let stack = self.stack();
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, "Hg", 1.0, true);
        builder.push_default(parley::StyleProperty::FontStack(stack));
        builder.push_default(parley::StyleProperty::FontSize(PROBE));
        builder.push_default(parley::StyleProperty::LineHeight(DEFAULT_LINE_HEIGHT));
        let mut layout: parley::Layout<Brush> = builder.build("Hg");
        layout.break_all_lines(None);

        if let Some(line) = layout.lines().next() {
            let m = line.metrics();
            self.ascent_ratio = m.ascent / PROBE;
            self.leading_ratio = m.leading / PROBE;
        }
    }

    /// Names the vendored family, so no layout can fall through to a
    /// system font. Owned because the builder borrows `self` mutably.
    fn stack(&self) -> parley::FontStack<'static> {
        parley::FontStack::Single(parley::FontFamily::Named(std::borrow::Cow::Owned(
            self.family.clone(),
        )))
    }

    /// Shapes `text` in the regular face.
    pub fn shape(&mut self, text: &str) -> ShapedRun {
        self.shape_in(text, Face::REGULAR)
    }

    /// Shapes `text` in `face`, serving repeats from the cache.
    ///
    /// The face is part of the key: the same word in bold is different
    /// glyphs, not the same glyphs drawn heavier.
    pub fn shape_in(&mut self, text: &str, face: Face) -> ShapedRun {
        let key = (face, text.to_string());
        if let Some(cached) = self.cache.get(&key) {
            self.hits += 1;
            return cached.clone();
        }
        self.misses += 1;

        let run = self.shape_uncached(text, face);
        self.cache.insert(key, run.clone());
        run
    }

    /// The bytes of a face, for the painter to embed.
    pub fn face_bytes(&self, face: Face) -> Option<&Arc<[u8]>> {
        self.faces.get(&face)
    }

    /// Every registered face.
    pub fn faces(&self) -> impl Iterator<Item = (&Face, &Arc<[u8]>)> {
        self.faces.iter()
    }

    fn shape_uncached(&mut self, text: &str, face: Face) -> ShapedRun {
        // Shaped once at a reference em, then normalised, so one cache entry
        // serves every size the run is ever drawn at.
        const EM: f32 = 1000.0;

        self.layouts += 1;
        let stack = self.stack();
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(parley::StyleProperty::FontStack(stack));
        builder.push_default(parley::StyleProperty::FontSize(EM));
        builder.push_default(parley::StyleProperty::FontWeight(face.parley_weight()));
        builder.push_default(parley::StyleProperty::FontStyle(face.parley_style()));
        let mut layout: parley::Layout<Brush> = builder.build(text);
        layout.break_all_lines(None);

        let mut glyphs = Vec::new();
        let mut advance = 0.0f32;

        for line in layout.lines() {
            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                // Each glyph is paired with the byte range of the cluster it
                // was shaped from, which is what the ToUnicode map needs.
                let mut clusters = glyph_run.run().visual_clusters().flat_map(|cluster| {
                    let r = cluster.text_range();
                    let range = r.start as u32..r.end as u32;
                    cluster.glyphs().map(move |_| range.clone())
                });

                for g in glyph_run.positioned_glyphs() {
                    glyphs.push(Glyph {
                        id: g.id,
                        x_advance: g.advance / EM,
                        text_range: clusters.next().unwrap_or(0..0),
                    });
                    advance += g.advance / EM;
                }
            }
        }

        ShapedRun { glyphs, advance }
    }

    /// Breaks `text` into lines that fit within `max_width` at `size`.
    ///
    /// Short text that fits on one line takes a shortcut through the cache
    /// that [`Self::shape`] maintains, because that is what a table cell is
    /// and a table cell repeats thousands of times. Anything else — prose,
    /// a forced break, text wider than the column — gets a full layout,
    /// whose result cannot be cached usefully since a break position depends
    /// on the column width as well as the text.
    ///
    /// The two paths must produce identical lines; a test pins that.
    ///
    /// Breaks are taken at Unicode line-break opportunities (UAX #14), not at
    /// spaces: breaking on spaces alone never works for CJK, and mishandles
    /// hyphens and non-breaking spaces everywhere else.
    pub fn break_lines(&mut self, text: &str, size: Pt, max_width: Pt) -> Vec<Line> {
        self.break_lines_in(text, size, max_width, Face::REGULAR)
    }

    /// Breaks a run of differently styled stretches into lines.
    ///
    /// One layout over the whole paragraph, not one per stretch: kerning
    /// crosses a style boundary, Arabic letters join across it, and a line
    /// break may fall inside a bold phrase. Shaping each stretch separately
    /// would get all three wrong.
    pub fn break_rich(&mut self, runs: &[TextRun], size: Pt, max_width: Pt) -> Vec<Line> {
        if runs.iter().all(|r| r.text.is_empty()) {
            return Vec::new();
        }

        // One string, with each run's byte range remembered so its style can
        // be pushed over exactly those bytes.
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(runs.len());
        for run in runs {
            let start = text.len();
            text.push_str(&run.text);
            ranges.push(start..text.len());
        }

        let styles: Vec<(Face, Color)> = runs.iter().map(|r| (r.face, r.color)).collect();
        self.layouts += 1;
        let stack = self.stack();
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, &text, 1.0, true);
        builder.push_default(parley::StyleProperty::FontStack(stack));
        builder.push_default(parley::StyleProperty::FontSize(size.get()));
        builder.push_default(parley::StyleProperty::LineHeight(DEFAULT_LINE_HEIGHT));

        for (i, (run, range)) in runs.iter().zip(&ranges).enumerate() {
            builder.push(
                parley::StyleProperty::FontWeight(run.face.parley_weight()),
                range.clone(),
            );
            builder.push(
                parley::StyleProperty::FontStyle(run.face.parley_style()),
                range.clone(),
            );
            builder.push(parley::StyleProperty::Brush(i as Brush), range.clone());
        }

        let mut layout: parley::Layout<Brush> = builder.build(&text);
        layout.break_all_lines(Some(max_width.get()));
        self.collect_lines(
            &layout,
            Arc::from(text.as_str()),
            size,
            &styles,
            Face::REGULAR,
        )
    }

    /// Breaks `text` into lines set in `face`.
    pub fn break_lines_in(&mut self, text: &str, size: Pt, max_width: Pt, face: Face) -> Vec<Line> {
        if text.is_empty() {
            return Vec::new();
        }

        if let Some(line) = self.try_single_line(text, size, max_width, face) {
            return vec![line];
        }

        self.layouts += 1;
        let stack = self.stack();
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(parley::StyleProperty::FontStack(stack));
        builder.push_default(parley::StyleProperty::FontSize(size.get()));
        builder.push_default(parley::StyleProperty::FontWeight(face.parley_weight()));
        builder.push_default(parley::StyleProperty::FontStyle(face.parley_style()));
        builder.push_default(parley::StyleProperty::LineHeight(DEFAULT_LINE_HEIGHT));
        let mut layout: parley::Layout<Brush> = builder.build(text);
        layout.break_all_lines(Some(max_width.get()));

        let styles = [(face, Color::BLACK)];
        self.collect_lines(&layout, Arc::from(text), size, &styles, face)
    }

    /// Turns a broken layout into lines of styled stretches.
    fn collect_lines(
        &self,
        layout: &parley::Layout<Brush>,
        source: Arc<str>,
        size: Pt,
        styles: &[(Face, Color)],
        fallback: Face,
    ) -> Vec<Line> {
        layout
            .lines()
            .map(|line| {
                let metrics = line.metrics();
                let mut segments = Vec::new();
                let mut x = 0.0f32;

                // Where the current run's glyphs have been read up to. A line
                // that changes style part-way through is several *glyph* runs
                // over one parley run, and each of them walks that run's
                // clusters from the beginning: without this, the bold half of
                // "Total 1.234,00" is handed the source ranges of "Total ",
                // and every glyph in it claims to stand for the wrong letter.
                //
                // Nothing on the page moves, which is what makes it worth a
                // comment. What breaks is the `ToUnicode` map built from
                // these ranges — the document looks perfect and the text
                // copies out as nonsense.
                let mut taken = 0usize;
                let mut run_at: Option<std::ops::Range<usize>> = None;

                for item in line.items() {
                    let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                        continue;
                    };
                    let run_range = glyph_run.run().text_range();
                    if run_at.as_ref() != Some(&run_range) {
                        run_at = Some(run_range);
                        taken = 0;
                    }
                    let mut clusters = glyph_run
                        .run()
                        .visual_clusters()
                        .flat_map(|cluster| {
                            let r = cluster.text_range();
                            let range = r.start as u32..r.end as u32;
                            cluster.glyphs().map(move |_| range.clone())
                        })
                        .skip(taken);

                    let mut glyphs = Vec::new();
                    let mut advance = 0.0f32;
                    for g in glyph_run.positioned_glyphs() {
                        glyphs.push(Glyph {
                            id: g.id,
                            // Absolute here, not em-normalised: a line is
                            // already tied to the size it was broken at.
                            x_advance: g.advance,
                            text_range: clusters.next().unwrap_or(0..0),
                        });
                        advance += g.advance;
                    }

                    // The brush names the author's run, so a stretch keeps
                    // the face and colour it was asked for even when the
                    // break fell in the middle of it.
                    let (face, color) = styles
                        .get(glyph_run.style().brush as usize)
                        .copied()
                        .unwrap_or((fallback, Color::BLACK));

                    taken += glyphs.len();
                    segments.push(Segment {
                        glyphs,
                        face,
                        color,
                        x: Pt(x),
                        width: Pt(advance),
                    });
                    x += advance;
                }

                Line {
                    segments,
                    text: Arc::clone(&source),
                    size,
                    width: Pt(metrics.advance),
                    height: Pt(metrics.line_height),
                    baseline: Pt(metrics.ascent + metrics.leading / 2.0),
                }
            })
            .collect()
    }

    /// Builds the line directly from the shaping cache when the text is a
    /// short single line that fits. `None` means the caller must lay out.
    fn try_single_line(&mut self, text: &str, size: Pt, max_width: Pt, face: Face) -> Option<Line> {
        if text.len() > FAST_PATH_MAX_CHARS || text.contains('\n') || text.contains('\r') {
            return None;
        }

        let run = self.shape_in(text, face);
        let width = run.width_at(size);
        if width.get() > max_width.get() {
            return None;
        }

        let s = size.get();
        Some(Line {
            segments: vec![Segment {
                glyphs: run
                    .glyphs
                    .iter()
                    .map(|g| Glyph {
                        id: g.id,
                        // The cache holds em units; a line carries absolute ones.
                        x_advance: g.x_advance * s,
                        text_range: g.text_range.clone(),
                    })
                    .collect(),
                face,
                color: Color::BLACK,
                x: Pt(0.0),
                width,
            }],
            text: Arc::from(text),
            size,
            width,
            height: Pt(s * 1.2),
            baseline: Pt((self.ascent_ratio + self.leading_ratio / 2.0) * s),
        })
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// How many times text has been handed to the layout engine.
    pub fn layouts(&self) -> u64 {
        self.layouts
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }
}

/// One stretch of author text with its own style.
///
/// A paragraph is a list of these. Splitting style from text at this level —
/// rather than styling whole paragraphs — is what makes a bold word inside a
/// sentence expressible at all, and it is the shape a React `<strong>` inside
/// a `<Text>` naturally produces.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub face: Face,
    pub color: Color,
}

impl TextRun {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            face: Face::REGULAR,
            color: Color::BLACK,
        }
    }

    pub fn bold(mut self) -> Self {
        self.face = Face::BOLD;
        self
    }

    pub fn italic(mut self) -> Self {
        self.face = Face::ITALIC;
        self
    }

    pub fn in_face(mut self, face: Face) -> Self {
        self.face = face;
        self
    }

    pub fn inked(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

/// A stretch of one line drawn in a single face and colour.
///
/// A line is a list of these rather than one glyph run because a sentence can
/// change weight or colour part-way through, and a break can fall anywhere —
/// including inside a bold phrase.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub glyphs: Vec<Glyph>,
    pub face: Face,
    pub color: Color,
    /// Where this stretch begins, measured from the start of the line.
    pub x: Pt,
    /// Advance of this stretch alone.
    pub width: Pt,
}

/// One laid-out line, ready to become an [`crate::atom::Atom`].
///
/// Unlike [`ShapedRun`], a line is tied to the size it was broken at, so its
/// measurements are absolute.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The stretches that make up the line, left to right.
    pub segments: Vec<Segment>,
    /// The text these glyphs were shaped from. Shared with every other line
    /// of the same paragraph — `Glyph::text_range` indexes into it, so the
    /// painter needs no re-basing and the string is stored once.
    pub text: Arc<str>,
    /// The size this line was set at. Carried on the line because a document
    /// mixes sizes, so the painter cannot be told one globally.
    pub size: Pt,
    pub width: Pt,
    /// Height of the line box — what the packer will budget for.
    pub height: Pt,
    /// Distance from the top of the line box down to the baseline.
    pub baseline: Pt,
}

impl Line {
    /// Every glyph on the line, in order, whatever face it is set in.
    pub fn glyphs(&self) -> impl Iterator<Item = &Glyph> {
        self.segments.iter().flat_map(|s| s.glyphs.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(|s| s.glyphs.is_empty())
    }

    /// The face of the first stretch — what a single-style line was set in.
    pub fn face(&self) -> Face {
        self.segments.first().map(|s| s.face).unwrap_or_default()
    }

    /// The colour of the first stretch.
    pub fn color(&self) -> Color {
        self.segments
            .first()
            .map(|s| s.color)
            .unwrap_or(Color::BLACK)
    }

    /// Widens this line's spaces until it reaches `width`.
    ///
    /// Justification is not alignment with a different offset: nothing moves,
    /// the gaps between the words grow. Only the spaces grow — scaling every
    /// advance would also hit the number and would set the words in a font
    /// nobody chose.
    ///
    /// A trailing space takes no share, and does not count towards the line
    /// either: it **hangs** past the edge. Almost every line a breaker returns
    /// ends in one, so counting it leaves the last visible glyph short by
    /// exactly its own advance — and since that advance is the same on every
    /// line, the right edge comes out straight but inset, which reads as flush
    /// until something else on the page is set against the same margin.
    ///
    /// Callers are expected to leave the last line of a paragraph alone; a
    /// justified last line is the giveaway of a naive implementation.
    pub fn justify(&mut self, width: Pt) {
        let text = Arc::clone(&self.text);
        let is_space = |glyph: &Glyph| {
            text.get(glyph.text_range.start as usize..glyph.text_range.end as usize)
                .is_some_and(|s| !s.is_empty() && s.chars().all(char::is_whitespace))
        };

        // Which glyphs are spaces, in order. Collected rather than walked
        // twice because `glyphs()` flattens the segments and cannot be run
        // backwards, and the trailing run has to be found from the end.
        let spaces: Vec<bool> = self.glyphs().map(&is_space).collect();
        let trailing = spaces.iter().rev().take_while(|&&s| s).count();
        let stretchable = spaces[..spaces.len() - trailing]
            .iter()
            .filter(|&&s| s)
            .count();
        if stretchable == 0 {
            return;
        }

        // Measured to the last glyph anybody can see, not to the end of the
        // line. The hanging space still occupies its advance afterwards, so
        // the glyphs add up to more than `width` — deliberately, since what
        // has to land on the margin is the text.
        let hanging: f32 = self
            .glyphs()
            .skip(spaces.len() - trailing)
            .map(|glyph| glyph.x_advance)
            .sum();
        let slack = width.get() - (self.width.get() - hanging);
        if slack <= 0.0 {
            return;
        }

        let extra = slack / stretchable as f32;
        let mut at = 0;
        let mut x = Pt(0.0);

        for segment in &mut self.segments {
            segment.x = x;
            let mut grown = 0.0f32;
            for glyph in &mut segment.glyphs {
                if spaces[at] && at < spaces.len() - trailing {
                    glyph.x_advance += extra;
                    grown += extra;
                }
                at += 1;
            }
            segment.width = segment.width + Pt(grown);
            x = x + segment.width;
        }

        self.width = width;
    }

    /// Sets the ink colour of every stretch. Shaping is colour-blind — the
    /// same glyphs serve every colour — so this is applied after the fact
    /// rather than being a second cache key.
    pub fn with_color(mut self, color: Color) -> Self {
        for segment in &mut self.segments {
            segment.color = color;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vendored so metrics are identical on every machine — see
    /// `tests/fonts/README.md`.
    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    const ROBOTO_BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
    const ROBOTO_ITALIC: &[u8] = include_bytes!("../tests/fonts/Roboto-Italic.ttf");

    fn shaper() -> Shaper {
        Shaper::new(ROBOTO.to_vec())
    }

    fn family() -> Shaper {
        Shaper::with_faces([
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, ROBOTO_BOLD.to_vec()),
            (Face::ITALIC, ROBOTO_ITALIC.to_vec()),
        ])
    }

    #[test]
    fn an_empty_string_shapes_to_nothing() {
        let run = shaper().shape("");

        assert!(run.glyphs.is_empty());
        assert_eq!(run.advance, 0.0);
        assert_eq!(run.width_at(Pt(12.0)), Pt(0.0));
    }

    #[test]
    fn the_glyphs_come_from_the_vendored_font_and_no_other() {
        // Registering a font puts it in the collection; it does not select
        // it. Without an explicit font stack parley falls back to a system
        // font, the writer draws those ids against Roboto, and every letter
        // comes out shifted — a page of plausible-looking gibberish that
        // every metric test in this file happily passes.
        //
        // These ids are read straight out of Roboto's cmap table. They are
        // the only assertion here that can tell the right font from a
        // convincing impostor.
        let mut s = shaper();

        assert_eq!(s.shape("M").glyphs[0].id, 50, "not Roboto");
        assert_eq!(s.shape("K").glyphs[0].id, 48, "not Roboto");
        assert_eq!(s.shape("o").glyphs[0].id, 84, "not Roboto");
        assert_eq!(s.shape("m").glyphs[0].id, 82, "not Roboto");
    }

    #[test]
    fn line_breaking_uses_the_vendored_font_too() {
        // The two paths build separate parley layouts, so they can diverge.
        let line = shaper().break_lines("M", Pt(10.0), Pt(400.0)).remove(0);

        assert_eq!(line.glyphs().next().unwrap().id, 50, "not Roboto");
    }

    #[test]
    fn every_character_of_plain_latin_text_produces_a_glyph() {
        // "Total" has no ligatures or combining marks in Roboto, so the
        // glyph count is the character count.
        let run = shaper().shape("Total");

        assert_eq!(run.glyphs.len(), 5);
    }

    #[test]
    fn a_run_advances_by_the_sum_of_its_glyphs() {
        let run = shaper().shape("Total");

        let summed: f32 = run.glyphs.iter().map(|g| g.x_advance).sum();
        assert!(
            (run.advance - summed).abs() < 1e-6,
            "advance {} vs summed {summed}",
            run.advance
        );
    }

    #[test]
    fn wide_glyphs_measure_wider_than_narrow_ones() {
        let mut s = shaper();

        let wide = s.shape("MMMM").advance;
        let narrow = s.shape("iiii").advance;

        assert!(wide > narrow, "M ({wide}) should exceed i ({narrow})");
    }

    #[test]
    fn width_is_proportional_to_font_size() {
        // The invariant that lets the cache ignore size entirely.
        let run = shaper().shape("Total");

        let small = run.width_at(Pt(7.0)).get();
        let large = run.width_at(Pt(14.0)).get();

        assert!((large - small * 2.0).abs() < 1e-4, "{small} vs {large}");
    }

    #[test]
    fn a_realistic_seven_point_label_measures_a_plausible_width() {
        // Guards against a units slip that a proportionality test cannot see:
        // if advances were left in font units, or in pixels, this would be
        // off by orders of magnitude. Roughly 0.5 em per character at 7 pt
        // puts "Total asiento" (13 chars) in the tens of points.
        let width = shaper().shape("Total asiento").width_at(Pt(7.0)).get();

        assert!(
            (25.0..70.0).contains(&width),
            "13 characters at 7pt measured {width}pt"
        );
    }

    #[test]
    fn shaping_is_deterministic() {
        let mut a = shaper();
        let mut b = shaper();

        assert_eq!(a.shape("Cliente comercial"), b.shape("Cliente comercial"));
    }

    #[test]
    fn glyphs_carry_the_source_range_they_came_from() {
        // Without this the PDF's ToUnicode map is wrong and the text cannot
        // be copied or searched — a defect invisible on screen.
        let run = shaper().shape("Total");

        let ranges: Vec<Range<u32>> = run.glyphs.iter().map(|g| g.text_range.clone()).collect();
        assert_eq!(ranges, vec![0..1, 1..2, 2..3, 3..4, 4..5]);
    }

    #[test]
    fn a_multibyte_character_reports_its_full_byte_range() {
        // "ó" is two bytes in UTF-8. A range that stopped at one byte would
        // slice mid-codepoint and produce a corrupt ToUnicode entry.
        let run = shaper().shape("ó");

        assert_eq!(run.glyphs.len(), 1);
        assert_eq!(run.glyphs[0].text_range, 0..2);
    }

    #[test]
    fn text_the_font_covers_reports_nothing_missing() {
        let text = "Prestación de servicios · 1.234,56 €";
        let run = shaper().shape(text);

        assert_eq!(run.missing(text), "");
    }

    #[test]
    fn a_character_the_font_lacks_is_named() {
        // Roboto has no geometric arrows. Without fallback they shape to
        // .notdef and print as an empty box — silently.
        let text = "▲ subida ▼ bajada";
        let run = shaper().shape(text);

        let missing = run.missing(text);
        assert!(missing.contains('▲'), "got {missing:?}");
        assert!(missing.contains('▼'), "got {missing:?}");
    }

    #[test]
    fn each_missing_character_is_named_once_however_often_it_appears() {
        let text = "▲▲▲▲▲";
        let run = shaper().shape(text);

        assert_eq!(run.missing(text), "▲");
    }

    #[test]
    fn text_is_black_unless_told_otherwise() {
        let line = shaper().break_lines("Total", Pt(7.0), Pt(400.0)).remove(0);

        assert_eq!(line.color(), Color::BLACK);
    }

    #[test]
    fn a_line_can_be_recoloured_without_reshaping() {
        let mut s = shaper();
        let navy = Color::parse_hex("#1F4E79").unwrap();

        let plain = s.break_lines("Total", Pt(7.0), Pt(400.0)).remove(0);
        let coloured = s
            .break_lines("Total", Pt(7.0), Pt(400.0))
            .remove(0)
            .with_color(navy);

        assert_eq!(coloured.color(), navy);
        assert_eq!(
            coloured.glyphs().collect::<Vec<_>>(),
            plain.glyphs().collect::<Vec<_>>(),
            "colour must not change the glyphs"
        );
    }

    #[test]
    fn both_paths_produce_black_text() {
        // The fast path builds its line by hand; it must not forget a field.
        let mut s = shaper();
        let fast = s.break_lines("Total", Pt(7.0), Pt(400.0)).remove(0);
        let slow = s.break_lines("Total\n", Pt(7.0), Pt(400.0)).remove(0);

        assert_eq!(fast.color(), Color::BLACK);
        assert_eq!(slow.color(), Color::BLACK);
    }

    #[test]
    fn a_line_carries_the_size_it_was_set_at() {
        let line = shaper().break_lines("Total", Pt(7.0), Pt(400.0)).remove(0);

        assert_eq!(line.size, Pt(7.0));
    }

    #[test]
    fn a_line_carries_the_text_its_glyphs_index_into() {
        let lines = shaper().break_lines(PROSE, Pt(7.0), Pt(60.0));

        for line in &lines {
            assert_eq!(&*line.text, PROSE, "every line shares the source");
            for g in &line.glyphs().cloned().collect::<Vec<_>>() {
                assert!(!g.text_range.is_empty(), "a glyph maps to no text");
                assert!(
                    line.text
                        .get(g.text_range.start as usize..g.text_range.end as usize)
                        .is_some(),
                    "range {:?} slices mid-codepoint",
                    g.text_range
                );
            }
        }
    }

    // ── the cache ───────────────────────────────────────────────────────

    #[test]
    fn a_first_shaping_is_a_miss() {
        let mut s = shaper();
        s.shape("Factura venta");

        assert_eq!(s.misses(), 1);
        assert_eq!(s.hits(), 0);
    }

    #[test]
    fn repeating_a_string_is_served_from_the_cache() {
        // The ledger case: the same label on every one of 40,000 rows.
        let mut s = shaper();
        for _ in 0..100 {
            s.shape("Factura venta");
        }

        assert_eq!(s.misses(), 1);
        assert_eq!(s.hits(), 99);
    }

    #[test]
    fn a_cached_run_equals_the_one_that_was_computed() {
        let mut s = shaper();

        let first = s.shape("430000");
        let second = s.shape("430000");

        assert_eq!(first, second);
    }

    #[test]
    fn different_strings_are_separate_entries() {
        let mut s = shaper();
        s.shape("430001");
        s.shape("430002");

        assert_eq!(s.misses(), 2);
        assert_eq!(s.hits(), 0);
    }

    // ── line breaking ───────────────────────────────────────────────────

    /// A real cell value from a ledger's description column.
    const PROSE: &str = "Prestación de servicios profesionales periodo 3";

    // ── the single-line fast path ───────────────────────────────────────
    // A table cell is short, fits on one line, and repeats across thousands
    // of rows. Sending it through a full layout throws away the cache: on
    // 56,000 real ledger cells, cached shaping was 2.9x faster at a 70% hit
    // rate. Both paths must produce the same line, or a document would
    // change depending on which one it happened to take.

    #[test]
    fn a_short_line_that_fits_is_served_from_the_shaping_cache() {
        let mut s = shaper();
        for _ in 0..100 {
            s.break_lines("Factura venta", Pt(7.0), Pt(400.0));
        }

        assert_eq!(s.misses(), 1);
        assert_eq!(s.hits(), 99);
    }

    #[test]
    fn the_fast_path_produces_the_same_line_as_a_full_layout() {
        // The guard that makes the shortcut safe. `\n` forces the slow path,
        // so the same text with a trailing newline is the control.
        for size in [7.0, 11.0, 24.0] {
            let mut s = shaper();
            let fast = s
                .break_lines("Total asiento", Pt(size), Pt(400.0))
                .remove(0);
            let slow = s
                .break_lines("Total asiento\n", Pt(size), Pt(400.0))
                .remove(0);

            let ids: Vec<u32> = fast.glyphs().map(|g| g.id).collect();
            let slow_ids: Vec<u32> = slow.glyphs().map(|g| g.id).collect();
            assert_eq!(ids, slow_ids, "different glyphs at {size}pt");

            for (f, sl) in fast.glyphs().zip(slow.glyphs()) {
                assert!(
                    (f.x_advance - sl.x_advance).abs() < 0.01,
                    "advance {} vs {} at {size}pt",
                    f.x_advance,
                    sl.x_advance
                );
                assert_eq!(f.text_range, sl.text_range);
            }
            assert!(
                (fast.width.get() - slow.width.get()).abs() < 0.05,
                "width at {size}pt"
            );
            assert!(
                (fast.height.get() - slow.height.get()).abs() < 1e-3,
                "height at {size}pt"
            );
            assert!(
                (fast.baseline.get() - slow.baseline.get()).abs() < 1e-3,
                "baseline at {size}pt: {} vs {}",
                fast.baseline.get(),
                slow.baseline.get()
            );
        }
    }

    #[test]
    fn text_containing_a_newline_never_takes_the_fast_path() {
        let lines = shaper().break_lines("Debe\nHaber", Pt(7.0), Pt(400.0));

        assert_eq!(lines.len(), 2, "the shortcut swallowed a forced break");
    }

    #[test]
    fn text_too_wide_for_the_column_never_takes_the_fast_path() {
        let lines = shaper().break_lines(PROSE, Pt(7.0), Pt(60.0));

        assert!(lines.len() > 1, "the shortcut ignored the column width");
    }

    #[test]
    fn a_fast_path_line_carries_its_text_and_size_like_any_other() {
        let line = shaper().break_lines("Total", Pt(7.0), Pt(400.0)).remove(0);

        assert_eq!(&*line.text, "Total");
        assert_eq!(line.size, Pt(7.0));
        assert!(!line.glyphs().cloned().collect::<Vec<_>>().is_empty());
    }

    #[test]
    fn text_that_fits_stays_on_one_line() {
        let lines = shaper().break_lines(PROSE, Pt(7.0), Pt(400.0));

        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn text_wider_than_the_column_is_broken_up() {
        let lines = shaper().break_lines(PROSE, Pt(7.0), Pt(40.0));

        assert!(lines.len() > 1, "got {} line(s)", lines.len());
    }

    #[test]
    fn no_line_overflows_a_column_wide_enough_for_its_longest_word() {
        // 60pt clears the widest word in PROSE ("profesionales", 41.6pt at
        // 7pt). A narrower column cannot honour this and must not pretend to
        // — see the overflow test below.
        let max = Pt(60.0);
        let lines = shaper().break_lines(PROSE, Pt(7.0), max);

        assert!(lines.len() > 1, "the sample must actually break");
        for (i, line) in lines.iter().enumerate() {
            assert!(
                line.width.get() <= max.get(),
                "line {i} is {}pt wide, column is {}pt",
                line.width.get(),
                max.get()
            );
        }
    }

    #[test]
    fn a_word_wider_than_the_column_overflows_rather_than_being_chopped() {
        // CSS `overflow-wrap: normal`, and the right default: silently
        // splitting a word mid-glyph produces a document that reads wrong,
        // which is worse than one that looks wrong. The engine will report
        // this as a build diagnostic rather than hide it.
        let word = "profesionales";
        let mut s = shaper();
        let natural = s.shape(word).width_at(Pt(7.0));

        let lines = s.break_lines(word, Pt(7.0), Pt(20.0));

        assert_eq!(lines.len(), 1, "there is no break opportunity to take");
        assert!(
            lines[0].width.get() > 20.0,
            "the line should overflow, not shrink"
        );
        assert!((lines[0].width.get() - natural.get()).abs() < 0.5);
    }

    #[test]
    fn a_narrower_column_never_produces_fewer_lines() {
        let mut s = shaper();

        let wide = s.break_lines(PROSE, Pt(7.0), Pt(120.0)).len();
        let narrow = s.break_lines(PROSE, Pt(7.0), Pt(40.0)).len();

        assert!(narrow >= wide, "narrow {narrow} vs wide {wide}");
    }

    #[test]
    fn a_larger_font_never_produces_fewer_lines_in_the_same_column() {
        let mut s = shaper();

        let small = s.break_lines(PROSE, Pt(7.0), Pt(80.0)).len();
        let large = s.break_lines(PROSE, Pt(14.0), Pt(80.0)).len();

        assert!(large >= small, "14pt {large} vs 7pt {small}");
    }

    #[test]
    fn a_word_too_long_for_the_column_is_still_laid_out() {
        // Must not hang looking for a break that does not exist, and must not
        // silently drop the content. Overflowing is the author's problem;
        // losing their text is ours.
        let lines = shaper().break_lines("Contabilización", Pt(12.0), Pt(8.0));

        assert!(!lines.is_empty());
        assert!(lines.iter().any(|l| !l.is_empty()), "text was lost");
    }

    #[test]
    fn empty_text_produces_no_lines() {
        // No line means no atom means no height — an empty cell must not
        // silently inflate its row.
        assert!(shaper().break_lines("", Pt(7.0), Pt(100.0)).is_empty());
    }

    #[test]
    fn an_explicit_newline_forces_a_break_even_with_room_to_spare() {
        let lines = shaper().break_lines("Debe\nHaber", Pt(7.0), Pt(400.0));

        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn a_line_box_is_taller_than_the_font_size_and_scales_with_it() {
        let mut s = shaper();

        let small = s.break_lines("Total", Pt(7.0), Pt(400.0))[0].height;
        let large = s.break_lines("Total", Pt(14.0), Pt(400.0))[0].height;

        assert!(small.get() > 7.0, "7pt text got a {}pt line", small.get());
        assert!(large.get() > small.get());
    }

    #[test]
    fn the_baseline_sits_inside_the_line_box() {
        let line = shaper().break_lines("Total", Pt(7.0), Pt(400.0)).remove(0);

        assert!(line.baseline.get() > 0.0);
        assert!(line.baseline.get() <= line.height.get());
    }

    #[test]
    fn breaking_is_deterministic() {
        let mut a = shaper();
        let mut b = shaper();

        assert_eq!(
            a.break_lines(PROSE, Pt(7.0), Pt(40.0)),
            b.break_lines(PROSE, Pt(7.0), Pt(40.0))
        );
    }

    #[test]
    fn every_glyph_of_the_source_survives_the_break() {
        // Breaking consumes the space at each break point, but nothing else
        // may go missing.
        let mut s = shaper();
        let unbroken = s.shape(PROSE).glyphs.len();

        let broken: usize = s
            .break_lines(PROSE, Pt(7.0), Pt(40.0))
            .iter()
            .map(|l| l.glyphs().count())
            .sum();

        let spaces = PROSE.matches(' ').count();
        assert!(
            broken >= unbroken - spaces,
            "{unbroken} glyphs became {broken} across lines"
        );
    }

    // ── faces ───────────────────────────────────────────────────────────

    #[test]
    fn bold_text_is_wider_than_regular_text() {
        let mut s = family();

        let regular = s.shape_in("Total asiento", Face::REGULAR).advance;
        let bold = s.shape_in("Total asiento", Face::BOLD).advance;

        assert!(bold > regular, "bold {bold} vs regular {regular}");
    }

    #[test]
    fn bold_is_different_glyphs_not_the_same_ones_drawn_heavier() {
        // If the shaper silently fell back to the regular face, the ids
        // would match and the page would print in the wrong weight without
        // a word of complaint.
        let mut s = family();

        let regular = s.shape_in("W", Face::REGULAR).glyphs[0].clone();
        let bold = s.shape_in("W", Face::BOLD).glyphs[0].clone();

        assert_ne!(
            regular.x_advance, bold.x_advance,
            "the bold face was not used"
        );
    }

    #[test]
    fn italic_differs_from_regular() {
        let mut s = family();

        let regular = s.shape_in("f", Face::REGULAR).advance;
        let italic = s.shape_in("f", Face::ITALIC).advance;

        assert_ne!(regular, italic, "the italic face was not used");
    }

    #[test]
    fn the_face_is_part_of_the_cache_key() {
        let mut s = family();
        s.shape_in("Total", Face::REGULAR);
        s.shape_in("Total", Face::BOLD);

        assert_eq!(s.misses(), 2, "bold reused the regular entry");
        assert_eq!(s.hits(), 0);
    }

    #[test]
    fn the_same_word_in_the_same_face_still_hits_the_cache() {
        let mut s = family();
        for _ in 0..10 {
            s.shape_in("Total", Face::BOLD);
        }

        assert_eq!(s.misses(), 1);
        assert_eq!(s.hits(), 9);
    }

    #[test]
    fn a_line_records_the_face_it_was_set_in() {
        // The painter reads this to pick which font file to embed.
        let mut s = family();

        let line = s
            .break_lines_in("Total", Pt(9.0), Pt(400.0), Face::BOLD)
            .remove(0);

        assert_eq!(line.face(), Face::BOLD);
    }

    #[test]
    fn a_shaper_with_one_face_still_answers_for_the_others() {
        // A document that asks for bold from a regular-only shaper gets
        // regular rather than nothing. Substitution, not a blank page.
        let mut s = shaper();

        let line = s.break_lines_in("Total", Pt(9.0), Pt(400.0), Face::BOLD);

        assert_eq!(line.len(), 1);
        assert!(!line[0].is_empty());
    }

    #[test]
    fn every_registered_face_can_be_looked_up_by_the_painter() {
        let s = family();

        for face in [Face::REGULAR, Face::BOLD, Face::ITALIC] {
            assert!(s.face_bytes(face).is_some(), "{face:?} was not registered");
        }
        assert_eq!(s.faces().count(), 3);
    }

    // ── rich inline text ────────────────────────────────────────────────
    // A sentence that turns bold or changes ink part-way through. Shaped as
    // one layout, not one per stretch: kerning crosses a style boundary,
    // Arabic letters join across it, and a break may fall inside a bold
    // phrase.

    fn runs(parts: &[(&str, Face)]) -> Vec<TextRun> {
        parts
            .iter()
            .map(|(t, f)| TextRun::new(*t).in_face(*f))
            .collect()
    }

    #[test]
    fn a_paragraph_of_one_style_is_a_single_stretch() {
        let mut s = family();

        let line = s
            .break_rich(
                &runs(&[("Total asiento", Face::REGULAR)]),
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        assert_eq!(line.segments.len(), 1);
        assert_eq!(line.face(), Face::REGULAR);
    }

    #[test]
    fn a_style_change_splits_the_line_into_stretches() {
        let mut s = family();

        let line = s
            .break_rich(
                &runs(&[
                    ("Total ", Face::REGULAR),
                    ("a pagar", Face::BOLD),
                    (" hoy", Face::REGULAR),
                ]),
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        let faces: Vec<Face> = line.segments.iter().map(|s| s.face).collect();
        assert_eq!(faces, vec![Face::REGULAR, Face::BOLD, Face::REGULAR]);
    }

    #[test]
    fn each_stretch_keeps_the_colour_it_was_given() {
        let navy = Color::parse_hex("#1F4E79").unwrap();
        let mut s = family();

        let line = s
            .break_rich(
                &[
                    TextRun::new("normal "),
                    TextRun::new("destacado").inked(navy),
                ],
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        assert_eq!(line.segments[0].color, Color::BLACK);
        assert_eq!(line.segments[1].color, navy);
    }

    #[test]
    fn stretches_are_laid_left_to_right_without_gaps() {
        let mut s = family();

        let line = s
            .break_rich(
                &runs(&[("uno ", Face::REGULAR), ("dos", Face::BOLD)]),
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        assert_eq!(line.segments[0].x, Pt(0.0));
        let first = line.segments[0].width.get();
        assert!(
            (line.segments[1].x.get() - first).abs() < 0.01,
            "the second stretch starts at {} but the first ends at {first}",
            line.segments[1].x.get()
        );
    }

    #[test]
    fn a_bold_stretch_measures_wider_than_the_same_words_plain() {
        let mut s = family();

        let plain = s
            .break_rich(
                &runs(&[("importe total", Face::REGULAR)]),
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);
        let mixed = s
            .break_rich(
                &runs(&[("importe ", Face::REGULAR), ("total", Face::BOLD)]),
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        assert!(
            mixed.width.get() > plain.width.get(),
            "the bold stretch was not shaped bold"
        );
    }

    #[test]
    fn a_break_inside_a_styled_stretch_keeps_the_style_on_both_lines() {
        // The case that a per-stretch shaper gets wrong: the phrase is bold
        // before the break and must stay bold after it.
        let mut s = family();

        let lines = s.break_rich(
            &runs(&[
                ("Nota: ", Face::REGULAR),
                (
                    "prestación de servicios profesionales durante el periodo indicado",
                    Face::BOLD,
                ),
            ]),
            Pt(9.0),
            Pt(120.0),
        );

        assert!(lines.len() > 1, "the sample must break");
        let last = lines.last().unwrap();
        assert!(
            last.segments.iter().all(|s| s.face == Face::BOLD),
            "the continuation lost its weight"
        );
    }

    #[test]
    fn the_whole_paragraph_shares_one_source_string() {
        // Glyph ranges index into it, so the painter needs no re-basing and
        // the ToUnicode map stays correct across a style change.
        let mut s = family();

        let line = s
            .break_rich(
                &runs(&[("uno ", Face::REGULAR), ("dos", Face::BOLD)]),
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        assert_eq!(&*line.text, "uno dos");
        for g in line.glyphs() {
            assert!(
                line.text
                    .get(g.text_range.start as usize..g.text_range.end as usize)
                    .is_some(),
                "range {:?} does not slice the source",
                g.text_range
            );
        }
    }

    #[test]
    fn every_glyph_of_a_styled_line_names_the_letter_it_actually_draws() {
        // The failure this catches is invisible on paper. Each glyph carries
        // the byte range it came from, and that range is what becomes the
        // PDF's ToUnicode map — so a document can look flawless and copy out
        // as "uno uno". It went wrong at exactly one place: a line that
        // changes style is several glyph runs over *one* shaped run, and
        // walking that run's clusters afresh for each of them starts the
        // second stretch back at the first letter of the first.
        //
        // Reading the source back through the ranges is the whole test: if
        // they name the right letters, the map is right.
        //
        // Two stretches in the same face and different ink, which is the
        // shape that actually breaks it. A bold stretch is a different font,
        // so parley makes it its own run and the walk restarts correctly by
        // accident; a red stretch of the same face is one run split in two by
        // the brush, and that is the case nothing else covers.
        let mut s = family();

        let line = s
            .break_rich(
                &[
                    TextRun::new("Total ").inked(Color::BLACK),
                    TextRun::new("1.234,00").inked(Color::rgb(200, 0, 0)),
                ],
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        let mut rebuilt = String::new();
        let mut previous: Option<Range<u32>> = None;
        for glyph in line.glyphs() {
            // Several glyphs can share one cluster — a decomposed accent —
            // and the cluster's characters are named once.
            if previous.as_ref() == Some(&glyph.text_range) {
                continue;
            }
            rebuilt.push_str(
                line.text
                    .get(glyph.text_range.start as usize..glyph.text_range.end as usize)
                    .expect("a glyph range must slice its own source"),
            );
            previous = Some(glyph.text_range.clone());
        }

        assert_eq!(rebuilt, *line.text, "the glyphs spell something else");
    }

    #[test]
    fn an_entirely_empty_paragraph_produces_no_lines() {
        let mut s = family();

        assert!(
            s.break_rich(&[TextRun::new(""), TextRun::new("")], Pt(9.0), Pt(400.0))
                .is_empty()
        );
    }

    #[test]
    fn an_empty_stretch_between_two_others_changes_nothing() {
        let mut s = family();

        let with_gap = s
            .break_rich(
                &[
                    TextRun::new("uno "),
                    TextRun::new(""),
                    TextRun::new("dos").bold(),
                ],
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);
        let without = s
            .break_rich(
                &[TextRun::new("uno "), TextRun::new("dos").bold()],
                Pt(9.0),
                Pt(400.0),
            )
            .remove(0);

        assert!((with_gap.width.get() - without.width.get()).abs() < 0.01);
    }

    #[test]
    fn rich_and_plain_agree_when_there_is_only_one_style() {
        // The two entry points must not disagree, or the same paragraph
        // would measure differently depending on how it was expressed.
        let mut s = family();

        let rich = s
            .break_rich(&runs(&[(PROSE, Face::REGULAR)]), Pt(8.0), Pt(120.0))
            .into_iter()
            .map(|l| (l.width, l.height, l.glyphs().count()))
            .collect::<Vec<_>>();
        let plain = s
            .break_lines(PROSE, Pt(8.0), Pt(120.0))
            .into_iter()
            .map(|l| (l.width, l.height, l.glyphs().count()))
            .collect::<Vec<_>>();

        assert_eq!(rich, plain);
    }

    #[test]
    fn justifying_a_line_widens_its_spaces_and_nothing_else() {
        // The measurement that matters is the line's own width: justified, it
        // reaches the track exactly. Asserting that the spaces grew and the
        // letters did not is the other half — a naive implementation that
        // scaled every advance would also hit the width, and would set the
        // words in a font nobody chose.
        let mut line = shaper()
            .break_lines("uno dos tres", Pt(10.0), Pt(400.0))
            .remove(0);

        let letters: Vec<f32> = line
            .glyphs()
            .filter(|g| &line.text[g.text_range.start as usize..g.text_range.end as usize] != " ")
            .map(|g| g.x_advance)
            .collect();
        let ragged = line.width;

        line.justify(Pt(300.0));

        assert!((line.width.get() - 300.0).abs() < 0.01, "{:?}", line.width);
        assert!(
            ragged.get() < 300.0,
            "the sample has to have slack to take up"
        );

        let after: Vec<f32> = line
            .glyphs()
            .filter(|g| &line.text[g.text_range.start as usize..g.text_range.end as usize] != " ")
            .map(|g| g.x_advance)
            .collect();
        assert_eq!(letters, after, "a letter was stretched");
    }

    #[test]
    fn justifying_a_line_puts_its_last_visible_glyph_on_the_edge() {
        // `line.width` is the wrong thing to assert and passed while this was
        // broken: the line reached the track, with the trailing space taking
        // the last two and a half points and the text stopping before it.
        // Every line came up short by the same amount, so the edge was
        // straight — inset, but straight, which is the version of this bug
        // that survives being looked at.
        let mut lines = shaper().break_lines(
            "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece",
            Pt(10.0),
            Pt(120.0),
        );
        assert!(lines.len() > 1, "the sample has to wrap to mean anything");

        let visible_end = |line: &Line| {
            let mut x = 0.0f32;
            let mut visible = 0.0f32;
            for glyph in line.glyphs() {
                x += glyph.x_advance;
                let at = glyph.text_range.start as usize..glyph.text_range.end as usize;
                if !line.text[at].trim().is_empty() {
                    visible = x;
                }
            }
            visible
        };

        let line = &mut lines[0];
        assert!(
            line.glyphs().last().is_some_and(|glyph| {
                let at = glyph.text_range.start as usize..glyph.text_range.end as usize;
                line.text[at].chars().all(char::is_whitespace)
            }),
            "the sample has to end in a space to mean anything"
        );

        line.justify(Pt(120.0));

        let reached = visible_end(line);
        assert!(
            (reached - 120.0).abs() < 0.01,
            "the text stops at {reached}, short of the edge it was justified to"
        );
    }

    #[test]
    fn justifying_a_line_with_no_room_leaves_it_alone() {
        // A line already at the track — or past it, which a single long word
        // can be — has nothing to give away, and stretching backwards would
        // pull it off the left of its own box.
        let mut line = shaper()
            .break_lines("uno dos", Pt(10.0), Pt(400.0))
            .remove(0);
        let before = line.clone();

        line.justify(Pt(1.0));

        assert_eq!(line, before);
    }

    #[test]
    fn justifying_a_line_with_no_spaces_leaves_it_alone() {
        // Nowhere to put the slack. Letter-spacing it would be a different
        // typographic decision and not one to make on somebody's behalf.
        let mut line = shaper()
            .break_lines("supercalifragilistico", Pt(10.0), Pt(400.0))
            .remove(0);
        let before = line.clone();

        line.justify(Pt(380.0));

        assert_eq!(line, before);
    }
}
