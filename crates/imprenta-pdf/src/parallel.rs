//! Measuring many blocks at once.
//!
//! Measuring is the expensive phase — 69% of a ledger render — and each block
//! is independent of every other, so it parallelises. Packing does not: it is
//! the one pass that walks the document in order, and that ordering is what
//! makes running totals and page-boundary decisions possible. Painting does
//! not either, because krilla's `Document::start_page` borrows the document
//! for the lifetime of the page; that one is a limitation of the writer
//! rather than of the design, and is worth revisiting upstream.
//!
//! Each worker gets its own [`Shaper`], and therefore its own cache. Sharing
//! one behind a lock would serialise the very thing being parallelised, and a
//! per-worker cache still sees most of the repetition: work is handed out in
//! contiguous chunks, and a ledger repeats its labels everywhere, not in one
//! corner.

use crate::measure::{Measured, TextStyle, measure_text_in};
use crate::shape::{Face, Shaper};
use imprenta_core::units::Pt;
use rayon::prelude::*;

/// One block of text to measure.
pub struct Block<'a> {
    pub text: &'a str,
    pub style: TextStyle,
    pub width: Pt,
    pub face: Face,
}

impl<'a> Block<'a> {
    pub fn new(text: &'a str, style: TextStyle, width: Pt) -> Self {
        Self {
            text,
            style,
            width,
            face: Face::REGULAR,
        }
    }

    pub fn in_face(mut self, face: Face) -> Self {
        self.face = face;
        self
    }
}

/// The faces a worker registers. Cloned per worker, so keep it to the faces
/// the document actually uses.
pub type Faces = Vec<(Face, Vec<u8>)>;

/// Measures every block, in parallel, preserving order.
///
/// `font` is cloned per worker: a `Shaper` owns parley contexts that are not
/// shareable, and building one is cheap next to the work it then does.
pub fn measure_all(font: &[u8], blocks: &[Block<'_>]) -> Vec<Measured> {
    measure_all_in(&vec![(Face::REGULAR, font.to_vec())], blocks)
}

/// Measures every block against a whole family, in parallel, in order.
pub fn measure_all_in(faces: &Faces, blocks: &[Block<'_>]) -> Vec<Measured> {
    let build = || Shaper::with_faces(faces.iter().cloned());

    if blocks.len() < PARALLEL_THRESHOLD {
        let mut shaper = build();
        return blocks
            .iter()
            .map(|b| measure_text_in(&mut shaper, b.text, b.style, b.width, b.face))
            .collect();
    }

    blocks
        .par_chunks(chunk_size(blocks.len()))
        .flat_map_iter(|chunk| {
            let mut shaper = build();
            chunk
                .iter()
                .map(|b| measure_text_in(&mut shaper, b.text, b.style, b.width, b.face))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Below this, building a `Shaper` per worker costs more than it saves.
const PARALLEL_THRESHOLD: usize = 256;

/// Chunks large enough that each worker's cache warms up, and numerous enough
/// that rayon can still balance the load.
pub(crate) fn chunk_size(total: usize) -> usize {
    let workers = rayon::current_num_threads().max(1);
    (total / (workers * 4)).max(64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn blocks(count: usize) -> Vec<String> {
        (0..count)
            .map(|i| format!("Prestación de servicios profesionales, asiento {i}"))
            .collect()
    }

    fn as_blocks(texts: &[String], width: f32) -> Vec<Block<'_>> {
        texts
            .iter()
            .map(|t| Block::new(t, TextStyle::new(Pt(8.0)), Pt(width)))
            .collect()
    }

    /// The same work done one block at a time.
    fn sequential(texts: &[String], width: f32) -> Vec<Measured> {
        let mut shaper = Shaper::new(ROBOTO.to_vec());
        texts
            .iter()
            .map(|t| {
                measure_text_in(
                    &mut shaper,
                    t,
                    TextStyle::new(Pt(8.0)),
                    Pt(width),
                    Face::REGULAR,
                )
            })
            .collect()
    }

    #[test]
    fn nothing_to_measure_is_not_an_error() {
        assert!(measure_all(ROBOTO, &[]).is_empty());
    }

    #[test]
    fn every_block_comes_back() {
        let texts = blocks(1000);

        assert_eq!(measure_all(ROBOTO, &as_blocks(&texts, 400.0)).len(), 1000);
    }

    #[test]
    fn results_keep_the_order_they_were_given_in() {
        // Work is handed out in chunks across threads; if the results came
        // back interleaved, every row of a ledger would land on the wrong
        // line and nothing would look obviously wrong.
        let texts = blocks(1000);

        let parallel = measure_all(ROBOTO, &as_blocks(&texts, 400.0));

        for (i, m) in parallel.iter().enumerate() {
            assert_eq!(
                &*m.lines[0].text,
                texts[i].as_str(),
                "block {i} came back out of order"
            );
        }
    }

    #[test]
    fn measuring_in_parallel_gives_the_same_answer_as_measuring_in_order() {
        // Per-worker caches must not change a single measurement, or a
        // document would depend on how many cores rendered it.
        let texts = blocks(1000);

        let parallel = measure_all(ROBOTO, &as_blocks(&texts, 90.0));
        let serial = sequential(&texts, 90.0);

        assert_eq!(parallel.len(), serial.len());
        for (i, (p, s)) in parallel.iter().zip(&serial).enumerate() {
            assert_eq!(p.atoms, s.atoms, "block {i} measured to different atoms");
            assert_eq!(p.lines, s.lines, "block {i} measured to different lines");
        }
    }

    #[test]
    fn wrapped_blocks_survive_the_split_intact() {
        let texts = blocks(600);

        let parallel = measure_all(ROBOTO, &as_blocks(&texts, 70.0));

        assert!(
            parallel.iter().all(|m| m.len() > 1),
            "the narrow column should have wrapped every block"
        );
    }

    #[test]
    fn a_small_batch_takes_the_sequential_path_and_still_measures() {
        let texts = blocks(10);

        let small = measure_all(ROBOTO, &as_blocks(&texts, 400.0));

        assert_eq!(small.len(), 10);
        assert_eq!(small, sequential(&texts, 400.0));
    }

    #[test]
    fn chunks_are_never_degenerate() {
        for total in [256, 1_000, 100_000] {
            let size = chunk_size(total);
            assert!(size >= 64, "{total} blocks gave chunks of {size}");
            assert!(size <= total, "{total} blocks gave chunks of {size}");
        }
    }

    #[test]
    fn a_block_can_name_its_own_face() {
        const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
        let faces: Faces = vec![
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, BOLD.to_vec()),
        ];
        let texts = blocks(400);
        let mut blocks: Vec<Block<'_>> = texts
            .iter()
            .map(|t| Block::new(t, TextStyle::new(Pt(8.0)), Pt(400.0)))
            .collect();
        for b in blocks.iter_mut().step_by(2) {
            b.face = Face::BOLD;
        }

        let measured = measure_all_in(&faces, &blocks);

        assert_eq!(measured[0].lines[0].face(), Face::BOLD);
        assert_eq!(measured[1].lines[0].face(), Face::REGULAR);
    }

    #[test]
    fn faces_survive_the_split_across_workers() {
        // Each worker builds its own shaper; one that forgot a face would
        // silently fall back and half the document would lose its weight.
        const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
        let faces: Faces = vec![
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, BOLD.to_vec()),
        ];
        let texts = blocks(2000);
        let blocks: Vec<Block<'_>> = texts
            .iter()
            .map(|t| Block::new(t, TextStyle::new(Pt(8.0)), Pt(400.0)).in_face(Face::BOLD))
            .collect();

        let measured = measure_all_in(&faces, &blocks);

        assert!(
            measured.iter().all(|m| m.lines[0].face() == Face::BOLD),
            "a worker lost the bold face"
        );
    }
}
