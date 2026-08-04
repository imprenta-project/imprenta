//! The IR envelope: the outermost layer of every document handed to the engine.
//!
//! Every payload declares which **format** it targets and which **schema
//! version** it was produced against, before the engine looks at a single
//! node. Both are checked up front so that a mismatch is a named error rather
//! than a node quietly ignored halfway through a 9,000-page render.

use serde::{Deserialize, Serialize};

/// Schema version this build of the engine speaks.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Output format an IR payload targets.
///
/// Each one has its own crate and its own IR, because they do not share a
/// model: a PDF is measured and paginated here, and a spreadsheet has no page
/// at all. What they share is this handshake and the vocabulary in
/// [`crate::units`], [`crate::color`] and [`crate::diagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pdf,
    Xlsx,
}

/// Everything that can go wrong before the document itself is examined.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EnvelopeError {
    #[error(
        "unsupported output format {found:?}; this build supports: {}",
        supported.join(", ")
    )]
    UnsupportedFormat {
        found: String,
        supported: Vec<&'static str>,
    },

    #[error(
        "IR schema version {found} is newer than this engine understands \
         (version {supported}) — upgrade the Imprenta engine"
    )]
    SchemaTooNew { found: u32, supported: u32 },

    #[error(
        "IR schema version {found} is older than this engine supports \
         (version {supported}) — upgrade your @imprentajs/* packages"
    )]
    SchemaTooOld { found: u32, supported: u32 },

    #[error("malformed IR envelope: {0}")]
    Malformed(String),
}

/// A parsed, version-checked IR payload.
///
/// `document` stays as raw JSON: `imprenta-core` is format-neutral by design
/// and must not know the shape of a PDF document, so the format crate
/// deserialises it into its own IR once the envelope has been validated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub schema_version: u32,
    pub format: Format,
    pub document: serde_json::Value,
}

impl Format {
    /// Format names this build can produce, for error messages.
    pub const SUPPORTED: &'static [&'static str] = &["pdf", "xlsx"];

    fn from_name(name: &str) -> Result<Self, EnvelopeError> {
        match name {
            "pdf" => Ok(Self::Pdf),
            "xlsx" => Ok(Self::Xlsx),
            other => Err(EnvelopeError::UnsupportedFormat {
                found: other.to_string(),
                supported: Self::SUPPORTED.to_vec(),
            }),
        }
    }
}

/// The envelope exactly as it arrives on the wire.
///
/// `format` is read as a plain string rather than straight into [`Format`] so
/// that an unknown value produces a named [`EnvelopeError::UnsupportedFormat`]
/// instead of serde's generic "unknown variant".
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEnvelope {
    schema_version: u32,
    format: String,
    document: serde_json::Value,
}

impl Envelope {
    /// Parses and validates an IR payload.
    pub fn parse(json: &str) -> Result<Self, EnvelopeError> {
        let raw: RawEnvelope =
            serde_json::from_str(json).map_err(|e| EnvelopeError::Malformed(e.to_string()))?;

        if raw.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(EnvelopeError::SchemaTooNew {
                found: raw.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if raw.schema_version < CURRENT_SCHEMA_VERSION {
            return Err(EnvelopeError::SchemaTooOld {
                found: raw.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }

        Ok(Self {
            schema_version: raw.schema_version,
            format: Format::from_name(&raw.format)?,
            document: raw.document,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_current_version_pdf_envelope() {
        let json = r#"{
            "schemaVersion": 1,
            "format": "pdf",
            "document": { "t": "document", "children": [] }
        }"#;

        let env = Envelope::parse(json).expect("valid envelope should parse");

        assert_eq!(env.schema_version, 1);
        assert_eq!(env.format, Format::Pdf);
        assert_eq!(env.document["t"], "document");
    }

    #[test]
    fn parses_a_current_version_xlsx_envelope() {
        // A spreadsheet is not a document with different options — it has no
        // page, no margin and no pagination — so it gets its own format and
        // its own crate. What the envelope shares is the version handshake.
        let json = r#"{
            "schemaVersion": 1,
            "format": "xlsx",
            "document": { "t": "workbook", "sheets": [] }
        }"#;

        let env = Envelope::parse(json).expect("valid envelope should parse");

        assert_eq!(env.format, Format::Xlsx);
        assert_eq!(env.document["t"], "workbook");
    }

    #[test]
    fn rejects_an_unknown_format_by_name() {
        // The whole point of the discriminator: a payload aimed at a format
        // this build cannot produce must say so, naming what it can produce.
        let json = r#"{"schemaVersion": 1, "format": "pptx", "document": {}}"#;

        let err = Envelope::parse(json).expect_err("pptx is not a format we produce");

        assert_eq!(
            err,
            EnvelopeError::UnsupportedFormat {
                found: "pptx".to_string(),
                supported: vec!["pdf", "xlsx"],
            }
        );
        assert!(err.to_string().contains("pptx"));
        assert!(err.to_string().contains("pdf, xlsx"));
    }

    #[test]
    fn rejects_a_schema_version_newer_than_the_engine() {
        let json = r#"{"schemaVersion": 99, "format": "pdf", "document": {}}"#;

        let err = Envelope::parse(json).expect_err("version 99 is from the future");

        assert_eq!(
            err,
            EnvelopeError::SchemaTooNew {
                found: 99,
                supported: CURRENT_SCHEMA_VERSION,
            }
        );
        // The message must tell the user which side to upgrade.
        assert!(err.to_string().contains("upgrade the Imprenta engine"));
    }

    #[test]
    fn rejects_a_schema_version_older_than_the_engine() {
        let json = r#"{"schemaVersion": 0, "format": "pdf", "document": {}}"#;

        let err = Envelope::parse(json).expect_err("version 0 predates the engine");

        assert_eq!(
            err,
            EnvelopeError::SchemaTooOld {
                found: 0,
                supported: CURRENT_SCHEMA_VERSION,
            }
        );
        assert!(
            err.to_string()
                .contains("upgrade your @imprentajs/* packages")
        );
    }

    #[test]
    fn reports_malformed_json_rather_than_panicking() {
        let err = Envelope::parse("{ not json").expect_err("garbage must not panic");
        assert!(matches!(err, EnvelopeError::Malformed(_)));
    }

    #[test]
    fn requires_the_schema_version_field() {
        // A payload with no version is not "version 1 by default" — that is how
        // a stale producer silently renders against the wrong schema.
        let err = Envelope::parse(r#"{"format": "pdf", "document": {}}"#)
            .expect_err("missing schemaVersion must be rejected");
        assert!(matches!(err, EnvelopeError::Malformed(_)));
    }
}
