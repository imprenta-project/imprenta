//! One render, described in terms Node can hand over.
//!
//! Everything here is ordinary Rust. The napi layer above translates JS values
//! into a [`Job`] and the result back again, and does nothing else — so the
//! behaviour that matters can be tested without starting a Node process.

use imprenta_pdf::build::{Assets, build};
use imprenta_pdf::ir::Document;
use imprenta_pdf::render::Options;
use imprenta_pdf::shape::{Face, Weight};
use std::path::PathBuf;

/// A typeface the document may ask for, and the file behind it.
#[derive(Debug, Clone)]
pub struct FontInput {
    pub weight: String,
    pub italic: bool,
    pub data: Vec<u8>,
}

/// An image the document refers to by name.
#[derive(Debug, Clone)]
pub struct ImageInput {
    pub name: String,
    pub data: Vec<u8>,
}

/// Where the finished PDF should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Back to the caller. Costs a copy into the JS heap.
    Buffer,
    /// Straight to disk from Rust, so the bytes never reach the JS heap.
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Job {
    pub ir: String,
    pub fonts: Vec<FontInput>,
    pub images: Vec<ImageInput>,
    pub output: Output,
}

/// What came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Present only when the job asked for a buffer.
    pub pdf: Option<Vec<u8>>,
    pub bytes: usize,
    pub pages: usize,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("the document is not valid JSON: {0}")]
    Malformed(String),
    #[error("no fonts were supplied, and a document cannot be set without one")]
    NoFonts,
    #[error("unknown font weight {0:?}; expected \"regular\" or \"bold\"")]
    UnknownWeight(String),
    #[error("image {name:?} could not be read: {reason}")]
    BadImage { name: String, reason: String },
    #[error("could not write {path}: {reason}")]
    Unwritable { path: String, reason: String },
    #[error("{0}")]
    Build(String),
}

pub fn run(mut job: Job) -> Result<Outcome, JobError> {
    // Everything the caller could have got wrong is checked before a glyph is
    // shaped, so a typo in a font weight is not discovered forty seconds into
    // a fifty-thousand-page run.
    // Parsed, then the text is dropped before anything is measured. A long
    // ledger arrives as tens of megabytes of JSON, and holding it for the
    // whole render buys nothing once the tree exists.
    let document: Document = {
        let ir = std::mem::take(&mut job.ir);
        serde_json::from_str(&ir).map_err(|e| JobError::Malformed(e.to_string()))?
    };

    if job.fonts.is_empty() {
        return Err(JobError::NoFonts);
    }

    let mut assets = Assets::new();
    for font in job.fonts {
        assets = assets.with_font(face(&font)?, font.data);
    }
    assets = assets_from(assets, job.images)?;

    let built = build(&document, &assets, Options::default())
        .map_err(|e| JobError::Build(e.to_string()))?;

    let bytes = built.pdf.len();
    let pdf = match job.output {
        Output::Buffer => Some(built.pdf),
        Output::File(path) => {
            // Written here rather than handed back, so the peak stays at the
            // one copy the engine already holds.
            std::fs::write(&path, &built.pdf).map_err(|e| JobError::Unwritable {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;
            None
        }
    };

    Ok(Outcome {
        pdf,
        bytes,
        pages: built.pages,
        diagnostics: built.diagnostics,
    })
}

/// Adds every image to `assets`, naming the one that could not be read.
pub fn assets_from(
    mut assets: Assets,
    images: impl IntoIterator<Item = ImageInput>,
) -> Result<Assets, JobError> {
    for image in images {
        assets = assets
            .with_image(&image.name, image.data)
            .map_err(|e| JobError::BadImage {
                name: image.name,
                reason: e.to_string(),
            })?;
    }
    Ok(assets)
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

    const HELLO: &str = r#"{
        "page": { "width": 595, "height": 842 },
        "children": [{ "t": "text", "runs": [{ "text": "Hola" }] }]
    }"#;

    fn regular() -> FontInput {
        FontInput {
            weight: "regular".into(),
            italic: false,
            data: ROBOTO.to_vec(),
        }
    }

    fn job(ir: &str) -> Job {
        Job {
            ir: ir.into(),
            fonts: vec![regular()],
            images: vec![],
            output: Output::Buffer,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("imprenta-job-{name}.pdf"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_declared_document_comes_back_as_a_pdf() {
        let outcome = run(job(HELLO)).unwrap();

        let pdf = outcome.pdf.expect("a buffer was asked for");
        assert!(pdf.starts_with(b"%PDF-"), "not a PDF");
        assert_eq!(outcome.pages, 1);
        assert_eq!(outcome.bytes, pdf.len());
    }

    #[test]
    fn writing_to_a_file_keeps_the_bytes_out_of_the_callers_heap() {
        // The reason this variant exists: a 50,000-page ledger is 128 MB, and
        // handing it back as a Buffer costs that much again on the JS side
        // for a caller whose next move is to write it to disk anyway.
        let path = scratch("to-file");

        let outcome = run(Job {
            output: Output::File(path.clone()),
            ..job(HELLO)
        })
        .unwrap();

        assert!(outcome.pdf.is_none(), "the bytes were handed back anyway");
        assert!(outcome.bytes > 0, "the size is still reported");
        assert_eq!(
            std::fs::metadata(&path).unwrap().len() as usize,
            outcome.bytes
        );
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-"));
    }

    #[test]
    fn both_routes_produce_the_same_document() {
        let path = scratch("same");

        let buffered = run(job(HELLO)).unwrap();
        run(Job {
            output: Output::File(path.clone()),
            ..job(HELLO)
        })
        .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), buffered.pdf.unwrap());
    }

    #[test]
    fn a_second_face_is_available_to_the_document() {
        let bold = r#"{
            "page": { "width": 595, "height": 842 },
            "children": [{ "t": "text", "runs": [
                { "text": "Total " },
                { "text": "7.400,00", "weight": "bold" }
            ] }]
        }"#;

        let outcome = run(Job {
            fonts: vec![
                regular(),
                FontInput {
                    weight: "bold".into(),
                    italic: false,
                    data: ROBOTO_BOLD.to_vec(),
                },
            ],
            ..job(bold)
        })
        .unwrap();

        assert!(outcome.diagnostics.is_empty(), "{:?}", outcome.diagnostics);
        assert_eq!(outcome.pages, 1);
    }

    #[test]
    fn an_image_is_supplied_as_bytes_and_nothing_else() {
        // No format, no dimensions: Node has no idea what is in the file and
        // should not have to pretend otherwise.
        let with_logo = r#"{
            "page": { "width": 595, "height": 842 },
            "children": [{ "t": "image", "src": "logo", "width": 120 }]
        }"#;

        let outcome = run(Job {
            images: vec![ImageInput {
                name: "logo".into(),
                data: LOGO.to_vec(),
            }],
            ..job(with_logo)
        })
        .unwrap();

        assert_eq!(outcome.pages, 1);
        assert!(outcome.diagnostics.is_empty(), "{:?}", outcome.diagnostics);
    }

    // ── what the caller gets wrong ──────────────────────────────────────

    #[test]
    fn malformed_json_says_where_it_went_wrong() {
        let broken = run(job(r#"{ "page": { "width": 595, }}"#)).unwrap_err();

        let message = broken.to_string();
        assert!(
            message.contains("line") || message.contains("column"),
            "no position in {message:?}"
        );
    }

    #[test]
    fn a_document_with_no_fonts_is_refused_before_anything_is_rendered() {
        let bare = run(Job {
            fonts: vec![],
            ..job(HELLO)
        })
        .unwrap_err();

        assert!(matches!(bare, JobError::NoFonts));
    }

    #[test]
    fn an_unknown_weight_names_the_ones_that_exist() {
        // A caller reaching for "semibold" has to be told, not silently given
        // regular and left to wonder why the heading is thin.
        let wrong = run(Job {
            fonts: vec![FontInput {
                weight: "semibold".into(),
                italic: false,
                data: ROBOTO.to_vec(),
            }],
            ..job(HELLO)
        })
        .unwrap_err();

        let message = wrong.to_string();
        assert!(message.contains("semibold"), "{message}");
        assert!(message.contains("bold"), "{message}");
    }

    #[test]
    fn a_weight_is_matched_whatever_the_caller_capitalised() {
        let shouted = run(Job {
            fonts: vec![FontInput {
                weight: "Bold".into(),
                italic: false,
                data: ROBOTO_BOLD.to_vec(),
            }],
            ..job(HELLO)
        });

        assert!(shouted.is_ok(), "{:?}", shouted.err());
    }

    #[test]
    fn an_unreadable_image_names_the_asset_it_came_in_as() {
        // "not a PNG or JPEG" alone is useless when the document carries
        // eleven images and one of them is a stray SVG.
        let broken = run(Job {
            images: vec![ImageInput {
                name: "sello".into(),
                data: b"<svg/>".to_vec(),
            }],
            ..job(HELLO)
        })
        .unwrap_err();

        assert!(broken.to_string().contains("sello"), "{broken}");
    }

    #[test]
    fn an_image_the_document_never_asked_for_is_no_error() {
        // Handing over a whole asset library and using two of it is normal.
        let outcome = run(Job {
            images: vec![ImageInput {
                name: "unused".into(),
                data: LOGO.to_vec(),
            }],
            ..job(HELLO)
        });

        assert!(outcome.is_ok(), "{:?}", outcome.err());
    }

    #[test]
    fn a_path_that_cannot_be_written_says_so_and_says_where() {
        let nowhere = PathBuf::from("/no/such/directory/out.pdf");

        let refused = run(Job {
            output: Output::File(nowhere),
            ..job(HELLO)
        })
        .unwrap_err();

        let message = refused.to_string();
        assert!(message.contains("/no/such/directory/out.pdf"), "{message}");
    }

    #[test]
    fn what_the_engine_noticed_reaches_the_caller() {
        // A character no supplied font can draw. The page still renders — the
        // caller decides whether a missing glyph is worth failing over — but
        // it must not disappear silently.
        let unsupported = r#"{
            "page": { "width": 595, "height": 842 },
            "children": [{ "t": "text", "runs": [{ "text": "日本語" }] }]
        }"#;

        let outcome = run(job(unsupported)).unwrap();

        assert!(!outcome.diagnostics.is_empty(), "nothing was reported");
    }
}
