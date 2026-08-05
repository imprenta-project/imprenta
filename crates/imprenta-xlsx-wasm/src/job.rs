//! One workbook, described in terms a host can hand over.
//!
//! Everything here is ordinary Rust. The ABI above translates pointers into
//! these types and the result back into pointers, and does nothing else — so
//! the behaviour that matters is tested with `cargo test` on the host, with no
//! WebAssembly runtime anywhere near it.

use imprenta_xlsx::ir::Workbook;

/// What came of a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub xlsx: Vec<u8>,
    pub sheets: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("the workbook is not valid JSON: {0}")]
    Malformed(String),
    #[error("{0}")]
    Write(#[from] imprenta_xlsx::Error),
    #[error("{0}")]
    OutOfOrder(String),
}

/// Parses a declared workbook and writes it.
///
/// There is no path-taking variant, unlike the Node binding. A WebAssembly
/// module has no filesystem — which is the point of it — so the bytes always
/// come back through linear memory and the host decides where they go.
pub fn run(ir: &[u8]) -> Result<Outcome, JobError> {
    let book: Workbook =
        serde_json::from_slice(ir).map_err(|e| JobError::Malformed(e.to_string()))?;
    let sheets = book.sheets.len();
    let xlsx = imprenta_xlsx::write(&book)?;
    Ok(Outcome { xlsx, sheets })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOK: &[u8] = br#"{
        "sheets": [{
            "name": "Ventas",
            "rows": [
                { "cells": [{ "value": { "t": "text", "v": "Concepto" } }, { "value": { "t": "text", "v": "Importe" } }] },
                { "cells": [{ "value": { "t": "text", "v": "Servicios" } }, { "value": { "t": "number", "v": 1200 } }] }
            ]
        }]
    }"#;

    #[test]
    fn a_declared_workbook_comes_back_as_a_package() {
        let outcome = run(BOOK).unwrap();

        // An OOXML package is a zip, and a zip starts with "PK".
        assert_eq!(&outcome.xlsx[..2], b"PK");
        assert_eq!(outcome.sheets, 1);
    }

    #[test]
    fn the_bytes_are_the_ones_the_writer_produces_directly() {
        // The ABI is a way in, never a second writer. If this diverges, a
        // workbook depends on which binding produced it.
        let book: Workbook = serde_json::from_slice(BOOK).unwrap();
        let direct = imprenta_xlsx::write(&book).unwrap();

        assert_eq!(run(BOOK).unwrap().xlsx, direct);
    }

    #[test]
    fn writing_twice_gives_the_same_workbook_twice() {
        let first = run(BOOK).unwrap();
        let second = run(BOOK).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn a_malformed_workbook_is_an_error_and_not_a_panic() {
        let err = run(b"{ not json").unwrap_err();

        assert!(matches!(err, JobError::Malformed(_)), "{err:?}");
    }

    #[test]
    fn a_number_stays_a_number() {
        // The whole reason this crate exists rather than a CSV: a value
        // written as text makes `SUM` return zero, and nothing about the file
        // looks wrong until somebody adds a formula.
        use calamine::{Data, Reader, Xlsx};
        use std::io::Cursor;

        let outcome = run(BOOK).unwrap();

        let mut read = Xlsx::new(Cursor::new(outcome.xlsx)).unwrap();
        let sheet = read.worksheet_range("Ventas").unwrap();
        let value = sheet.get_value((1, 1)).unwrap();
        assert!(
            matches!(value, Data::Float(f) if (*f - 1200.0).abs() < f64::EPSILON),
            "got {value:?}"
        );
    }
}
