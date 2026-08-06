//! The file as it is written: a chain of blocks that are never moved.
//!
//! # Why not a `Vec<u8>`
//!
//! Because of what growing one costs where this actually runs. A `Vec`
//! doubles: to reach thirty-two megabytes it allocates sixteen, asks for
//! thirty-two, copies, and frees the sixteen. For a moment forty-eight
//! megabytes are live, and the sixteen it gave back is a hole in the wrong
//! shape for the next request.
//!
//! On a normal heap that is untidy. Inside a WebAssembly module it is the
//! whole cost: linear memory only ever grows, so every hole left behind is
//! held for the life of the instance and the high-water mark is what a
//! service carries. Measured on a ten thousand page ledger, the output buffer
//! alone accounted for **fifty-seven megabytes of a seventy-nine megabyte
//! footprint** — for a twenty-two megabyte file.
//!
//! A block that is full is never touched again. Nothing is copied, nothing is
//! freed early, and the memory the writer holds is the file plus at most one
//! part-used block.
//!
//! Blocks start small and grow to a ceiling, because most documents are an
//! invoice: a fixed four megabyte block would make a twenty kilobyte file
//! cost four megabytes, and a fixed sixty-four kilobyte one would make a
//! ledger a list of six hundred blocks.

/// The largest a single block gets.
const CEILING: usize = 4 * 1024 * 1024;

/// The first block, and the smallest anything costs.
const FLOOR: usize = 8 * 1024;

#[derive(Default)]
pub(crate) struct Blocks {
    blocks: Vec<Vec<u8>>,
    len: usize,
}

impl Blocks {
    /// How many bytes have been written in all. This is the offset the next
    /// object will start at, which is what an xref entry records.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.blocks.last().is_none_or(|b| b.len() == b.capacity()) {
                let size = self
                    .blocks
                    .last()
                    .map_or(FLOOR, |b| (b.capacity() * 2).min(CEILING));
                self.blocks.push(Vec::with_capacity(size.max(rest.len())));
            }
            let block = self.blocks.last_mut().expect("just pushed");
            let room = block.capacity() - block.len();
            let take = room.min(rest.len());
            block.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
        self.len += bytes.len();
    }

    /// Everything written, in one piece.
    ///
    /// The one copy the whole design admits to, and it is made at exactly the
    /// right size — so where a `Vec` would have been holding three times the
    /// file at its worst, this holds twice it for the length of a `memcpy`,
    /// and each block is given back as it is drained.
    pub fn into_vec(mut self) -> Vec<u8> {
        if self.blocks.len() == 1 {
            // An invoice. Nothing to join, and the block is already the file.
            return self.blocks.pop().unwrap_or_default();
        }
        let mut out = Vec::with_capacity(self.len);
        for block in self.blocks.drain(..) {
            out.extend_from_slice(&block);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_goes_in_comes_out_whatever_the_block_boundaries_fall_on() {
        let mut blocks = Blocks::default();
        let mut expected = Vec::new();
        for n in 0..5_000u32 {
            let piece = format!("{n} 0 obj\n<< /Type /Page >>\nendobj\n");
            blocks.push(piece.as_bytes());
            expected.extend_from_slice(piece.as_bytes());
        }

        assert_eq!(blocks.len(), expected.len());
        assert_eq!(blocks.into_vec(), expected);
    }

    #[test]
    fn a_piece_larger_than_a_whole_block_still_fits() {
        // A page carrying a photograph is one object of several megabytes,
        // and it arrives as one slice.
        let mut blocks = Blocks::default();
        let big = vec![0xABu8; CEILING * 2 + 17];

        blocks.push(b"header");
        blocks.push(&big);
        blocks.push(b"tail");

        let out = blocks.into_vec();
        assert_eq!(out.len(), 6 + big.len() + 4);
        assert!(out.starts_with(b"header"));
        assert!(out.ends_with(b"tail"));
    }

    #[test]
    fn a_short_document_costs_one_small_block() {
        let mut blocks = Blocks::default();
        blocks.push(b"%PDF-1.7\n");

        assert_eq!(blocks.blocks.len(), 1);
        assert_eq!(blocks.blocks[0].capacity(), FLOOR);
    }

    #[test]
    fn nothing_written_is_an_empty_file_and_not_a_panic() {
        assert!(Blocks::default().into_vec().is_empty());
    }
}
