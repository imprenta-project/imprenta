//! Embedding a typeface, and only the part of it a document used.
//!
//! # The one thing that is easy to get wrong
//!
//! A subset font renumbers its glyphs: Roboto's `P` is glyph 51, and in a
//! subset that contains eleven letters it might be glyph 1. So the content
//! stream cannot name the glyph the shaper found — it has to name the glyph
//! the *subset* will have, and the subset is not built until the document
//! closes.
//!
//! It is resolved by deciding the numbering first and building the font to
//! match. [`subsetter::GlyphRemapper`] hands out subset ids in the order
//! glyphs are first asked for, which is an order the painter can produce as
//! it goes, and the subset built from it at the end agrees by construction.
//!
//! # And the one that is silent
//!
//! Nothing in a PDF says what a glyph *means*. Without a `ToUnicode` map the
//! page renders perfectly and its text cannot be copied, searched, indexed or
//! read aloud — a defect that survives every visual check there is. The map
//! is built from the byte ranges the shaper recorded against each glyph, and
//! `imprenta-pdf` has its own tests for those ranges being right, because
//! this is where being wrong stops being visible.

use crate::WriteError;
use pdf_writer::{Chunk, Filter, Finish, Name, Rect, Ref, Str};
use skrifa::MetadataProvider;
use skrifa::raw::TableProvider;
use std::collections::BTreeMap;

/// Which registered typeface, as the writer numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaceId(pub(crate) usize);

/// One positioned glyph, as the shaper produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub id: u16,
    /// How far the pen moves after it, in points. Shaped rather than read off
    /// the font, so kerning is in it.
    pub x_advance: f32,
    /// The bytes of the source string this glyph stands for.
    pub text: std::ops::Range<usize>,
}

/// The object numbers one embedded face occupies.
pub(crate) struct Refs {
    pub font: Ref,
    pub cid: Ref,
    pub descriptor: Ref,
    pub file: Ref,
    pub to_unicode: Ref,
}

/// A run of glyphs encoded for one `TJ`.
pub(crate) struct Run {
    /// Two bytes per glyph, big-endian, as Identity-H wants.
    pub encoded: Vec<u8>,
    /// `(index, points)` — after the glyph at `index`, the pen has to move a
    /// further `points` than the font's own width would take it.
    pub adjustments: Vec<(usize, f32)>,
}

/// A typeface the document may draw with.
pub(crate) struct Face {
    data: Vec<u8>,
    /// Assigned the first time the face is actually drawn with, so a face
    /// that was registered and never used is never embedded.
    pub reference: Option<Ref>,
    remapper: subsetter::GlyphRemapper,
    /// Subset id to the characters it stands for. First writing wins: the
    /// same glyph reached from two different strings is the same letter, and
    /// a ligature reached twice is the same pair of them.
    to_unicode: BTreeMap<u16, String>,
    units_per_em: f32,
    /// Advance widths in font units, indexed by the original glyph id.
    advances: Vec<u16>,
    metrics: Metrics,
}

/// What a `/FontDescriptor` has to say about a face.
#[derive(Debug, Clone, Copy, Default)]
struct Metrics {
    bbox: [f32; 4],
    ascent: f32,
    descent: f32,
    cap_height: f32,
    italic_angle: f32,
    /// Set for a face whose outlines are CFF rather than `glyf`, because the
    /// two are embedded under different subtypes and a reader that is told
    /// the wrong one shows nothing at all.
    cff: bool,
}

impl Face {
    pub fn new(data: Vec<u8>) -> Result<Self, WriteError> {
        let font = skrifa::FontRef::new(&data).map_err(|_| WriteError::UnreadableFont)?;
        let upem = f32::from(
            font.head()
                .map_err(|_| WriteError::UnreadableFont)?
                .units_per_em(),
        );

        // Read once, into a plain vector. A `W` array wants the width of
        // every glyph the document used, and looking each one up through the
        // font's tables while writing means parsing them again per glyph.
        let location = skrifa::instance::LocationRef::default();
        let glyph_metrics = font.glyph_metrics(skrifa::instance::Size::unscaled(), location);
        let count: u16 = font.maxp().map(|m| m.num_glyphs()).unwrap_or(0);
        let advances: Vec<u16> = (0..count)
            .map(|id: u16| {
                glyph_metrics
                    .advance_width(skrifa::GlyphId::new(u32::from(id)))
                    .unwrap_or(0.0) as u16
            })
            .collect();

        let head = font.head().map_err(|_| WriteError::UnreadableFont)?;
        let hhea = font.hhea().ok();
        let os2 = font.os2().ok();
        let post = font.post().ok();
        let metrics = Metrics {
            bbox: [
                f32::from(head.x_min()),
                f32::from(head.y_min()),
                f32::from(head.x_max()),
                f32::from(head.y_max()),
            ],
            ascent: hhea
                .as_ref()
                .map_or(upem * 0.8, |h| f32::from(h.ascender().to_i16())),
            descent: hhea
                .as_ref()
                .map_or(-upem * 0.2, |h| f32::from(h.descender().to_i16())),
            cap_height: os2
                .as_ref()
                .and_then(|o| o.s_cap_height())
                .map_or(upem * 0.7, f32::from),
            italic_angle: post.map_or(0.0, |p| p.italic_angle().to_f32()),
            cff: font.table_data(skrifa::Tag::new(b"CFF ")).is_some()
                || font.table_data(skrifa::Tag::new(b"CFF2")).is_some(),
        };

        let mut remapper = subsetter::GlyphRemapper::new();
        // `.notdef` is glyph zero by definition and has to stay glyph zero in
        // the subset, so it is claimed before anything else can take the slot.
        remapper.remap(0);

        Ok(Self {
            data,
            reference: None,
            remapper,
            to_unicode: BTreeMap::new(),
            units_per_em: upem,
            advances,
            metrics,
        })
    }

    /// A face with nothing in it, to leave behind when the real one is taken
    /// out to be written.
    pub fn empty() -> Self {
        Self {
            data: Vec::new(),
            reference: None,
            remapper: subsetter::GlyphRemapper::new(),
            to_unicode: BTreeMap::new(),
            units_per_em: 1000.0,
            advances: Vec::new(),
            metrics: Metrics::default(),
        }
    }

    /// Turns a run of shaped glyphs into the bytes a `TJ` shows, claiming a
    /// subset id for each glyph it has not seen before.
    pub fn encode(&mut self, glyphs: &[Glyph], text: &str, size: f32) -> Run {
        let mut encoded = Vec::with_capacity(glyphs.len() * 2);
        let mut adjustments = Vec::new();

        for (index, glyph) in glyphs.iter().enumerate() {
            let cid = self.remapper.remap(glyph.id);
            encoded.extend_from_slice(&cid.to_be_bytes());

            if let Some(source) = text.get(glyph.text.clone())
                && !source.is_empty()
            {
                self.to_unicode
                    .entry(cid)
                    .or_insert_with(|| source.to_string());
            }

            // What the viewer will do on its own, against what the shaper
            // asked for. They differ wherever a pair kerned.
            let natural = f32::from(
                self.advances
                    .get(usize::from(glyph.id))
                    .copied()
                    .unwrap_or(0),
            ) / self.units_per_em
                * size;
            let difference = glyph.x_advance - natural;
            // A twentieth of a point at ten point text is a thousandth of an
            // em: below the rounding of the numbers being compared, and not
            // worth an operand per glyph.
            if difference.abs() > 0.001 {
                adjustments.push((index, difference));
            }
        }

        Run {
            encoded,
            adjustments,
        }
    }
}

/// Writes the five objects a CID font occupies.
pub(crate) fn write(
    out: &mut Chunk,
    face: &Face,
    refs: Refs,
    compress: bool,
) -> Result<(), WriteError> {
    let subset = subsetter::subset(&face.data, 0, &face.remapper)
        .map_err(|e| WriteError::Subset(e.to_string()))?;

    // Six upper-case letters and a plus, as the format requires. Derived from
    // the subset rather than counted, so the same document always produces
    // the same name and two documents that used different letters of the same
    // face cannot be mistaken for each other by a reader that caches by name.
    let tag = subset_tag(&subset);
    let base = format!("{tag}+Imprenta");

    out.type0_font(refs.font)
        .base_font(Name(base.as_bytes()))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(refs.cid)
        .to_unicode(refs.to_unicode);

    let scale = 1000.0 / face.units_per_em;
    {
        let mut cid = out.cid_font(refs.cid);
        cid.subtype(if face.metrics.cff {
            pdf_writer::types::CidFontType::Type0
        } else {
            pdf_writer::types::CidFontType::Type2
        });
        cid.base_font(Name(base.as_bytes()));
        cid.system_info(pdf_writer::types::SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        });
        cid.font_descriptor(refs.descriptor);
        cid.default_width(0.0);
        if !face.metrics.cff {
            // Identity, and only meaningful for a TrueType descendant: the
            // subsetter already made the subset id the glyph id.
            cid.cid_to_gid_map_predefined(Name(b"Identity"));
        }
        {
            // One contiguous run from CID zero. The subset ids are dense by
            // construction, so the sparse form would only be longer.
            let widths: Vec<f32> = face
                .remapper
                .remapped_gids()
                .map(|original| {
                    f32::from(
                        face.advances
                            .get(usize::from(original))
                            .copied()
                            .unwrap_or(0),
                    ) * scale
                })
                .collect();
            let mut w = cid.widths();
            w.consecutive(0, widths);
            w.finish();
        }
        cid.finish();
    }

    {
        let m = &face.metrics;
        let mut descriptor = out.font_descriptor(refs.descriptor);
        descriptor.name(Name(base.as_bytes()));
        // Symbolic, which is what a PDF calls a font whose encoding is its
        // own business — which every Identity-H font's is.
        descriptor.flags(pdf_writer::types::FontFlags::SYMBOLIC);
        descriptor.bbox(Rect::new(
            m.bbox[0] * scale,
            m.bbox[1] * scale,
            m.bbox[2] * scale,
            m.bbox[3] * scale,
        ));
        descriptor.italic_angle(m.italic_angle);
        descriptor.ascent(m.ascent * scale);
        descriptor.descent(m.descent * scale);
        descriptor.cap_height(m.cap_height * scale);
        // Required and unused: no reader lays out with it, and there is no
        // way to derive the real one short of measuring stems in outlines.
        descriptor.stem_v(80.0);
        if m.cff {
            descriptor.font_file3(refs.file);
        } else {
            descriptor.font_file2(refs.file);
        }
        descriptor.finish();
    }

    {
        let deflated;
        let bytes = if compress {
            deflated = miniz_oxide::deflate::compress_to_vec_zlib(&subset, 6);
            deflated.as_slice()
        } else {
            subset.as_slice()
        };
        let mut file = out.stream(refs.file, bytes);
        if compress {
            file.filter(Filter::FlateDecode);
        }
        if face.metrics.cff {
            file.pair(Name(b"Subtype"), Name(b"OpenType"));
        }
        file.finish();
    }

    let cmap = to_unicode_cmap(&face.to_unicode);
    out.stream(refs.to_unicode, cmap.as_bytes()).finish();

    Ok(())
}

/// The six-letter tag that marks a font programme as a subset.
///
/// Derived from the bytes rather than allocated from a counter, so the same
/// document always produces the same file — which is what makes a golden PDF
/// worth diffing in CI.
fn subset_tag(subset: &[u8]) -> String {
    // FNV-1a over the subset. Nothing here needs a strong hash: two different
    // subsets colliding would produce two fonts with the same tag, which a
    // reader tolerates because the tag is only a hint.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in subset {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    (0..6)
        .map(|i| char::from(b'A' + ((hash >> (i * 5)) & 0x19) as u8))
        .collect()
}

/// The map from subset id to the characters it stands for.
///
/// Written uncompressed: it is a few kilobytes at most, and a reader that
/// cannot extract text is a defect worth being able to see in a hex dump.
fn to_unicode_cmap(entries: &BTreeMap<u16, String>) -> String {
    let mut out = String::with_capacity(entries.len() * 24 + 512);
    out.push_str(
        "/CIDInit /ProcSet findresource begin\n\
         12 dict begin\n\
         begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n\
         /CMapType 2 def\n\
         1 begincodespacerange\n\
         <0000> <FFFF>\n\
         endcodespacerange\n",
    );

    // A hundred at a time: the limit is in the specification, and a reader
    // that enforces it drops everything past the hundredth entry.
    let usable: Vec<(&u16, &String)> = entries.iter().filter(|(_, s)| !s.is_empty()).collect();
    for batch in usable.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", batch.len()));
        for (cid, text) in batch {
            out.push_str(&format!("<{cid:04X}> <"));
            for unit in text.encode_utf16() {
                out.push_str(&format!("{unit:04X}"));
            }
            out.push_str(">\n");
        }
        out.push_str("endbfchar\n");
    }

    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBOTO: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Regular.ttf");

    #[test]
    fn notdef_keeps_the_first_slot_whatever_is_drawn_first() {
        // Glyph zero is `.notdef` by definition, in the subset as much as in
        // the original. A subset that gave the slot to whichever letter
        // happened to be drawn first would make every missing character
        // render as that letter.
        let mut face = Face::new(ROBOTO.to_vec()).unwrap();

        let run = face.encode(
            &[Glyph {
                id: 51,
                x_advance: 6.0,
                text: 0..1,
            }],
            "P",
            9.0,
        );

        assert_eq!(
            run.encoded,
            vec![0x00, 0x01],
            "the first letter is not CID 1"
        );
    }

    #[test]
    fn the_same_glyph_keeps_the_same_subset_id() {
        let mut face = Face::new(ROBOTO.to_vec()).unwrap();
        let glyph = |id| Glyph {
            id,
            x_advance: 6.0,
            text: 0..1,
        };

        let first = face.encode(&[glyph(51), glyph(70)], "Pa", 9.0);
        let again = face.encode(&[glyph(70), glyph(51)], "aP", 9.0);

        assert_eq!(first.encoded, vec![0x00, 0x01, 0x00, 0x02]);
        assert_eq!(again.encoded, vec![0x00, 0x02, 0x00, 0x01]);
    }

    #[test]
    fn a_glyph_whose_advance_matches_the_font_needs_no_nudge() {
        // The common case by far, and the one that decides how large a
        // ledger's content streams are: an operand per glyph would be a
        // number per letter on every page.
        let mut face = Face::new(ROBOTO.to_vec()).unwrap();
        let natural = f32::from(face.advances[51]) / face.units_per_em * 9.0;

        let run = face.encode(
            &[Glyph {
                id: 51,
                x_advance: natural,
                text: 0..1,
            }],
            "P",
            9.0,
        );

        assert!(run.adjustments.is_empty(), "{:?}", run.adjustments);
    }

    #[test]
    fn a_kerned_pair_is_nudged_by_exactly_what_the_shaper_asked_for() {
        let mut face = Face::new(ROBOTO.to_vec()).unwrap();
        let natural = f32::from(face.advances[51]) / face.units_per_em * 9.0;

        let run = face.encode(
            &[Glyph {
                id: 51,
                x_advance: natural - 0.5,
                text: 0..1,
            }],
            "P",
            9.0,
        );

        assert_eq!(run.adjustments.len(), 1);
        assert!((run.adjustments[0].1 + 0.5).abs() < 0.001);
    }

    #[test]
    fn the_map_names_the_characters_and_not_the_bytes() {
        // A character outside the basic plane is two UTF-16 units and both
        // have to be written, or the text extracts as half of itself.
        let mut face = Face::new(ROBOTO.to_vec()).unwrap();
        face.to_unicode.insert(1, "ó".into());
        face.to_unicode.insert(2, "fi".into());

        let cmap = to_unicode_cmap(&face.to_unicode);

        assert!(cmap.contains("<0001> <00F3>"), "{cmap}");
        assert!(cmap.contains("<0002> <00660069>"), "{cmap}");
    }

    #[test]
    fn the_map_is_written_in_batches_a_reader_will_accept() {
        // A hundred entries per `beginbfchar` is the limit in the
        // specification, and a reader that enforces it silently drops the
        // rest — which is a document whose text half extracts.
        let mut entries = BTreeMap::new();
        for cid in 0..250u16 {
            entries.insert(cid, "a".to_string());
        }

        let cmap = to_unicode_cmap(&entries);

        assert_eq!(cmap.matches("beginbfchar").count(), 3);
        assert!(!cmap.contains("101 beginbfchar"));
    }
}
