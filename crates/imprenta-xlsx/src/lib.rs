//! The Imprenta spreadsheet writer.
//!
//! A workbook model, and the OOXML package it becomes. There is no measuring
//! phase, no pagination and no painting: a spreadsheet has no page, and what a
//! cell looks like is decided by Excel when somebody opens it. This crate's
//! whole job is to say, exactly and in the right XML, **what is in each cell
//! and what type it is**.
//!
//! That is why it is a separate crate from the PDF engine rather than another
//! output of it. The two share the vocabulary in `imprenta-core` — lengths,
//! colour, diagnostics, the versioned envelope — and share no model at all.

pub mod ir;
mod package;
pub mod picture;
pub mod serial;
pub mod session;
mod sheet;
pub mod style;
mod xml;

pub use package::{Error, write, write_to_file};
pub use picture::{Image, PictureError};
pub use session::Session;
