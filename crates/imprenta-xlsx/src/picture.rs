//! Pictures: the one thing in a sheet that is not in a cell.
//!
//! Everything else here is a value in a grid. A picture is not — it floats
//! over the grid, hung off a cell so that it moves when rows are inserted
//! above it, and it is the only part of a workbook that has a size in length
//! units rather than in characters and points of row height.
//!
//! OOXML keeps it a long way from the sheet. The worksheet points at a
//! *drawing* part, the drawing part points at a *media* part through its own
//! relationships file, and the media part is the image bytes under a name the
//! package chose. Four files to put a logo in a corner, and Excel opens a
//! repair dialog naming none of them if any one is missing.
//!
//! # What is not here
//!
//! No scaling to a cell, no cropping, no rotation, no floating position in
//! sheet coordinates. A picture hangs off a cell at a size, which is what a
//! letterhead needs, and everything past that is a feature nobody has asked
//! for yet.

use imprenta_core::image::{ImageFormat, ImageInfo, identify};

use crate::ir::{Picture, Sheet};
use crate::xml::escaped;

/// English Metric Units in one point. DrawingML measures in these and
/// nothing else: 914 400 to the inch, so 12 700 to the point.
const EMU_PER_PT: f64 = 12_700.0;

/// An image handed over beside the workbook, under the name the IR uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub name: String,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(name: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            data: data.into(),
        }
    }
}

/// An image that has been identified and given a part name in the package.
#[derive(Debug, Clone)]
pub(crate) struct Stored {
    /// The name the IR refers to it by.
    pub name: String,
    /// `image1.png`, and so on. Numbered by first use, so the package is
    /// deterministic for a given workbook.
    pub part: String,
    pub info: ImageInfo,
}

impl Stored {
    pub fn extension(&self) -> &'static str {
        extension(self.info.format)
    }
}

pub(crate) fn extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
    }
}

/// What the media part of the package will hold, in a settled order.
///
/// Only the images some sheet actually names. A caller handing over every logo
/// it has and using one should not ship the rest inside the file — and a
/// caller that hands over none, on a workbook with no pictures, must produce
/// exactly the package it produces today.
pub(crate) fn stored(
    sheets: &[crate::ir::Sheet],
    images: &[Image],
) -> Result<Vec<Stored>, PictureError> {
    let mut out: Vec<Stored> = Vec::new();

    for picture in sheets.iter().flat_map(|sheet| &sheet.pictures) {
        if out.iter().any(|s| s.name == picture.image) {
            continue;
        }
        let image = images
            .iter()
            .find(|image| image.name == picture.image)
            .ok_or_else(|| PictureError::Missing(picture.image.clone()))?;

        let info = identify(&image.data)
            .map_err(|why| PictureError::Unreadable(picture.image.clone(), why.to_string()))?;

        out.push(Stored {
            name: picture.image.clone(),
            part: format!("image{}.{}", out.len() + 1, extension(info.format)),
            info,
        });
    }

    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PictureError {
    #[error("a picture names the image {0:?}, and no image of that name was handed over")]
    Missing(String),
    #[error("the image {0:?} could not be read: {1}")]
    Unreadable(String, String),

    /// A streamed sheet whose rows went past before a merge claimed them.
    ///
    /// Placing a picture down the page needs the heights of the rows its block
    /// covers. A streaming session keeps those rows only while some picture can
    /// still need them, which it works out from the merges it has been told
    /// about — so a merge arriving after its own rows leaves nothing to measure.
    /// Guessing would put the picture somewhere the same workbook declared
    /// whole does not, and the file would open either way.
    #[error(
        "the picture {image:?} is centred down a block ending at row {bottom}, and that merge \
         was declared after those rows had been written: declare it on the sheet, or place the \
         picture with `dy` instead"
    )]
    Unplaceable { image: String, bottom: u32 },
}

/// The drawing part for one sheet's pictures.
///
/// `oneCellAnchor` rather than `twoCellAnchor`: the picture is pinned to one
/// cell and keeps the size it was given. The two-cell form makes a picture
/// stretch between two corners, so widening a column distorts a logo — which
/// is exactly the silent squashing this engine refuses to do on paper.
pub(crate) fn drawing(sheet: &Sheet, stored: &[Stored]) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str(
        concat!(
            r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing""#,
            r#" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#,
            r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        ),
    );

    for (index, picture) in sheet.pictures.iter().enumerate() {
        let Some(image) = stored.iter().find(|s| s.name == picture.image) else {
            continue;
        };
        let (cx, cy) = extent(picture, image);
        let (dx, dy) = placed(sheet, picture, image);
        // Ids are one-based and unique within the drawing. Excel shows the
        // name in the selection pane, so it says which picture it is rather
        // than "Picture 1" four times.
        let id = index + 2;
        let rel = local_rel(image, stored);

        xml.push_str("<xdr:oneCellAnchor>");
        xml.push_str(&format!(
            concat!(
                "<xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>{dx}</xdr:colOff>",
                "<xdr:row>{row}</xdr:row><xdr:rowOff>{dy}</xdr:rowOff></xdr:from>",
                r#"<xdr:ext cx="{cx}" cy="{cy}"/>"#,
            ),
            col = picture.column,
            row = picture.row,
            dx = emu(dx),
            dy = emu(dy),
            cx = cx,
            cy = cy,
        ));
        xml.push_str(&format!(
            concat!(
                "<xdr:pic><xdr:nvPicPr>",
                r#"<xdr:cNvPr id="{id}" name="{name}"/>"#,
                r#"<xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr>"#,
                "</xdr:nvPicPr>",
                r#"<xdr:blipFill><a:blip r:embed="rId{rel}"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill>"#,
                r#"<xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>"#,
                "<xdr:clientData/></xdr:oneCellAnchor>",
            ),
            id = id,
            name = escaped(&picture.image),
            rel = rel,
            cx = cx,
            cy = cy,
        ));
    }

    xml.push_str("</xdr:wsDr>");
    xml
}

/// The relationships of one sheet's drawing, pointing into the media folder.
pub(crate) fn drawing_rels(stored: &[Stored]) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (index, image) in stored.iter().enumerate() {
        xml.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/{}"/>"#,
            index + 1,
            image.part,
        ));
    }
    xml.push_str("</Relationships>");
    xml
}

/// The deepest row whose height some picture on this sheet needs.
///
/// `None` when nothing is placed down the page, which is every picture until
/// somebody writes `valign` — and that is what lets a streaming session forget
/// each row the moment it has been written. Only a block a picture is centred
/// or bottomed inside has to be measured, and a block is a letterhead's worth
/// of rows rather than a sheet's.
pub(crate) fn deepest_row(sheet: &Sheet) -> Option<u32> {
    sheet
        .pictures
        .iter()
        .filter(|picture| !picture.valign.is_start())
        .map(|picture| sheet.block(picture.row, picture.column).2)
        .max()
}

/// Which relationship id in the drawing part points at this image.
///
/// Every drawing declares the whole media list rather than only what it uses.
/// It costs one line per image in a file nobody reads, and it means the id an
/// anchor names does not depend on which sheet the picture landed on.
fn local_rel(image: &Stored, stored: &[Stored]) -> usize {
    stored
        .iter()
        .position(|s| s.name == image.name)
        .unwrap_or(0)
        + 1
}

/// How big the picture is, in EMU.
///
/// The width is what the author asked for and the height comes from the
/// image's own pixels, exactly as `<Image width>` behaves on a page. Asking
/// for both is how a logo gets squashed by somebody who typed the old one's
/// numbers.
fn extent(picture: &Picture, image: &Stored) -> (i64, i64) {
    let (width, height) = size(picture, image);
    (emu(width), emu(height))
}

/// Where the picture starts inside the cell it hangs from.
///
/// The block is the merge that swallowed the anchor, not the anchor alone: a
/// logo hangs off `A1` and the author combined `A1:B4` to make room for it, so
/// centring in `A1` would put it in the corner of what the eye sees as one
/// cell. The author's own `dx`/`dy` are a nudge from wherever the placement put
/// it, not an alternative to it.
fn placed(sheet: &Sheet, picture: &Picture, image: &Stored) -> (f64, f64) {
    let (top, left, bottom, right) = sheet.block(picture.row, picture.column);
    let (width, height) = size(picture, image);

    (
        picture
            .align
            .offset(sheet.columns_points(left, right), width)
            + picture.dx,
        picture
            .valign
            .offset(sheet.rows_points(top, bottom), height)
            + picture.dy,
    )
}

/// How big the picture is, in points: the width asked for and the height its
/// own pixels give it.
fn size(picture: &Picture, image: &Stored) -> (f64, f64) {
    let ratio = f64::from(image.info.height) / f64::from(image.info.width);
    (picture.width, picture.width * ratio)
}

fn emu(points: f64) -> i64 {
    (points * EMU_PER_PT).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Column, Merge, Picture, Placement, Row, Sheet};

    const LOGO: &[u8] = include_bytes!("../tests/images/logo.png");

    fn logo() -> Image {
        Image::new("logo", LOGO)
    }

    fn sheet_with(pictures: Vec<Picture>) -> Sheet {
        Sheet {
            name: "Hoja".into(),
            pictures,
            ..Sheet::default()
        }
    }

    fn at_origin(image: &str, width: f64) -> Picture {
        Picture {
            image: image.into(),
            row: 0,
            column: 0,
            dx: 0.0,
            dy: 0.0,
            width,
            ..Picture::default()
        }
    }

    #[test]
    fn a_picture_keeps_the_ratio_of_its_own_pixels() {
        // The logo is 240×80, so three points wide is one point tall. Asking
        // for a height as well as a width is the one way to squash it, and
        // there is deliberately no way to ask.
        let sheets = [sheet_with(vec![at_origin("logo", 120.0)])];
        let stored = stored(&sheets, &[logo()]).expect("the image was handed over");
        let xml = drawing(&sheets[0], &stored);

        // 120 pt × 12 700 = 1 524 000; a third of that for the height.
        assert!(
            xml.contains(r#"<xdr:ext cx="1524000" cy="508000"/>"#),
            "{xml}"
        );
    }

    #[test]
    fn a_picture_hangs_off_the_cell_it_names() {
        let sheets = [sheet_with(vec![Picture {
            image: "logo".into(),
            row: 3,
            column: 2,
            dx: 6.0,
            dy: 1.5,
            width: 60.0,
            ..Picture::default()
        }])];
        let stored = stored(&sheets, &[logo()]).expect("the image was handed over");
        let xml = drawing(&sheets[0], &stored);

        assert!(xml.contains("<xdr:col>2</xdr:col>"), "{xml}");
        assert!(xml.contains("<xdr:row>3</xdr:row>"), "{xml}");
        // The offset into the cell is a length, so it is EMU like everything
        // else in a drawing: 6 pt is 76 200.
        assert!(xml.contains("<xdr:colOff>76200</xdr:colOff>"), "{xml}");
        assert!(xml.contains("<xdr:rowOff>19050</xdr:rowOff>"), "{xml}");
    }

    #[test]
    fn a_picture_is_anchored_to_one_cell_and_not_stretched_between_two() {
        // `twoCellAnchor` makes the picture span from one corner to another,
        // so widening a column distorts a logo. Nothing in this engine
        // squashes an image to fit, on paper or here.
        let sheets = [sheet_with(vec![at_origin("logo", 60.0)])];
        let stored = stored(&sheets, &[logo()]).expect("the image was handed over");
        let xml = drawing(&sheets[0], &stored);

        assert!(xml.contains("<xdr:oneCellAnchor>"), "{xml}");
        assert!(!xml.contains("twoCellAnchor"), "{xml}");
        assert!(xml.contains(r#"<a:picLocks noChangeAspect="1"/>"#), "{xml}");
    }

    #[test]
    fn an_image_nobody_names_is_not_carried_in_the_package() {
        // A caller with a folder of logos hands over what it has and uses one.
        // Shipping the rest inside every export is a bigger file for nothing.
        let sheets = [sheet_with(vec![at_origin("logo", 60.0)])];
        let images = vec![logo(), Image::new("otro", LOGO)];

        let stored = stored(&sheets, &images).expect("the used image was handed over");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "logo");
    }

    #[test]
    fn the_same_image_on_two_sheets_is_stored_once() {
        let sheets = [
            sheet_with(vec![at_origin("logo", 60.0)]),
            sheet_with(vec![at_origin("logo", 90.0)]),
        ];
        let stored = stored(&sheets, &[logo()]).expect("the image was handed over");
        assert_eq!(stored.len(), 1, "one media part, two anchors");
    }

    #[test]
    fn a_centred_picture_sits_in_the_middle_of_the_block_it_hangs_from() {
        // The block is the merge, not the anchor cell. A logo hangs off `A1`
        // and the author combined `A1:B4` to make room for it; centring in
        // `A1` alone would put it in the top-left corner of what the eye sees.
        //
        // A is 13 characters and B is 52, so the block is 348.75 pt across —
        // and four default rows make it 60 pt down. The logo is 120 × 40.
        let sheet = Sheet {
            name: "Hoja".into(),
            columns: vec![
                Column {
                    width: Some(13.0),
                    style: None,
                },
                Column {
                    width: Some(52.0),
                    style: None,
                },
            ],
            rows: vec![
                Row::default(),
                Row::default(),
                Row::default(),
                Row::default(),
            ],
            merges: vec![Merge {
                from_row: 0,
                from_column: 0,
                to_row: 3,
                to_column: 1,
            }],
            pictures: vec![Picture {
                image: "logo".into(),
                width: 120.0,
                align: Placement::Center,
                valign: Placement::Center,
                ..at_origin("logo", 120.0)
            }],
            ..Sheet::default()
        };
        let stored =
            stored(std::slice::from_ref(&sheet), &[logo()]).expect("the image was handed over");
        let xml = drawing(&sheet, &stored);

        // (348.75 - 120) / 2 = 114.375 pt, which is 1 452 563 EMU.
        assert!(xml.contains("<xdr:colOff>1452563</xdr:colOff>"), "{xml}");
        // (60 - 40) / 2 = 10 pt, which is 127 000.
        assert!(xml.contains("<xdr:rowOff>127000</xdr:rowOff>"), "{xml}");
    }

    #[test]
    fn a_picture_nobody_placed_still_sits_in_the_corner() {
        // The default has to be what it was before there was a choice, or
        // every sheet already written moves.
        let sheets = [sheet_with(vec![at_origin("logo", 120.0)])];
        let stored = stored(&sheets, &[logo()]).expect("the image was handed over");
        let xml = drawing(&sheets[0], &stored);

        assert!(xml.contains("<xdr:colOff>0</xdr:colOff>"), "{xml}");
        assert!(xml.contains("<xdr:rowOff>0</xdr:rowOff>"), "{xml}");
    }

    #[test]
    fn a_picture_wider_than_its_block_is_not_pushed_off_the_sheet() {
        // Centring something bigger than the room it has gives a negative
        // offset, and a negative one puts it past the left edge — where it
        // cannot be seen and cannot be dragged back.
        let sheet = Sheet {
            name: "Hoja".into(),
            columns: vec![Column {
                width: Some(4.0),
                style: None,
            }],
            pictures: vec![Picture {
                image: "logo".into(),
                width: 400.0,
                align: Placement::Center,
                valign: Placement::Center,
                ..at_origin("logo", 400.0)
            }],
            ..Sheet::default()
        };
        let stored =
            stored(std::slice::from_ref(&sheet), &[logo()]).expect("the image was handed over");
        let xml = drawing(&sheet, &stored);

        assert!(xml.contains("<xdr:colOff>0</xdr:colOff>"), "{xml}");
        assert!(xml.contains("<xdr:rowOff>0</xdr:rowOff>"), "{xml}");
    }

    #[test]
    fn an_offset_the_author_gave_is_added_to_where_it_was_placed() {
        // The two are not alternatives: `dx` is a nudge from wherever the
        // placement put it, so a centred logo can still be pushed off the
        // rule it sits on.
        let sheet = Sheet {
            name: "Hoja".into(),
            columns: vec![Column {
                width: Some(30.0),
                style: None,
            }],
            pictures: vec![Picture {
                image: "logo".into(),
                width: 100.0,
                dx: 5.0,
                align: Placement::Center,
                ..at_origin("logo", 100.0)
            }],
            ..Sheet::default()
        };
        let stored =
            stored(std::slice::from_ref(&sheet), &[logo()]).expect("the image was handed over");
        let xml = drawing(&sheet, &stored);

        // A column of 30 characters is 161.25 pt; (161.25 - 100) / 2 + 5 is
        // 35.625 pt, which is 452 438 EMU.
        assert!(xml.contains("<xdr:colOff>452438</xdr:colOff>"), "{xml}");
    }

    #[test]
    fn a_picture_naming_an_image_nobody_handed_over_is_refused() {
        // The alternative is a workbook with a hole where the logo was, which
        // nobody notices until a customer opens it.
        let sheets = [sheet_with(vec![at_origin("membrete", 60.0)])];
        let why = stored(&sheets, &[logo()]).expect_err("nothing was handed over under that name");

        assert_eq!(why, PictureError::Missing("membrete".into()));
    }

    #[test]
    fn something_that_is_not_an_image_is_refused_by_name() {
        let sheets = [sheet_with(vec![at_origin("logo", 60.0)])];
        let why = stored(
            &sheets,
            &[Image::new("logo", b"no soy una imagen".to_vec())],
        )
        .expect_err("those bytes are not a PNG");

        assert!(matches!(why, PictureError::Unreadable(name, _) if name == "logo"));
    }

    #[test]
    fn a_media_part_is_named_after_what_it_turned_out_to_be() {
        // The extension in the package has to match the content type declared
        // for it, and the bytes decide — not what the caller called the image.
        let sheets = [sheet_with(vec![at_origin("logo", 60.0)])];
        let stored = stored(&sheets, &[Image::new("logo", LOGO)]).expect("a PNG");

        assert_eq!(stored[0].part, "image1.png");
    }
}
