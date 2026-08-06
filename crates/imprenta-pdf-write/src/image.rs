//! Embedding a picture.
//!
//! PDF knows two of the formats a document is likely to carry and knows them
//! very differently. **JPEG goes in whole**: `DCTDecode` is the JPEG decoder,
//! so the file is the stream and nothing is re-encoded — which matters, since
//! re-encoding a photograph loses a little of it every time. **PNG does not
//! exist** as far as PDF is concerned: it has to be decoded to raw samples
//! and deflated back down, and its alpha channel becomes a separate greyscale
//! image the page uses as a soft mask.

use pdf_writer::{Chunk, Filter, Finish, Ref};

/// Which of the two formats a buffer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

/// Which registered image, as the writer numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(pub(crate) usize);

/// A picture ready to be written, and how.
#[derive(Default)]
pub(crate) struct Embedded {
    /// Either the JPEG as it arrived or the PNG's decoded samples.
    pub samples: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// One byte per pixel, when the source had an alpha channel.
    pub alpha: Option<Vec<u8>>,
    pub colours: Colours,
    /// True when `samples` is a JPEG to be handed over untouched.
    pub jpeg: bool,
    /// Assigned the first time the image is actually drawn.
    pub reference: Option<Ref>,
    /// The buffer this was decoded from, held so that the address it is
    /// keyed by cannot be handed to something else.
    pub source: Option<std::sync::Arc<[u8]>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Colours {
    #[default]
    Rgb,
    Grey,
    Cmyk,
}

impl Embedded {
    /// Reads `data`, or `None` if it cannot be read.
    ///
    /// A decode failure is not an error the caller can do anything with: the
    /// painter has no way to report and a logo nobody can read must not take
    /// a nine thousand page render down with it. `imprenta-pdf` turns an
    /// unreadable image into a build diagnostic while it is still measuring.
    pub fn decode(data: &[u8], format: ImageFormat) -> Option<Self> {
        match format {
            ImageFormat::Jpeg => jpeg(data),
            ImageFormat::Png => png(data),
        }
    }
}

fn jpeg(data: &[u8]) -> Option<Embedded> {
    let (width, height, components) = jpeg_frame(data)?;
    Some(Embedded {
        samples: data.to_vec(),
        width,
        height,
        alpha: None,
        colours: match components {
            1 => Colours::Grey,
            4 => Colours::Cmyk,
            _ => Colours::Rgb,
        },
        jpeg: true,
        reference: None,
        source: None,
    })
}

/// Walks the segment chain to the frame header.
///
/// The size and the component count are never at a fixed offset — a camera's
/// file carries EXIF and a colour profile first — so skipping by declared
/// length is the only way through.
fn jpeg_frame(data: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i = 2;
    loop {
        while data.get(i) == Some(&0xFF) {
            i += 1;
        }
        let marker = *data.get(i)?;
        i += 1;
        if matches!(marker, 0x01 | 0xD0..=0xD9) {
            continue;
        }
        let length = u16::from_be_bytes([*data.get(i)?, *data.get(i + 1)?]) as usize;
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let frame = data.get(i + 3..i + 8)?;
            return Some((
                u32::from(u16::from_be_bytes([frame[2], frame[3]])),
                u32::from(u16::from_be_bytes([frame[0], frame[1]])),
                frame[4],
            ));
        }
        i += length;
    }
}

fn png(data: &[u8]) -> Option<Embedded> {
    let decoder = png::Decoder::new(data);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());

    // Sixteen bits a channel is decoded to eight. PDF can carry sixteen, but
    // nothing that reaches this engine is a medical scan, and eight halves
    // the file for a difference nobody printing an invoice will see.
    let sixteen = info.bit_depth == png::BitDepth::Sixteen;
    let take = |i: usize| -> u8 { if sixteen { buffer[i * 2] } else { buffer[i] } };
    let values = if sixteen {
        buffer.len() / 2
    } else {
        buffer.len()
    };
    let pixels = (info.width as usize) * (info.height as usize);

    let (samples, alpha, colours) = match info.color_type {
        png::ColorType::Rgb => (
            (0..values).map(take).collect::<Vec<u8>>(),
            None,
            Colours::Rgb,
        ),
        png::ColorType::Rgba => {
            let mut rgb = Vec::with_capacity(pixels * 3);
            let mut mask = Vec::with_capacity(pixels);
            for pixel in 0..pixels {
                rgb.push(take(pixel * 4));
                rgb.push(take(pixel * 4 + 1));
                rgb.push(take(pixel * 4 + 2));
                mask.push(take(pixel * 4 + 3));
            }
            (rgb, Some(mask), Colours::Rgb)
        }
        png::ColorType::Grayscale => (
            (0..values).map(take).collect::<Vec<u8>>(),
            None,
            Colours::Grey,
        ),
        png::ColorType::GrayscaleAlpha => {
            let mut grey = Vec::with_capacity(pixels);
            let mut mask = Vec::with_capacity(pixels);
            for pixel in 0..pixels {
                grey.push(take(pixel * 2));
                mask.push(take(pixel * 2 + 1));
            }
            (grey, Some(mask), Colours::Grey)
        }
        // `read_info` expands a palette for us, so this is unreachable in
        // practice; treating it as greyscale is at least not a panic.
        png::ColorType::Indexed => (
            (0..values).map(take).collect::<Vec<u8>>(),
            None,
            Colours::Grey,
        ),
    };

    Some(Embedded {
        samples,
        width: info.width,
        height: info.height,
        alpha: alpha.filter(|mask| mask.iter().any(|a| *a != 255)),
        colours,
        jpeg: false,
        reference: None,
        source: None,
    })
}

/// Writes the image, and its soft mask when it has one.
pub(crate) fn write(out: &mut Chunk, image: &Embedded, id: Ref, mask: Option<Ref>, compress: bool) {
    let deflated;
    let bytes = if image.jpeg || !compress {
        image.samples.as_slice()
    } else {
        deflated = miniz_oxide::deflate::compress_to_vec_zlib(&image.samples, 6);
        deflated.as_slice()
    };

    {
        let mut xobject = out.image_xobject(id, bytes);
        xobject.width(image.width as i32);
        xobject.height(image.height as i32);
        match image.colours {
            Colours::Rgb => xobject.color_space().device_rgb(),
            Colours::Grey => xobject.color_space().device_gray(),
            Colours::Cmyk => xobject.color_space().device_cmyk(),
        }
        xobject.bits_per_component(8);
        if image.jpeg {
            xobject.filter(Filter::DctDecode);
        } else if compress {
            xobject.filter(Filter::FlateDecode);
        }
        if let Some(mask) = mask {
            xobject.s_mask(mask);
        }
        xobject.finish();
    }

    if let (Some(mask), Some(alpha)) = (mask, image.alpha.as_ref()) {
        let deflated;
        let bytes = if compress {
            deflated = miniz_oxide::deflate::compress_to_vec_zlib(alpha, 6);
            deflated.as_slice()
        } else {
            alpha.as_slice()
        };
        let mut soft = out.image_xobject(mask, bytes);
        soft.width(image.width as i32);
        soft.height(image.height as i32);
        soft.color_space().device_gray();
        soft.bits_per_component(8);
        if compress {
            soft.filter(Filter::FlateDecode);
        }
        soft.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A four-pixel PNG with an alpha channel, written by hand so the test
    /// depends on nothing but the decoder.
    fn rgba_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[
                    255, 0, 0, 255, //
                    0, 255, 0, 128, //
                    0, 0, 255, 255, //
                    0, 0, 0, 0,
                ])
                .unwrap();
        }
        out
    }

    #[test]
    fn a_transparent_png_keeps_its_alpha_as_a_separate_mask() {
        // PDF has no notion of an alpha channel inside an image. Without the
        // mask a logo comes out on a black square, which is a defect that
        // passes every test about sizes and positions.
        let embedded = Embedded::decode(&rgba_png(), ImageFormat::Png).expect("decode");

        assert_eq!(embedded.width, 2);
        assert_eq!(embedded.samples.len(), 12, "three bytes a pixel, no alpha");
        assert_eq!(embedded.alpha.as_deref(), Some(&[255, 128, 255, 0][..]));
    }

    #[test]
    fn an_opaque_png_is_written_without_a_mask_at_all() {
        // A mask of nothing but 255 costs an object, a stream and a decode on
        // every page the image appears on, and does nothing.
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[10, 20, 30, 255]).unwrap();
        }

        let embedded = Embedded::decode(&out, ImageFormat::Png).expect("decode");

        assert!(embedded.alpha.is_none());
        assert_eq!(embedded.samples, vec![10, 20, 30]);
    }

    #[test]
    fn a_jpeg_is_handed_over_untouched() {
        // `DCTDecode` is the JPEG decoder, so re-encoding would lose a little
        // of the photograph for nothing at all.
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../imprenta-pdf/tests/images/logo.jpg"
        ))
        .expect("fixture");

        let embedded = Embedded::decode(&data, ImageFormat::Jpeg).expect("decode");

        assert!(embedded.jpeg);
        assert_eq!(embedded.samples, data);
        assert!(embedded.width > 0 && embedded.height > 0);
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused_rather_than_panicked_on() {
        assert!(Embedded::decode(b"not a picture", ImageFormat::Png).is_none());
        assert!(Embedded::decode(b"\xFF\xD8 truncated", ImageFormat::Jpeg).is_none());
    }
}
