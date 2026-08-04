//! Reading an image's format and size from its own bytes.
//!
//! A producer hands the engine a logo and a name. It should not also have to
//! hand over `{ format: "png", width: 240, height: 80 }` — the file already
//! says so, and a producer that gets it wrong squashes the logo silently.
//!
//! This reads headers only. Decoding pixels is krilla's job and happens once,
//! at paint time; here we want the aspect ratio during measurement, which is
//! the first eight bytes of an IHDR chunk or a JPEG frame header.

use crate::content::ImageFormat;

/// What the bytes turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    #[error("not a PNG or JPEG image")]
    UnknownFormat,
    #[error("the image header is truncated")]
    Truncated,
    #[error("the image declares a size of {0}×{1}")]
    ZeroSized(u32, u32),
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Identifies `bytes` and reads the pixel dimensions out of its header.
pub fn identify(bytes: &[u8]) -> Result<ImageInfo, ImageError> {
    if bytes.starts_with(&PNG_SIGNATURE) {
        return png(bytes);
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg(bytes);
    }
    Err(ImageError::UnknownFormat)
}

fn png(bytes: &[u8]) -> Result<ImageInfo, ImageError> {
    // Signature, then a chunk length and type, then IHDR's width and height.
    // The spec requires IHDR to come first, so the offsets are fixed.
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return Err(ImageError::Truncated);
    }
    sized(ImageFormat::Png, be32(&bytes[16..20]), be32(&bytes[20..24]))
}

fn jpeg(bytes: &[u8]) -> Result<ImageInfo, ImageError> {
    // Walk the segment chain to the frame header. A camera's JPEG carries
    // EXIF and colour-profile segments first, so the size is never at a fixed
    // offset — skipping by declared length is the only way through.
    let mut i = 2;
    loop {
        // Segments may be separated by any number of 0xFF fill bytes.
        while bytes.get(i) == Some(&0xFF) {
            i += 1;
        }
        let marker = *bytes.get(i).ok_or(ImageError::Truncated)?;
        i += 1;

        // Standalone markers carry no length, so there is nothing to skip.
        if matches!(marker, 0x01 | 0xD0..=0xD9) {
            continue;
        }

        let length = bytes.get(i..i + 2).map(be16).ok_or(ImageError::Truncated)? as usize;

        // Every SOFn holds the size in the same place. The markers that share
        // the range but are not frame headers are excluded by name: DHT
        // (0xC4), JPG (0xC8) and DAC (0xCC).
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let frame = bytes.get(i + 3..i + 7).ok_or(ImageError::Truncated)?;
            return sized(
                ImageFormat::Jpeg,
                be16(&frame[2..4]) as u32,
                be16(&frame[0..2]) as u32,
            );
        }

        // Past the start of scan comes entropy-coded data, not segments, and
        // a length read out of it would send the walk anywhere at all.
        if marker == 0xDA {
            return Err(ImageError::Truncated);
        }

        i += length;
    }
}

fn sized(format: ImageFormat, width: u32, height: u32) -> Result<ImageInfo, ImageError> {
    if width == 0 || height == 0 {
        return Err(ImageError::ZeroSized(width, height));
    }
    Ok(ImageInfo {
        format,
        width,
        height,
    })
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGO_PNG: &[u8] = include_bytes!("../tests/images/logo.png");
    const MARK_PNG: &[u8] = include_bytes!("../tests/images/mark.png");
    const LOGO_JPEG: &[u8] = include_bytes!("../tests/images/logo.jpg");
    const MARK_JPEG: &[u8] = include_bytes!("../tests/images/mark.jpg");

    #[test]
    fn a_png_gives_up_its_format_and_size() {
        let info = identify(LOGO_PNG).unwrap();

        assert_eq!(info.format, ImageFormat::Png);
        assert_eq!((info.width, info.height), (240, 80));
    }

    #[test]
    fn a_jpeg_gives_up_its_format_and_size() {
        let info = identify(LOGO_JPEG).unwrap();

        assert_eq!(info.format, ImageFormat::Jpeg);
        assert_eq!((info.width, info.height), (240, 80));
    }

    #[test]
    fn the_size_is_read_past_the_metadata_segments() {
        // This file carries JFIF, EXIF and a Photoshop block before the frame
        // header, so a reader that looked at a fixed offset would find none
        // of the size and all of the camera's opinion about it.
        let markers: Vec<u8> = segment_markers(LOGO_JPEG);

        assert!(
            markers.len() > 1,
            "the sample must have segments to skip: {markers:02X?}"
        );
        assert_eq!(identify(LOGO_JPEG).unwrap().width, 240);
    }

    #[test]
    fn width_and_height_are_not_transposed() {
        // A square sample would hide the mistake, so both samples are oblong
        // and one is square only to prove the reader is not guessing.
        assert_eq!(identify(LOGO_PNG).unwrap().width, 240);
        assert_eq!(identify(LOGO_PNG).unwrap().height, 80);
        assert_eq!(identify(LOGO_JPEG).unwrap().width, 240);
        assert_eq!(identify(LOGO_JPEG).unwrap().height, 80);

        let square = identify(MARK_PNG).unwrap();
        assert_eq!((square.width, square.height), (64, 64));
    }

    #[test]
    fn a_progressive_jpeg_is_read_like_any_other() {
        // SOF2 rather than SOF0. The frame header is laid out identically,
        // so the only way to fail this is to hardcode the baseline marker.
        let mut progressive = MARK_JPEG.to_vec();
        let sof = find_sof(&progressive);
        progressive[sof] = 0xC2;

        let info = identify(&progressive).unwrap();

        assert_eq!((info.width, info.height), (64, 64));
    }

    #[test]
    fn a_huffman_table_is_not_mistaken_for_a_frame_header() {
        // DHT is 0xC4 — inside the SOFn range but not a frame header. Reading
        // its payload as a size yields nonsense rather than an error, which is
        // the worst kind of bug, so it gets its own test.
        let mut tampered = MARK_JPEG.to_vec();
        let sof = find_sof(&tampered);
        tampered[sof] = 0xC4;

        // With the real frame header disguised there is nothing left to find.
        let found = identify(&tampered);

        assert!(
            found.is_err() || found.unwrap().width != 64,
            "0xC4 must not be read as a frame header"
        );
    }

    #[test]
    fn anything_that_is_neither_is_refused() {
        assert_eq!(
            identify(b"GIF89a...").unwrap_err(),
            ImageError::UnknownFormat
        );
        assert_eq!(identify(b"<svg/>").unwrap_err(), ImageError::UnknownFormat);
        assert_eq!(identify(&[]).unwrap_err(), ImageError::UnknownFormat);
    }

    #[test]
    fn a_truncated_header_is_refused_rather_than_read_past_the_end() {
        // Every prefix of a real file, one byte at a time. Any of these
        // panicking would take down a server that was handed a partial upload.
        for cut in 0..LOGO_PNG.len().min(64) {
            let _ = identify(&LOGO_PNG[..cut]);
        }
        for cut in 0..LOGO_JPEG.len().min(4096) {
            let _ = identify(&LOGO_JPEG[..cut]);
        }

        assert!(identify(&LOGO_PNG[..20]).is_err());
        assert!(identify(&LOGO_JPEG[..8]).is_err());
    }

    #[test]
    fn a_jpeg_with_no_frame_header_at_all_terminates() {
        // Bytes that look like a JPEG and then are not. The walk must stop at
        // the start of scan instead of running to the end of a 200 MB file.
        let mut headless = vec![0xFF, 0xD8];
        headless.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]);
        headless.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        headless.extend_from_slice(&[0x42; 1024]);

        assert_eq!(identify(&headless).unwrap_err(), ImageError::Truncated);
    }

    #[test]
    fn the_compressed_scan_is_not_searched_for_a_frame_header() {
        // Entropy-coded data is arbitrary bytes, and in a photograph of any
        // size some pair of them will be 0xFF 0xC0. Reading on past the start
        // of scan therefore does not fail — it succeeds, with a size invented
        // by the compressor. Here the impostor claims 4×4.
        let mut disguised = vec![0xFF, 0xD8];
        disguised.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        disguised.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x04, 0x00, 0x04]);

        let found = identify(&disguised);

        assert!(
            found.is_err(),
            "read a size out of the scan data: {found:?}"
        );
    }

    #[test]
    fn a_png_whose_first_chunk_is_not_the_header_is_refused() {
        // The width lives at a fixed offset only because IHDR is required to
        // come first. Without checking that, four bytes of some other chunk
        // become the image's size and the logo is scaled by them.
        let mut headerless = LOGO_PNG.to_vec();
        headerless[12..16].copy_from_slice(b"tEXt");

        assert_eq!(identify(&headerless).unwrap_err(), ImageError::Truncated);
    }

    #[test]
    fn a_segment_that_declares_no_length_does_not_stall_the_walk() {
        // Bytes a compressor would never emit: a segment claiming to be zero
        // long, so skipping it moves nowhere. The walk survives only because
        // reading the marker advances on its own — which is exactly the kind
        // of thing a later tidy-up removes. A malformed upload that hung a
        // worker until the process was restarted is worth this much care, so
        // the test runs on its own thread and asserts it comes back at all.
        use std::sync::mpsc;
        use std::time::Duration;

        let mut stalling = vec![0xFF, 0xD8];
        stalling.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x00]);
        stalling.extend_from_slice(&[0x00; 256]);

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || tx.send(identify(&stalling)));

        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the walk never terminated"
        );
    }

    #[test]
    fn a_png_that_declares_no_pixels_is_refused() {
        // Dividing by this to keep the aspect ratio is how it would surface
        // otherwise: an image scaled to infinity, or to nothing.
        let mut zeroed = LOGO_PNG.to_vec();
        zeroed[20..24].copy_from_slice(&0u32.to_be_bytes());

        assert_eq!(
            identify(&zeroed).unwrap_err(),
            ImageError::ZeroSized(240, 0)
        );
    }

    #[test]
    fn a_file_that_is_only_a_signature_is_refused() {
        assert_eq!(identify(&PNG_SIGNATURE).unwrap_err(), ImageError::Truncated);
        assert_eq!(identify(&[0xFF, 0xD8]).unwrap_err(), ImageError::Truncated);
    }

    // ── helpers ─────────────────────────────────────────────────────────

    /// The marker byte of every segment before the frame header.
    fn segment_markers(jpeg: &[u8]) -> Vec<u8> {
        let mut markers = Vec::new();
        let mut i = 2;
        while i + 4 <= jpeg.len() && jpeg[i] == 0xFF {
            markers.push(jpeg[i + 1]);
            if matches!(jpeg[i + 1], 0xC0..=0xCF) {
                break;
            }
            i += 2 + be16(&jpeg[i + 2..i + 4]) as usize;
        }
        markers
    }

    /// Index of the frame header's marker byte.
    fn find_sof(jpeg: &[u8]) -> usize {
        let mut i = 2;
        loop {
            if matches!(jpeg[i + 1], 0xC0..=0xCF) && !matches!(jpeg[i + 1], 0xC4 | 0xC8 | 0xCC) {
                return i + 1;
            }
            i += 2 + be16(&jpeg[i + 2..i + 4]) as usize;
        }
    }
}
