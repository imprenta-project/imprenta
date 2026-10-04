use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    #[serde(default)]
    pub profile: Profile,
    pub children: Vec<Node>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub columns: Option<u16>,
    pub printable_width: Option<u16>,
    pub codepage: Option<String>,
    pub codepage_id: Option<u8>,
    pub dpi: Option<u16>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProfile {
    pub columns: u16,
    pub printable_width: u16,
    pub codepage: String,
    pub codepage_id: u8,
    pub dpi: u16,
    pub margin_left: u16,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub enum Font {
    #[default]
    A,
    B,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub enum Underline {
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "one-dot-thick")]
    Single,
    #[serde(rename = "two-dot-thick")]
    Double,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    #[serde(default)]
    pub justification: Align,
    #[serde(default)]
    pub font: Font,
    pub font_width: Option<u8>,
    pub font_height: Option<u8>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub underline: Underline,
    #[serde(default)]
    pub inverse: bool,
    #[serde(default)]
    pub double_strike: bool,
    pub line_spacing: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Column {
    pub text: String,
    pub width: Option<u16>,
    #[serde(default)]
    pub justification: Align,
    #[serde(default)]
    pub overflow: Overflow,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Overflow {
    #[default]
    Wrap,
    Error,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CutMode {
    Full,
    #[default]
    Partial,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Node {
    Init {
        width: Option<f64>,
        margin_left: Option<f64>,
        dpi: Option<u16>,
    },
    Text {
        text: String,
        #[serde(flatten)]
        style: Style,
        lf: Option<bool>,
        prefix: Option<String>,
        suffix: Option<String>,
        format: Option<String>,
    },
    Row {
        columns: Vec<Column>,
        gap: Option<u16>,
        #[serde(flatten)]
        style: Style,
    },
    Rule {
        character: Option<String>,
        #[serde(flatten)]
        style: Style,
    },
    Qrcode {
        value: String,
        model: Option<u8>,
        size: Option<u8>,
        error_correction_level: Option<String>,
        #[serde(default)]
        justification: Align,
    },
    Barcode {
        value: String,
        #[serde(rename = "barcodeType")]
        barcode_type: Option<String>,
        width: Option<u8>,
        height: Option<u8>,
        hri_position: Option<String>,
        hri_font: Option<Font>,
        #[serde(default)]
        justification: Align,
    },
    Pdf417 {
        value: String,
        width: Option<u8>,
        height: Option<u8>,
        number_of_columns: Option<u8>,
        number_of_rows: Option<u8>,
        error_correction_level: Option<u8>,
        option: Option<String>,
        #[serde(default)]
        justification: Align,
    },
    Image {
        src: String,
        width: u16,
        height: Option<u16>,
        filter: Option<String>,
        threshold: Option<u8>,
        #[serde(default)]
        justification: Align,
    },
    Feed {
        lines: Option<u8>,
    },
    Cut {
        #[serde(default)]
        mode: CutMode,
    },
    Bytes {
        hex: String,
    },
}
