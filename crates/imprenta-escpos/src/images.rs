use crate::Error;
use image::{DynamicImage, ImageReader, imageops::FilterType};
use std::io::Cursor;

pub fn decode(bytes: &[u8]) -> Result<DynamicImage, Error> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|e| Error(format!("image could not be decoded: {e}")))
}

pub fn raster(
    image: &DynamicImage,
    width: u16,
    height: Option<u16>,
    filter: &str,
    threshold: u8,
) -> Result<Vec<u8>, Error> {
    let height = height
        .map(u32::from)
        .unwrap_or_else(|| (image.height() * u32::from(width) / image.width()).max(1));
    if height == 0 || height > 16384 {
        return Err(Error("image height must be 1–16384 dots".into()));
    }
    if !["monochrome", "dither"].contains(&filter) {
        return Err(Error("image filter must be monochrome or dither".into()));
    }
    let rgba = image
        .resize_exact(u32::from(width), height, FilterType::Triangle)
        .to_rgba8();
    let stride = usize::from(width).div_ceil(8);
    let mut pixels = vec![0_u8; stride * height as usize];
    let mut errors = vec![0_f32; usize::from(width) + 2];
    for y in 0..height {
        let mut next = vec![0_f32; errors.len()];
        for x in 0..u32::from(width) {
            let p = rgba.get_pixel(x, y).0;
            let alpha = f32::from(p[3]) / 255.0;
            let gray =
                (f32::from(p[0]) * 0.299 + f32::from(p[1]) * 0.587 + f32::from(p[2]) * 0.114)
                    * alpha
                    + 255.0 * (1.0 - alpha);
            let value = gray + errors[x as usize + 1];
            let black = value < f32::from(threshold);
            if black {
                pixels[y as usize * stride + x as usize / 8] |= 0x80 >> (x % 8);
            }
            if filter == "dither" {
                let error = value - if black { 0.0 } else { 255.0 };
                errors[x as usize + 2] += error * 7.0 / 16.0;
                next[x as usize] += error * 3.0 / 16.0;
                next[x as usize + 1] += error * 5.0 / 16.0;
                next[x as usize + 2] += error / 16.0;
            }
        }
        errors = next;
    }
    let mut bytes = Vec::with_capacity(pixels.len() + height as usize / 255 * 8 + 8);
    // Raster blocks of at most 255 rows also work on devices that ignore yH.
    for chunk in pixels.chunks(stride * 255) {
        let rows = chunk.len() / stride;
        bytes.extend_from_slice(&[
            0x1d,
            0x76,
            0x30,
            0,
            stride as u8,
            (stride >> 8) as u8,
            rows as u8,
            0,
        ]);
        bytes.extend_from_slice(chunk);
    }
    Ok(bytes)
}
