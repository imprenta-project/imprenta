//! The Imprenta PDF engine.

/// The finished file as [`build::Built`] and [`compose::Composed`] carry it:
/// blocks that read like the `Vec<u8>` they replaced, joined only on demand.
/// Re-exported so a consumer of this crate never has to name the writer.
pub use imprenta_pdf_write::Pdf;

pub mod atom;
pub mod build;
pub mod compose;
pub mod content;
pub mod decoration;
pub mod ir;
pub mod list;
pub mod measure;
pub mod pack;
pub mod parallel;
pub mod render;
pub mod session;
pub mod shape;
pub mod table;
pub mod widows;
