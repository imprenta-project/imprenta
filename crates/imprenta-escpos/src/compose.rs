use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use escpos::{driver::Driver, printer::Printer, utils::*};
use unicode_normalization::UnicodeNormalization;
use yore::code_pages::{CP437, CP850, CP1252};

use crate::{
    Error, Image, Outcome, images,
    model::{self, Align, CutMode, Font, Node, Receipt, ResolvedProfile, Style, Underline},
};

impl From<escpos::errors::PrinterError> for Error {
    fn from(value: escpos::errors::PrinterError) -> Self {
        Self(value.to_string())
    }
}

#[derive(Clone, Default)]
struct Buffer(Rc<RefCell<Vec<u8>>>);
impl Driver for Buffer {
    fn name(&self) -> String {
        "memory".into()
    }
    fn write(&self, data: &[u8]) -> escpos::errors::Result<()> {
        self.0.borrow_mut().extend_from_slice(data);
        Ok(())
    }
    fn read(&self, _data: &mut [u8]) -> escpos::errors::Result<usize> {
        Ok(0)
    }
    fn flush(&self) -> escpos::errors::Result<()> {
        Ok(())
    }
}

fn range(value: u16, name: &str, min: u16, max: u16) -> Result<(), Error> {
    if !(min..=max).contains(&value) {
        return Err(Error(format!("{name} must be between {min} and {max}")));
    }
    Ok(())
}

fn mm(value: f64, dpi: u16, name: &str) -> Result<u16, Error> {
    let dots = (value * f64::from(dpi) / 25.4).round();
    if !value.is_finite() || value < 0.0 || dots > 65535.0 {
        return Err(Error(format!("invalid {name} in millimetres")));
    }
    Ok(dots as u16)
}

fn profile(receipt: &Receipt) -> Result<ResolvedProfile, Error> {
    let mut dpi = receipt.profile.dpi.unwrap_or(203);
    range(dpi, "dpi", 72, 1200)?;
    let mut columns = receipt.profile.columns.unwrap_or(48);
    range(columns, "columns", 1, 256)?;
    let mut width = receipt.profile.printable_width.unwrap_or(columns * 12);
    let mut margin_left = 0;
    let mut initialized = false;
    for (i, node) in receipt.children.iter().enumerate() {
        if let Node::Init {
            width: declared,
            margin_left: margin,
            dpi: resolution,
        } = node
        {
            if i != 0 || initialized {
                return Err(Error("<Init> belongs at the start, once per ticket".into()));
            }
            initialized = true;
            dpi = resolution.unwrap_or(dpi);
            range(dpi, "dpi", 72, 1200)?;
            margin_left = mm(margin.unwrap_or(0.0), dpi, "marginLeft")?;
            if let Some(declared) = declared {
                width = mm(*declared, dpi, "width")?;
                columns = width / 12;
            }
        }
    }
    range(width, "printableWidth", 12, 3072)?;
    range(columns, "columns", 1, width / 12)?;
    if u32::from(width) + u32::from(margin_left) > 65535 {
        return Err(Error(
            "printable area and margin exceed ESC/POS limits".into(),
        ));
    }
    let codepage = receipt
        .profile
        .codepage
        .clone()
        .unwrap_or_else(|| "cp858".into());
    let id = match codepage.as_str() {
        "cp437" => 0,
        "cp850" => 2,
        "cp858" => 19,
        "cp1252" => 16,
        _ => return Err(Error(format!("unsupported codepage: {codepage}"))),
    };
    Ok(ResolvedProfile {
        columns,
        printable_width: width,
        margin_left,
        dpi,
        codepage,
        codepage_id: receipt.profile.codepage_id.unwrap_or(id),
    })
}

fn safe(value: &str) -> Result<(), Error> {
    if value.chars().any(|ch| ch.is_control() && ch != '\n') {
        return Err(Error("text contains a printer control character".into()));
    }
    Ok(())
}

fn encode(value: &str, page: &str, diagnostics: &mut BTreeSet<String>) -> Result<Vec<u8>, Error> {
    safe(value)?;
    let mut out = Vec::with_capacity(value.len());
    for ch in value.nfc() {
        if ch.is_ascii() {
            out.push(ch as u8);
            continue;
        }
        if page == "cp858" && ch == '€' {
            out.push(0xd5);
            continue;
        }
        let mut buf = [0; 4];
        let s = ch.encode_utf8(&mut buf);
        let encoded = match page {
            "cp437" => CP437.encode(s),
            "cp1252" => CP1252.encode(s),
            _ => CP850.encode(s),
        };
        match encoded {
            Ok(bytes) if !(page == "cp858" && bytes[0] == 0xd5) => out.extend_from_slice(&bytes),
            _ => {
                out.push(b'?');
                diagnostics.insert(format!(
                    "unsupported-character: {ch:?} is unavailable in {page}; printed as ?"
                ));
            }
        }
    }
    Ok(out)
}

fn justify(printer: &mut Printer<Buffer>, align: Align) -> Result<(), Error> {
    printer.justify(match align {
        Align::Left => JustifyMode::LEFT,
        Align::Center => JustifyMode::CENTER,
        Align::Right => JustifyMode::RIGHT,
    })?;
    Ok(())
}

fn styled(
    printer: &mut Printer<Buffer>,
    style: &Style,
    profile: &ResolvedProfile,
) -> Result<usize, Error> {
    let width = style.font_width.unwrap_or(1);
    let height = style.font_height.unwrap_or(1);
    range(width.into(), "fontWidth", 1, 8)?;
    range(height.into(), "fontHeight", 1, 8)?;
    printer
        .font(match style.font {
            Font::A => escpos::utils::Font::A,
            Font::B => escpos::utils::Font::B,
        })?
        .size(width, height)?
        .bold(style.bold)?
        .reverse(style.inverse)?
        .double_strike(style.double_strike)?
        .underline(match style.underline {
            Underline::None => UnderlineMode::None,
            Underline::Single => UnderlineMode::Single,
            Underline::Double => UnderlineMode::Double,
        })?;
    justify(printer, style.justification)?;
    match style.line_spacing {
        Some(value) => {
            printer.line_spacing(value)?;
        }
        None => {
            printer.reset_line_spacing()?;
        }
    }
    let cells = match style.font {
        Font::A => usize::from(profile.columns),
        Font::B => usize::from(profile.printable_width) / 9,
    } / usize::from(width);
    if cells == 0 {
        return Err(Error("fontWidth leaves no room for a character".into()));
    }
    Ok(cells)
}

/// Wrap single-byte native character cells. Explicit line breaks survive.
fn wrap(value: &[u8], width: usize) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    for paragraph in value.split(|byte| *byte == b'\n') {
        let mut rest = paragraph;
        while rest.len() > width {
            let at = if rest[width] == b' ' {
                width
            } else {
                rest[..width]
                    .iter()
                    .rposition(|b| *b == b' ')
                    .filter(|i| *i > 0)
                    .unwrap_or(width)
            };
            lines.push(rest[..at].to_vec());
            rest = &rest[at..];
            while rest.first() == Some(&b' ') {
                rest = &rest[1..];
            }
        }
        lines.push(rest.to_vec());
    }
    lines
}

fn row(
    columns: &[model::Column],
    gap: usize,
    available: usize,
    page: &str,
    diagnostics: &mut BTreeSet<String>,
) -> Result<Vec<Vec<u8>>, Error> {
    if columns.is_empty() {
        return Err(Error("a row needs at least one column".into()));
    }
    let fixed: usize = columns
        .iter()
        .map(|c| usize::from(c.width.unwrap_or(0)))
        .sum();
    let flexible = columns.iter().filter(|c| c.width.is_none()).count();
    let space = fixed
        .checked_add(gap.saturating_mul(columns.len() - 1))
        .ok_or_else(|| Error("row width overflows".into()))?;
    let remaining = available
        .checked_sub(space)
        .ok_or_else(|| Error("row columns exceed available width".into()))?;
    if remaining < flexible || columns.iter().any(|c| c.width == Some(0)) {
        return Err(Error("row columns need a positive width".into()));
    }
    let mut extra = if flexible == 0 {
        0
    } else {
        remaining % flexible
    };
    let mut cells = Vec::with_capacity(columns.len());
    for col in columns {
        let width = if let Some(width) = col.width {
            usize::from(width)
        } else {
            let width = remaining / flexible + usize::from(extra > 0);
            extra = extra.saturating_sub(1);
            width
        };
        let value = encode(&col.text, page, diagnostics)?;
        if matches!(col.overflow, model::Overflow::Error)
            && value.split(|b| *b == b'\n').any(|p| p.len() > width)
        {
            return Err(Error(format!("column text exceeds its width of {width}")));
        }
        cells.push((width, col.justification, wrap(&value, width)));
    }
    let height = cells.iter().map(|c| c.2.len()).max().unwrap_or(0);
    let mut lines = Vec::with_capacity(height);
    for i in 0..height {
        let mut line = Vec::with_capacity(available);
        for (c, (width, align, values)) in cells.iter().enumerate() {
            if c > 0 {
                line.extend(std::iter::repeat_n(b' ', gap));
            }
            let value = values.get(i).map(Vec::as_slice).unwrap_or(&[]);
            let rest = width - value.len();
            let left = match align {
                Align::Left => 0,
                Align::Center => rest / 2,
                Align::Right => rest,
            };
            line.extend(std::iter::repeat_n(b' ', left));
            line.extend_from_slice(value);
            line.extend(std::iter::repeat_n(b' ', rest - left));
        }
        lines.push(line);
    }
    Ok(lines)
}

fn lines(printer: &mut Printer<Buffer>, lines: &[Vec<u8>], lf: bool) -> Result<(), Error> {
    for (i, line) in lines.iter().enumerate() {
        printer.custom(line)?;
        if lf || i + 1 < lines.len() {
            printer.custom(b"\n")?;
        }
    }
    Ok(())
}

pub fn render(receipt: &Receipt, assets: &[Image]) -> Result<Outcome, Error> {
    let profile = profile(receipt)?;
    let buffer = Buffer::default();
    let mut printer = Printer::new(buffer.clone(), Protocol::default(), None);
    printer.init()?.custom(&[0x1b, 0x74, profile.codepage_id])?;
    // Restore the device's native motion units; the profile's DPI describes that device.
    printer.custom(&[0x1d, 0x50, 0, 0])?;
    printer.custom(&[
        0x1d,
        0x4c,
        profile.margin_left as u8,
        (profile.margin_left >> 8) as u8,
    ])?;
    printer.custom(&[
        0x1d,
        0x57,
        profile.printable_width as u8,
        (profile.printable_width >> 8) as u8,
    ])?;
    let mut diagnostics = BTreeSet::new();
    let mut tickets = 0;
    let mut content = false;
    let mut open_line = false;
    let mut cursor = 0_usize;
    let mut decoded = std::collections::HashMap::new();
    for node in &receipt.children {
        if !matches!(
            node,
            Node::Text { .. } | Node::Init { .. } | Node::Bytes { .. }
        ) && open_line
        {
            printer.custom(b"\n")?;
            open_line = false;
            cursor = 0;
        }
        match node {
            Node::Init { .. } => {}
            Node::Text {
                text,
                style,
                lf,
                prefix,
                suffix,
                format,
            } => {
                let width = styled(&mut printer, style, &profile)?;
                let value = if let Some(format) = format {
                    if format.matches("%s").count() != 1 {
                        return Err(Error("Text format needs exactly one %s placeholder".into()));
                    }
                    format.replacen("%s", text, 1)
                } else {
                    text.clone()
                };
                let value = format!(
                    "{}{}{}",
                    prefix.as_deref().unwrap_or(""),
                    value,
                    suffix.as_deref().unwrap_or("")
                );
                let encoded = encode(&value, &profile.codepage, &mut diagnostics)?;
                let cell = match style.font {
                    Font::A => 12,
                    Font::B => 9,
                } * usize::from(style.font_width.unwrap_or(1));
                if open_line && !matches!(style.justification, Align::Left) {
                    return Err(Error("inline Text justification must be left; feed before centering or right-aligning".into()));
                }
                let room = usize::from(profile.printable_width).saturating_sub(cursor) / cell;
                let mut wrapped = Vec::new();
                if open_line && room < width && !encoded.is_empty() {
                    if room == 0 {
                        printer.custom(b"\n")?;
                        cursor = 0;
                        wrapped = wrap(&encoded, width);
                    } else {
                        let first_break = encoded
                            .iter()
                            .position(|b| *b == b'\n')
                            .unwrap_or(encoded.len());
                        let first = &encoded[..first_break];
                        let end = if first.len() > room {
                            room
                        } else {
                            first.len()
                        };
                        wrapped.push(first[..end].to_vec());
                        if first.len() > room {
                            wrapped.extend(wrap(&encoded[end..], width));
                        } else if first_break < encoded.len() {
                            wrapped.extend(wrap(&encoded[first_break + 1..], width));
                        }
                    }
                } else {
                    wrapped = wrap(&encoded, width);
                }
                lines(&mut printer, &wrapped, lf.unwrap_or(true))?;
                cursor = if lf.unwrap_or(true) {
                    0
                } else {
                    let last = wrapped.last().map_or(0, Vec::len) * cell;
                    if wrapped.len() == 1 {
                        cursor + last
                    } else {
                        last
                    }
                };
                open_line = !lf.unwrap_or(true);
                content = true;
            }
            Node::Row {
                columns,
                gap,
                style,
            } => {
                let width = styled(&mut printer, style, &profile)?;
                lines(
                    &mut printer,
                    &row(
                        columns,
                        usize::from(gap.unwrap_or(1)),
                        width,
                        &profile.codepage,
                        &mut diagnostics,
                    )?,
                    true,
                )?;
                content = true;
            }
            Node::Rule { character, style } => {
                let width = styled(&mut printer, style, &profile)?;
                let encoded = encode(
                    character.as_deref().unwrap_or("-"),
                    &profile.codepage,
                    &mut diagnostics,
                )?;
                if encoded.len() != 1 || encoded[0] == b'\n' {
                    return Err(Error("a rule needs one printable character".into()));
                }
                printer.custom(&vec![encoded[0]; width])?.feed()?;
                content = true;
            }
            Node::Feed { lines } => {
                printer.feeds(lines.unwrap_or(1))?;
            }
            Node::Cut { mode } => {
                printer.custom(&[
                    0x1d,
                    0x56,
                    if matches!(mode, CutMode::Partial) {
                        1
                    } else {
                        0
                    },
                ])?;
                if content {
                    tickets += 1;
                }
                content = false;
            }
            Node::Qrcode {
                value,
                model,
                size,
                error_correction_level,
                justification,
            } => {
                safe(value)?;
                range(size.unwrap_or(4).into(), "QR size", 1, 8)?;
                let model = match model.unwrap_or(2) {
                    1 => QRCodeModel::Model1,
                    2 => QRCodeModel::Model2,
                    _ => return Err(Error("QR model must be 1 or 2".into())),
                };
                let level = match error_correction_level.as_deref().unwrap_or("M") {
                    "L" => QRCodeCorrectionLevel::L,
                    "M" => QRCodeCorrectionLevel::M,
                    "Q" => QRCodeCorrectionLevel::Q,
                    "H" => QRCodeCorrectionLevel::H,
                    _ => return Err(Error("invalid QR errorCorrectionLevel".into())),
                };
                justify(&mut printer, *justification)?;
                printer
                    .qrcode_option(value, QRCodeOption::new(model, size.unwrap_or(4), level))?
                    .feed()?;
                content = true;
            }
            Node::Barcode {
                value,
                barcode_type,
                width,
                height,
                hri_position,
                hri_font,
                justification,
            } => {
                safe(value)?;
                let width = width.unwrap_or(2);
                let height = height.unwrap_or(64);
                range(width.into(), "barcode width", 2, 6)?;
                range(height.into(), "barcode height", 1, 255)?;
                let position = match hri_position.as_deref().unwrap_or("not-printed") {
                    "not-printed" => 0,
                    "above-bar-code" => 1,
                    "below-bar-code" => 2,
                    "above-and-below-bar-code" => 3,
                    _ => return Err(Error("invalid hriPosition".into())),
                };
                let (kind, data) = barcode(value, barcode_type.as_deref().unwrap_or("code128"))?;
                justify(&mut printer, *justification)?;
                printer.custom(&[
                    0x1d,
                    0x77,
                    width,
                    0x1d,
                    0x68,
                    height,
                    0x1d,
                    0x48,
                    position,
                    0x1d,
                    0x66,
                    if matches!(hri_font, Some(Font::B)) {
                        1
                    } else {
                        0
                    },
                ])?;
                printer
                    .custom(&[0x1d, 0x6b, kind, data.len() as u8])?
                    .custom(&data)?
                    .feed()?;
                content = true;
            }
            Node::Pdf417 {
                value,
                width,
                height,
                number_of_columns,
                number_of_rows,
                error_correction_level,
                option,
                justification,
            } => {
                safe(value)?;
                range(width.unwrap_or(3).into(), "PDF417 width", 2, 8)?;
                range(height.unwrap_or(3).into(), "PDF417 height", 2, 8)?;
                if value.is_empty() || value.len() > 65532 {
                    return Err(Error("PDF417 value length must be 1–65532 bytes".into()));
                }
                let level = match error_correction_level.unwrap_or(1) {
                    0 => Pdf417CorrectionLevel::Level0,
                    1 => Pdf417CorrectionLevel::Level1,
                    2 => Pdf417CorrectionLevel::Level2,
                    3 => Pdf417CorrectionLevel::Level3,
                    4 => Pdf417CorrectionLevel::Level4,
                    5 => Pdf417CorrectionLevel::Level5,
                    6 => Pdf417CorrectionLevel::Level6,
                    7 => Pdf417CorrectionLevel::Level7,
                    8 => Pdf417CorrectionLevel::Level8,
                    _ => return Err(Error("PDF417 errorCorrectionLevel must be 0–8".into())),
                };
                let kind = match option.as_deref().unwrap_or("standard") {
                    "standard" => Pdf417Type::Standard,
                    "truncated" => Pdf417Type::Truncated,
                    _ => return Err(Error("PDF417 option must be standard or truncated".into())),
                };
                let options = Pdf417Option::new(
                    number_of_columns.unwrap_or(0),
                    number_of_rows.unwrap_or(0),
                    width.unwrap_or(3),
                    height.unwrap_or(3),
                    kind,
                    level,
                )?;
                justify(&mut printer, *justification)?;
                printer.pdf417_option(value, options)?.feed()?;
                content = true;
            }
            Node::Image {
                src,
                width,
                height,
                filter,
                threshold,
                justification,
            } => {
                range(*width, "image width", 1, profile.printable_width)?;
                if !decoded.contains_key(src) {
                    let asset = assets
                        .iter()
                        .find(|image| image.name == *src)
                        .ok_or_else(|| Error(format!("image {src:?} is not configured")))?;
                    decoded.insert(src.clone(), images::decode(&asset.data)?);
                }
                let image = images::raster(
                    &decoded[src],
                    *width,
                    *height,
                    filter.as_deref().unwrap_or("monochrome"),
                    threshold.unwrap_or(127),
                )?;
                justify(&mut printer, *justification)?;
                printer.custom(&image)?;
                content = true;
            }
            Node::Bytes { hex } => {
                let bytes = hex
                    .split_whitespace()
                    .map(|part| {
                        if part.len() == 2 {
                            u8::from_str_radix(part, 16)
                                .map_err(|_| Error("Bytes needs hexadecimal byte pairs".into()))
                        } else {
                            Err(Error("Bytes needs hexadecimal byte pairs".into()))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                printer.custom(&bytes)?;
                diagnostics.insert(
                    "raw-bytes: custom commands may change printer state; preview is a simulation"
                        .into(),
                );
                content |= !bytes.is_empty();
            }
        }
        if matches!(
            node,
            Node::Text { .. }
                | Node::Row { .. }
                | Node::Rule { .. }
                | Node::Qrcode { .. }
                | Node::Barcode { .. }
                | Node::Pdf417 { .. }
                | Node::Image { .. }
        ) {
            styled(&mut printer, &Style::default(), &profile)?;
        }
        // Keep only one block's instructions; the final byte buffer grows amortized linearly.
        printer.print()?;
    }
    if open_line {
        printer.custom(b"\n")?.print()?;
    }
    if content {
        tickets += 1;
    }
    printer.print()?;
    let escpos = std::mem::take(&mut *buffer.0.borrow_mut());
    Ok(Outcome {
        escpos,
        tickets,
        diagnostics: diagnostics.into_iter().collect(),
        profile,
    })
}

fn barcode(value: &str, kind: &str) -> Result<(u8, Vec<u8>), Error> {
    if value.is_empty() || !value.is_ascii() {
        return Err(Error("barcode value must be non-empty ASCII".into()));
    }
    let numeric = value.bytes().all(|b| b.is_ascii_digit());
    let (kind, data) = match kind {
        "code128" => {
            if value.bytes().any(|b| !(32..=126).contains(&b)) {
                return Err(Error("code128 supports printable ASCII".into()));
            }
            (73, format!("{{B{}", value.replace('{', "{{")).into_bytes())
        }
        "ean13" if numeric && [12, 13].contains(&value.len()) => (67, value.as_bytes().to_vec()),
        "ean8" if numeric && [7, 8].contains(&value.len()) => (68, value.as_bytes().to_vec()),
        "upca" if numeric && [11, 12].contains(&value.len()) => (65, value.as_bytes().to_vec()),
        "upce" if numeric && [6, 7, 8].contains(&value.len()) => (66, value.as_bytes().to_vec()),
        "itf" if numeric && value.len() % 2 == 0 => (70, value.as_bytes().to_vec()),
        "code39"
            if value.bytes().all(|b| {
                b.is_ascii_uppercase() || b.is_ascii_digit() || b" $%+-./".contains(&b)
            }) =>
        {
            (69, value.as_bytes().to_vec())
        }
        _ => {
            return Err(Error(format!(
                "unsupported barcode type or invalid data: {kind}"
            )));
        }
    };
    if data.len() > 255 {
        return Err(Error("barcode data exceeds 255 bytes".into()));
    }
    Ok((kind, data))
}
