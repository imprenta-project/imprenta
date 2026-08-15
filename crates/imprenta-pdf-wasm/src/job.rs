//! One render, described in terms a host can hand over.
//!
//! Everything here is ordinary Rust. The ABI above translates pointers into
//! these types and the result back into pointers, and does nothing else — so
//! the behaviour that matters is tested with `cargo test` on the host, with no
//! WebAssembly runtime anywhere near it.

use imprenta_pdf::build::{Assets, build};
use imprenta_pdf::ir::Document;
use imprenta_pdf::render::Options;
use imprenta_pdf::shape::{Face, Weight};

/// A typeface the document may ask for, and the file behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontInput {
    /// `"regular"` or `"bold"`. Empty means regular.
    pub weight: String,
    pub italic: bool,
    pub data: Vec<u8>,
}

/// An image the document refers to by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInput {
    pub name: String,
    pub data: Vec<u8>,
}

/// What came of a render.
///
/// The file stays in the blocks the writer produced — see
/// [`imprenta_pdf::Pdf`] — and the host reads them one at a time
/// through `imprenta_out_block_ptr`. Joining them here would put a second
/// copy of the document in linear memory, which never shrinks, at exactly the
/// moment the first is at its largest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub pdf: imprenta_pdf::Pdf,
    pub pages: usize,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JobError {
    #[error("the document is not valid JSON: {0}")]
    Malformed(String),
    #[error("no fonts were supplied, and a document cannot be set without one")]
    NoFonts,
    #[error("unknown font weight {0:?}; expected \"regular\" or \"bold\"")]
    UnknownWeight(String),
    #[error("image {name:?} could not be read: {reason}")]
    BadImage { name: String, reason: String },
    #[error("{0}")]
    Build(String),
}

/// The bytes a document may refer to, held across renders.
///
/// A warm instance renders one chunk after another and the fonts do not change
/// between them, so the host copies them into linear memory once at boot
/// rather than once per chunk. That is the whole reason this is a type and not
/// an argument.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    pub fonts: Vec<FontInput>,
    pub images: Vec<ImageInput>,
}

impl Library {
    pub fn clear(&mut self) {
        self.fonts.clear();
        self.images.clear();
    }

    /// Resolves into what the engine wants, checking everything the host could
    /// have got wrong before a single glyph is shaped.
    pub fn assets(&self) -> Result<Assets, JobError> {
        if self.fonts.is_empty() {
            return Err(JobError::NoFonts);
        }
        let mut assets = Assets::new();
        for font in &self.fonts {
            assets = assets.with_font(face(font)?, font.data.clone());
        }
        for image in &self.images {
            assets = assets
                .with_image(&image.name, image.data.clone())
                .map_err(|e| JobError::BadImage {
                    name: image.name.clone(),
                    reason: e.to_string(),
                })?;
        }
        Ok(assets)
    }
}

/// Renders a whole declared document.
pub fn run(ir: &[u8], library: &Library) -> Result<Outcome, JobError> {
    let assets = library.assets()?;
    // Parsed from the bytes the host wrote, and dropped before anything is
    // measured: a long ledger arrives as tens of megabytes of JSON and holding
    // it for the whole render buys nothing once the tree exists.
    let document: Document =
        serde_json::from_slice(ir).map_err(|e| JobError::Malformed(e.to_string()))?;

    let built = build(&document, &assets, Options::default())
        .map_err(|e| JobError::Build(e.to_string()))?;

    Ok(Outcome {
        pdf: built.pdf,
        pages: built.pages,
        diagnostics: built.diagnostics,
    })
}

pub fn face(font: &FontInput) -> Result<Face, JobError> {
    // Case-folded because "Bold" is what a caller writes when the surrounding
    // API is CSS-shaped, and refusing it teaches nothing.
    let weight = match font.weight.to_ascii_lowercase().as_str() {
        "regular" | "normal" | "" => Weight::Regular,
        "bold" => Weight::Bold,
        _ => return Err(JobError::UnknownWeight(font.weight.clone())),
    };
    Ok(Face {
        weight,
        italic: font.italic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBOTO: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Regular.ttf");
    const ROBOTO_BOLD: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Bold.ttf");
    const LOGO: &[u8] = include_bytes!("../../imprenta-pdf/tests/images/logo.png");

    const HELLO: &[u8] = br#"{
        "page": { "width": 595, "height": 842 },
        "children": [{ "t": "text", "runs": [{ "text": "Hola" }] }]
    }"#;

    fn roman() -> Library {
        Library {
            fonts: vec![FontInput {
                weight: "regular".into(),
                italic: false,
                data: ROBOTO.to_vec(),
            }],
            images: vec![],
        }
    }

    #[test]
    fn a_declared_document_comes_back_as_a_pdf() {
        let outcome = run(HELLO, &roman()).unwrap();

        assert_eq!(&outcome.pdf[..5], b"%PDF-");
        assert_eq!(outcome.pages, 1);
    }

    #[test]
    fn the_bytes_are_the_ones_the_engine_produces_directly() {
        // The ABI is a way in, never a second layout path. If this ever
        // diverges, a document would depend on which binding rendered it.
        let assets = roman().assets().unwrap();
        let document: Document = serde_json::from_slice(HELLO).unwrap();
        let direct = build(&document, &assets, Options::default()).unwrap();

        let through = run(HELLO, &roman()).unwrap();

        assert_eq!(through.pdf, direct.pdf);
        assert_eq!(through.pages, direct.pages);
    }

    #[test]
    fn rendering_twice_gives_the_same_document_twice() {
        // The napi/emnapi build wedged on the second call and rendered exactly
        // one document per process. Nothing about that was WebAssembly's
        // fault, but it is the failure this binding exists to not have.
        let library = roman();

        let first = run(HELLO, &library).unwrap();
        let second = run(HELLO, &library).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn a_malformed_document_is_an_error_and_not_a_panic() {
        let err = run(b"{ not json", &roman()).unwrap_err();

        assert!(matches!(err, JobError::Malformed(_)), "{err:?}");
    }

    #[test]
    fn a_document_with_no_fonts_says_so() {
        let empty = Library::default();

        assert_eq!(run(HELLO, &empty).unwrap_err(), JobError::NoFonts);
    }

    #[test]
    fn an_unknown_weight_names_itself() {
        let library = Library {
            fonts: vec![FontInput {
                weight: "semibold".into(),
                italic: false,
                data: ROBOTO.to_vec(),
            }],
            images: vec![],
        };

        assert_eq!(
            library.assets().unwrap_err(),
            JobError::UnknownWeight("semibold".into())
        );
    }

    #[test]
    fn an_unreadable_image_names_itself_rather_than_failing_the_document() {
        let library = Library {
            fonts: roman().fonts,
            images: vec![ImageInput {
                name: "logo".into(),
                data: b"not a picture".to_vec(),
            }],
        };

        match library.assets().unwrap_err() {
            JobError::BadImage { name, .. } => assert_eq!(name, "logo"),
            other => panic!("expected a BadImage, got {other:?}"),
        }
    }

    #[test]
    fn a_face_can_be_named_the_way_css_names_it() {
        for (given, expected) in [
            ("regular", Weight::Regular),
            ("Regular", Weight::Regular),
            ("normal", Weight::Regular),
            ("", Weight::Regular),
            ("bold", Weight::Bold),
            ("BOLD", Weight::Bold),
        ] {
            let font = FontInput {
                weight: given.into(),
                italic: false,
                data: vec![],
            };
            assert_eq!(face(&font).unwrap().weight, expected, "for {given:?}");
        }
    }

    #[test]
    fn a_family_and_its_pictures_survive_into_the_assets() {
        let library = Library {
            fonts: vec![
                FontInput {
                    weight: "regular".into(),
                    italic: false,
                    data: ROBOTO.to_vec(),
                },
                FontInput {
                    weight: "bold".into(),
                    italic: false,
                    data: ROBOTO_BOLD.to_vec(),
                },
            ],
            images: vec![ImageInput {
                name: "logo".into(),
                data: LOGO.to_vec(),
            }],
        };

        let assets = library.assets().unwrap();

        assert_eq!(assets.fonts.len(), 2);
        assert!(assets.images.contains_key("logo"));
    }

    #[test]
    fn what_the_engine_noticed_comes_back() {
        // A diagnostic that never reaches the host is a defect nobody sees:
        // the page looks deliberate.
        const MISSING_GLYPH: &str = r#"{
            "page": { "width": 595, "height": 842 },
            "children": [{ "t": "text", "runs": [{ "text": "日本語" }] }]
        }"#;

        let outcome = run(MISSING_GLYPH.as_bytes(), &roman()).unwrap();

        assert!(
            !outcome.diagnostics.is_empty(),
            "a Latin font covering no Japanese should have said so"
        );
    }
}
