//! The Imprenta spreadsheet writer as a plain WebAssembly module.
//!
//! The same contract as `imprenta-pdf-wasm`, and a separate module for the
//! same reason the crates and the addons are separate: **share the
//! vocabulary, never the model.** A page is measured and every glyph on it was
//! placed here; a sheet has no page and nothing is painted, and a cell carries
//! a value and a type that Excel decides how to draw. Two engines that happen
//! to be reached the same way.
//!
//! Keeping them apart also removes a trap the native side has to live with:
//! two addons in one process cannot each declare a `#[global_allocator]`, and
//! `packages/xlsx/test/together.test.ts` exists to hold that line. Two
//! WebAssembly modules have a linear memory each and cannot collide at all.
//!
//! Numbers in, numbers out; anything larger travels through linear memory.
//! Every call returns `1` for success and `0` for failure, and a failure
//! leaves a message at [`imprenta_error_ptr`]. Nothing panics across the boundary: a
//! panic is an unrecoverable trap, and with a pool that means a dead worker.
//!
//! This file is glue and nothing else. What it delegates to lives in [`job`]
//! and [`stream`].

use std::cell::RefCell;

pub mod job;
pub mod stream;

use job::{JobError, Outcome};
use stream::Book;

thread_local! {
    /// The finished workbook, kept until the host has read it or released it.
    static OUT: RefCell<Outcome> = const {
        RefCell::new(Outcome { xlsx: Vec::new(), sheets: 0 })
    };
    /// Why the last call returned 0.
    static ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// The workbook being fed, if one is open.
    static BOOK: RefCell<Option<Book>> = const { RefCell::new(None) };
}

const OK: i32 = 1;
const FAILED: i32 = 0;

fn fail(e: JobError) -> i32 {
    ERROR.with(|slot| *slot.borrow_mut() = e.to_string().into_bytes());
    FAILED
}

fn succeed() -> i32 {
    ERROR.with(|slot| slot.borrow_mut().clear());
    OK
}

fn publish(outcome: Outcome) -> i32 {
    OUT.with(|slot| *slot.borrow_mut() = outcome);
    succeed()
}

// ── Memory ──────────────────────────────────────────────────────────────────

/// Memory the host may write into. Given back with [`imprenta_dealloc`].
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
///
/// `ptr` must have come from [`imprenta_alloc`] with the same `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_dealloc(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
    }
}

/// # Safety
///
/// `ptr` must point at `len` readable bytes.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

// ── One workbook, declared whole ────────────────────────────────────────────

/// Writes a declared workbook. Read the result with [`imprenta_out_ptr`].
///
/// # Safety
///
/// `ir_ptr` must point at `ir_len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_write(ir_ptr: *const u8, ir_len: usize) -> i32 {
    match job::run(unsafe { bytes(ir_ptr, ir_len) }) {
        Ok(outcome) => publish(outcome),
        Err(e) => fail(e),
    }
}

// ── One workbook, fed a batch at a time ─────────────────────────────────────

/// Opens a workbook. The sheets are declared here; the rows stream.
///
/// # Safety
///
/// `ptr` must point at `len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_book_open(ptr: *const u8, len: usize) -> i32 {
    match Book::open(unsafe { bytes(ptr, len) }) {
        Ok(book) => {
            BOOK.with(|slot| *slot.borrow_mut() = Some(book));
            succeed()
        }
        Err(e) => fail(e),
    }
}

fn with_book(f: impl FnOnce(&mut Book) -> Result<(), JobError>) -> i32 {
    BOOK.with(|slot| match slot.borrow_mut().as_mut() {
        Some(book) => match f(book) {
            Ok(()) => succeed(),
            Err(e) => fail(e),
        },
        None => fail(JobError::OutOfOrder("no workbook is open".into())),
    })
}

/// # Safety
///
/// `ptr` must point at `len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_book_rows(ptr: *const u8, len: usize) -> i32 {
    let json = unsafe { bytes(ptr, len) };
    with_book(|book| book.rows(json))
}

/// # Safety
///
/// `ptr` must point at `len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_book_merge(ptr: *const u8, len: usize) -> i32 {
    let json = unsafe { bytes(ptr, len) };
    with_book(|book| book.merge(json))
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_book_next_sheet() -> i32 {
    with_book(Book::next_sheet)
}

/// How many rows have gone into the open sheet. A merge is expressed in
/// absolute rows, so a caller building a total row needs this.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_book_row() -> u32 {
    BOOK.with(|slot| slot.borrow().as_ref().map_or(0, Book::row))
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_book_finish() -> i32 {
    let book = BOOK.with(|slot| slot.borrow_mut().take());
    match book {
        Some(book) => match book.finish() {
            Ok(outcome) => publish(outcome),
            Err(e) => fail(e),
        },
        None => fail(JobError::OutOfOrder("no workbook is open".into())),
    }
}

// ── Reading the result ──────────────────────────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_ptr() -> *const u8 {
    OUT.with(|slot| slot.borrow().xlsx.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_len() -> usize {
    OUT.with(|slot| slot.borrow().xlsx.len())
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_sheets() -> usize {
    OUT.with(|slot| slot.borrow().sheets)
}

/// Why the last call returned 0, as UTF-8. Empty after a call that succeeded.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_error_ptr() -> *const u8 {
    ERROR.with(|slot| slot.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_error_len() -> usize {
    ERROR.with(|slot| slot.borrow().len())
}

/// Gives back the finished workbook without tearing the instance down.
/// WebAssembly memory is never returned to the host, so an instance that kept
/// its last file would hold the largest one it ever made.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_release() -> i32 {
    OUT.with(|slot| {
        let mut out = slot.borrow_mut();
        out.xlsx = Vec::new();
        out.sheets = 0;
    });
    succeed()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOK_JSON: &[u8] = br#"{
        "sheets": [{
            "name": "Ventas",
            "rows": [{ "cells": [{ "value": { "t": "text", "v": "Servicios" } }, { "value": { "t": "number", "v": 1200 } }] }]
        }]
    }"#;

    /// The ABI as a host drives it. Every test goes through the pointers,
    /// because the pointers are the contract.
    fn put(data: &[u8]) -> (*mut u8, usize) {
        let ptr = imprenta_alloc(data.len());
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
        (ptr, data.len())
    }

    fn give_back((ptr, len): (*mut u8, usize)) {
        unsafe { imprenta_dealloc(ptr, len) };
    }

    fn read_out() -> Vec<u8> {
        unsafe { std::slice::from_raw_parts(imprenta_out_ptr(), imprenta_out_len()) }.to_vec()
    }

    fn read_error() -> String {
        String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(imprenta_error_ptr(), imprenta_error_len())
        })
        .into_owned()
    }

    /// One module-wide state, so the tests must not run at once.
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_workbook_written_into_memory_comes_back_as_a_package() {
        let _lock = guard();
        let ir = put(BOOK_JSON);

        assert_eq!(unsafe { imprenta_write(ir.0, ir.1) }, OK);

        assert_eq!(&read_out()[..2], b"PK");
        assert_eq!(imprenta_out_sheets(), 1);
        give_back(ir);
    }

    #[test]
    fn a_second_write_works_exactly_like_the_first() {
        let _lock = guard();
        let ir = put(BOOK_JSON);

        assert_eq!(unsafe { imprenta_write(ir.0, ir.1) }, OK);
        let first = read_out();
        assert_eq!(unsafe { imprenta_write(ir.0, ir.1) }, OK);

        assert_eq!(read_out(), first);
        give_back(ir);
    }

    #[test]
    fn a_malformed_workbook_leaves_a_message_and_no_corpse() {
        let _lock = guard();
        let bad = put(b"{ not json");

        assert_eq!(unsafe { imprenta_write(bad.0, bad.1) }, FAILED);

        assert!(read_error().contains("not valid JSON"), "{}", read_error());
        give_back(bad);
    }

    #[test]
    fn a_workbook_can_be_fed_through_the_pointers() {
        let _lock = guard();
        let sheets = put(br#"[{ "name": "Ventas" }]"#);

        assert_eq!(unsafe { imprenta_book_open(sheets.0, sheets.1) }, OK);
        let rows = put(br#"[{"cells":[{"value":{"t":"number","v":1200}}]}]"#);
        assert_eq!(unsafe { imprenta_book_rows(rows.0, rows.1) }, OK);
        assert_eq!(imprenta_book_row(), 1);
        assert_eq!(imprenta_book_finish(), OK);

        assert_eq!(&read_out()[..2], b"PK");
        give_back(sheets);
        give_back(rows);
    }

    #[test]
    fn feeding_with_nothing_open_is_an_error_rather_than_a_panic() {
        let _lock = guard();
        imprenta_book_finish(); // clears anything a previous test left open
        let rows = put(b"[]");

        assert_eq!(unsafe { imprenta_book_rows(rows.0, rows.1) }, FAILED);

        assert!(
            read_error().contains("no workbook is open"),
            "{}",
            read_error()
        );
        give_back(rows);
    }

    #[test]
    fn releasing_the_result_gives_the_bytes_back() {
        let _lock = guard();
        let ir = put(BOOK_JSON);
        unsafe { imprenta_write(ir.0, ir.1) };
        assert!(imprenta_out_len() > 0);

        imprenta_out_release();

        assert_eq!(imprenta_out_len(), 0);
        assert_eq!(imprenta_out_sheets(), 0);
        give_back(ir);
    }
}
