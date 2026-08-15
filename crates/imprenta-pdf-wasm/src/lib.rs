//! The Imprenta PDF engine as a plain WebAssembly module.
//!
//! No napi, no emnapi, no WASI, no wasm-bindgen. The whole contract is these
//! exports plus linear memory: the host writes what it wants rendered into
//! memory this module hands it, calls in, and reads the PDF back as a view
//! over the same memory.
//!
//! # Why no glue layer
//!
//! The first attempt went through napi-rs's WebAssembly target, which brings
//! emnapi to implement Node-API inside the module. It rendered one document
//! and then deadlocked on the second, for reasons that had nothing to do with
//! this engine, and it cost 22% on top. A module with **no imports at all**
//! cannot deadlock in a layer it does not have, and it runs anywhere there is
//! a `WebAssembly.instantiate` — Node, the browser, Deno, Bun, an edge
//! runtime — with no per-platform build and no compatibility shim. That
//! property is not self-evidently preserved by a future change, so
//! `packages/wasm/test/module.test.ts` asserts the import list is empty
//! against the built module.
//!
//! # The shape of the ABI
//!
//! Numbers in, numbers out. Anything larger travels through linear memory:
//!
//! - [`imprenta_alloc`]/[`imprenta_dealloc`] give the host somewhere to write.
//! - `assets_*` load the fonts and images, **kept between renders** so a warm
//!   instance rendering chunk after chunk copies a font once, not once a chunk.
//! - [`imprenta_render`] does a whole document; `stream_*` does one fed in pieces.
//! - Every call returns `1` for success and `0` for failure, and a failure
//!   leaves a message at [`imprenta_error_ptr`]. Nothing panics across the boundary:
//!   a malformed document is an error the host can print, not a dead instance.
//! - The result is read block by block with [`imprenta_out_blocks`]/[`imprenta_out_block_ptr`]/
//!   [`imprenta_out_block_len`] and given back with
//!   [`imprenta_out_release`], so a long-lived instance does not sit on the last PDF it
//!   produced. The pointer is only good until the next allocation grows the
//!   memory, which is why the JavaScript binding copies rather than handing a
//!   caller a view that expires.
//!
//! This file is glue and nothing else. What it delegates to lives in [`job`]
//! and [`stream`], where it is tested with `cargo test` and no WebAssembly
//! runtime at all.

use std::cell::RefCell;

pub mod job;
pub mod merge;
pub mod stream;

use job::{FontInput, ImageInput, JobError, Library, Outcome};
use stream::Printer;

thread_local! {
    /// The fonts and images, held across renders. See the note on
    /// [`job::Library`] for why this outlives a single call.
    static LIBRARY: RefCell<Library> = const {
        RefCell::new(Library { fonts: Vec::new(), images: Vec::new() })
    };
    /// The finished document, kept until the host has read it or released it.
    static OUT: RefCell<Outcome> = const {
        RefCell::new(Outcome { pdf: imprenta_pdf::Pdf::empty(), pages: 0, diagnostics: Vec::new() })
    };
    /// The diagnostics, as the JSON array the host reads.
    static DIAGNOSTICS: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// Why the last call returned 0.
    static ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// The document being fed, if one is open. Not `Send`, and it does not
    /// need to be — see [`stream`].
    static PRINTER: RefCell<Option<Printer>> = const { RefCell::new(None) };
    /// Fragments of a sharded render, waiting to be merged. Held here rather
    /// than passed in one call because the host has them one at a time and
    /// nothing is served by making it concatenate them first.
    static FRAGMENTS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    /// Rows this instance measured, kept until it has painted them.
    ///
    /// Measuring is about three fifths of a render. Doing it once to plan and
    /// again to paint costs that share twice, which was exactly the margin
    /// between a sharded render and the native addon it replaced. So the
    /// engine that measured a row is the engine that paints it, and this is
    /// what it kept — released by `imprenta_measured_release`, because it is
    /// the largest thing an instance ever holds.
    static MEASURED: RefCell<Vec<imprenta_pdf::build::MeasuredRow>> =
        const { RefCell::new(Vec::new()) };
}

const OK: i32 = 1;
const FAILED: i32 = 0;

/// Records why a call failed and answers it.
fn fail(e: JobError) -> i32 {
    ERROR.with(|slot| *slot.borrow_mut() = e.to_string().into_bytes());
    FAILED
}

fn succeed() -> i32 {
    ERROR.with(|slot| slot.borrow_mut().clear());
    OK
}

/// Publishes a finished document where the host can read it.
fn publish(outcome: Outcome) -> i32 {
    // Serialised here rather than handed over one string at a time: there are
    // never many, and one read beats a call per diagnostic.
    let json = serde_json::to_vec(&outcome.diagnostics).unwrap_or_else(|_| b"[]".to_vec());
    DIAGNOSTICS.with(|slot| *slot.borrow_mut() = json);
    OUT.with(|slot| *slot.borrow_mut() = outcome);
    succeed()
}

// ── Memory ──────────────────────────────────────────────────────────────────

/// Memory the host may write into. Given back with [`imprenta_dealloc`], never freed
/// here.
///
/// # Safety
///
/// The pointer is valid for `len` bytes until `dealloc` is called with the
/// same pointer and length.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
///
/// `ptr` must have come from [`imprenta_alloc`] with the same `len`, and must not be
/// used afterwards.
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

// ── The library ─────────────────────────────────────────────────────────────

/// Forgets every font and image. A host that reuses an instance for a
/// different document calls this first.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_assets_reset() -> i32 {
    LIBRARY.with(|slot| slot.borrow_mut().clear());
    succeed()
}

/// Adds a typeface. `weight` is `"regular"` or `"bold"`; `italic` is 0 or 1.
///
/// # Safety
///
/// Both pointers must point at their stated number of readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_assets_font(
    weight_ptr: *const u8,
    weight_len: usize,
    italic: i32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let weight = match std::str::from_utf8(unsafe { bytes(weight_ptr, weight_len) }) {
        Ok(s) => s.to_owned(),
        Err(e) => {
            return fail(JobError::Malformed(format!(
                "the font weight is not text: {e}"
            )));
        }
    };
    let font = FontInput {
        weight,
        italic: italic != 0,
        data: unsafe { bytes(data_ptr, data_len) }.to_vec(),
    };
    // Checked now rather than at render time, so a typo in a weight is found
    // at boot instead of forty seconds into a ledger.
    if let Err(e) = job::face(&font) {
        return fail(e);
    }
    LIBRARY.with(|slot| slot.borrow_mut().fonts.push(font));
    succeed()
}

/// Adds an image the document refers to by name.
///
/// # Safety
///
/// Both pointers must point at their stated number of readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_assets_image(
    name_ptr: *const u8,
    name_len: usize,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let name = match std::str::from_utf8(unsafe { bytes(name_ptr, name_len) }) {
        Ok(s) => s.to_owned(),
        Err(e) => {
            return fail(JobError::Malformed(format!(
                "the image name is not text: {e}"
            )));
        }
    };
    let image = ImageInput {
        name,
        data: unsafe { bytes(data_ptr, data_len) }.to_vec(),
    };
    LIBRARY.with(|slot| slot.borrow_mut().images.push(image));
    succeed()
}

// ── One document, declared whole ────────────────────────────────────────────

/// Renders a declared document. Read the result with [`imprenta_out_block_ptr`].
///
/// # Safety
///
/// `ir_ptr` must point at `ir_len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_render(ir_ptr: *const u8, ir_len: usize) -> i32 {
    let ir = unsafe { bytes(ir_ptr, ir_len) };
    let outcome = LIBRARY.with(|library| job::run(ir, &library.borrow()));
    match outcome {
        Ok(outcome) => publish(outcome),
        Err(e) => fail(e),
    }
}

// ── One document, shared out ────────────────────────────────────────────────
//
// Four calls, in this order, and the order is the whole design:
//
//   1. `imprenta_measure_rows`, on every instance at once, over its own range
//      of the table. Hands back one height per row — four bytes, where the
//      content it came from is every glyph on the line.
//   2. `imprenta_plan`, on one instance, over all the heights. Says where each
//      page begins and what the running totals stood at. Cheap: the packer
//      sees heights and break flags, never text.
//   3. `imprenta_stream_open` with a `resume`, on every instance at once, each
//      painting the pages the plan gave it.
//   4. `imprenta_merge_*`, on one instance, over the fragments.
//
// Splitting anywhere but a page boundary repaginates, which is why step two
// exists and why it packs rather than estimating.

/// Measures a run of table rows, keeps them, and hands back one height each
/// as little-endian `f32` readable with [`imprenta_out_block_ptr`].
///
/// Only the heights cross: four bytes a row, against the kilobytes the row
/// itself weighs. The rows stay here for [`imprenta_fragment_measured`], which
/// is what stops a sharded render measuring everything twice.
///
/// # Safety
///
/// Every pointer must point at its stated number of readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_measure_rows(
    setup_ptr: *const u8,
    setup_len: usize,
    head_ptr: *const u8,
    head_len: usize,
    rows_ptr: *const u8,
    rows_len: usize,
) -> i32 {
    let setup: stream::Setup = match serde_json::from_slice(unsafe { bytes(setup_ptr, setup_len) })
    {
        Ok(setup) => setup,
        Err(e) => return fail(JobError::Malformed(e.to_string())),
    };
    let head: imprenta_pdf::ir::TableHead =
        match serde_json::from_slice(unsafe { bytes(head_ptr, head_len) }) {
            Ok(head) => head,
            Err(e) => return fail(JobError::Malformed(e.to_string())),
        };
    let rows: Vec<imprenta_pdf::ir::Row> =
        match serde_json::from_slice(unsafe { bytes(rows_ptr, rows_len) }) {
            Ok(rows) => rows,
            Err(e) => return fail(JobError::Malformed(e.to_string())),
        };

    let measured = LIBRARY.with(|library| {
        library.borrow().assets().and_then(|assets| {
            imprenta_pdf::build::measure_rows(&assets, &setup.page, &head, &rows)
                .map_err(|e| JobError::Build(e.to_string()))
        })
    });

    match measured {
        Ok(rows) => {
            let mut out = Vec::with_capacity(rows.len() * 4);
            for row in &rows {
                out.extend_from_slice(&row.atom().height.get().to_le_bytes());
            }
            let count = rows.len();
            MEASURED.with(|slot| *slot.borrow_mut() = rows);
            OUT.with(|slot| {
                let mut held = slot.borrow_mut();
                // Already contiguous, so it crosses as one block.
                held.pdf = out.into();
                held.pages = count;
                held.diagnostics = Vec::new();
            });
            succeed()
        }
        Err(e) => fail(e),
    }
}

/// Paints a run of the rows this instance measured, as a fragment.
///
/// `from` and `to` index what [`imprenta_measure_rows`] kept. `extra` is the
/// tail of rows the last page of this fragment needs and this instance never
/// measured — a page's worth at most, because a fragment is cut on a page
/// boundary and only the one page straddles the seam.
///
/// # Safety
///
/// Every pointer must point at its stated number of readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_fragment_measured(
    setup_ptr: *const u8,
    setup_len: usize,
    head_ptr: *const u8,
    head_len: usize,
    from: usize,
    to: usize,
    extra_ptr: *const u8,
    extra_len: usize,
) -> i32 {
    let printer = LIBRARY
        .with(|library| Printer::open(unsafe { bytes(setup_ptr, setup_len) }, &library.borrow()));
    let mut printer = match printer {
        Ok(printer) => printer,
        Err(e) => return fail(e),
    };

    let held = MEASURED.with(|slot| {
        let rows = slot.borrow();
        let end = to.min(rows.len());
        if from >= end {
            Vec::new()
        } else {
            rows[from..end].to_vec()
        }
    });
    if let Err(e) = printer.rows_measured(&held) {
        return fail(e);
    }

    let extra = unsafe { bytes(extra_ptr, extra_len) };
    if !extra.is_empty() {
        // Measured here rather than kept: it is one page of rows, and the
        // instance that did measure them is busy painting its own.
        let head: imprenta_pdf::ir::TableHead =
            match serde_json::from_slice(unsafe { bytes(head_ptr, head_len) }) {
                Ok(head) => head,
                Err(e) => return fail(JobError::Malformed(e.to_string())),
            };
        if let Err(e) = printer.rows_in(&head, extra) {
            return fail(e);
        }
    }

    match printer.finish() {
        Ok(outcome) => publish(outcome),
        Err(e) => fail(e),
    }
}

/// Gives back the rows this instance measured.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_measured_release() -> i32 {
    MEASURED.with(|slot| *slot.borrow_mut() = Vec::new());
    succeed()
}

/// Packs measured heights and hands back the plan as JSON.
///
/// # Safety
///
/// Both pointers must point at their stated number of readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_plan(
    setup_ptr: *const u8,
    setup_len: usize,
    heights_ptr: *const u8,
    heights_len: usize,
) -> i32 {
    let setup: stream::Setup = match serde_json::from_slice(unsafe { bytes(setup_ptr, setup_len) })
    {
        Ok(setup) => setup,
        Err(e) => return fail(JobError::Malformed(e.to_string())),
    };
    let raw = unsafe { bytes(heights_ptr, heights_len) };
    if raw.len() % 4 != 0 {
        return fail(JobError::Malformed(
            "the measured heights are not whole floats".into(),
        ));
    }
    let atoms: Vec<imprenta_pdf::atom::Atom> = raw
        .chunks_exact(4)
        .map(|four| {
            imprenta_pdf::atom::Atom::new(imprenta_core::units::Pt(f32::from_le_bytes([
                four[0], four[1], four[2], four[3],
            ])))
        })
        .collect();

    let bands = imprenta_pdf::session::Bands {
        header: setup.header.clone(),
        footer: setup.footer.clone(),
    };
    let planned = LIBRARY.with(|library| {
        library.borrow().assets().and_then(|assets| {
            imprenta_pdf::build::plan(
                &setup.page,
                &assets,
                &bands,
                setup.accumulators.len(),
                &atoms,
            )
            .map_err(|e| JobError::Build(e.to_string()))
        })
    });

    match planned {
        Ok(pages) => {
            let json: Vec<serde_json::Value> = pages
                .iter()
                .map(|page| {
                    serde_json::json!({
                        "firstAtom": page.first_atom,
                        "lastAtom": page.last_atom,
                        "opening": page.opening,
                    })
                })
                .collect();
            let bytes = serde_json::to_vec(&json).unwrap_or_else(|_| b"[]".to_vec());
            OUT.with(|slot| {
                let mut held = slot.borrow_mut();
                held.pages = json.len();
                // Already contiguous, so it crosses as one block.
                held.pdf = bytes.into();
                held.diagnostics = Vec::new();
            });
            succeed()
        }
        Err(e) => fail(e),
    }
}

/// Forgets any fragments held for merging.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_merge_reset() -> i32 {
    FRAGMENTS.with(|slot| slot.borrow_mut().clear());
    succeed()
}

/// Adds one fragment. Order is document order and nothing sorts it later.
///
/// # Safety
///
/// `ptr` must point at `len` readable bytes of PDF.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_merge_push(ptr: *const u8, len: usize) -> i32 {
    let bytes = unsafe { bytes(ptr, len) }.to_vec();
    FRAGMENTS.with(|slot| slot.borrow_mut().push(bytes));
    succeed()
}

/// Merges what was pushed and publishes the file at [`imprenta_out_block_ptr`].
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_merge_finish() -> i32 {
    let fragments = FRAGMENTS.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    match merge::merge(&fragments) {
        Ok(pdf) => {
            // The page count comes from the file rather than from the sum the
            // host kept, so a fragment that lost a page on the way in is
            // caught here rather than by whoever opens the document.
            let pages = merge::pages_in(&pdf).unwrap_or(0);
            publish(Outcome {
                pdf: pdf.into(),
                pages,
                diagnostics: Vec::new(),
            })
        }
        Err(e) => fail(JobError::Build(e.to_string())),
    }
}

// ── One document, fed in pieces ─────────────────────────────────────────────

/// Opens a document that will arrive in pieces.
///
/// # Safety
///
/// `ptr` must point at `len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_stream_open(ptr: *const u8, len: usize) -> i32 {
    let setup = unsafe { bytes(ptr, len) };
    match LIBRARY.with(|library| Printer::open(setup, &library.borrow())) {
        Ok(printer) => {
            PRINTER.with(|slot| *slot.borrow_mut() = Some(printer));
            succeed()
        }
        Err(e) => fail(e),
    }
}

/// Runs `f` against the open document, or fails saying there is none.
fn with_printer(f: impl FnOnce(&mut Printer) -> Result<(), JobError>) -> i32 {
    PRINTER.with(|slot| match slot.borrow_mut().as_mut() {
        Some(printer) => match f(printer) {
            Ok(()) => succeed(),
            Err(e) => fail(e),
        },
        None => fail(JobError::Build("no document is open".into())),
    })
}

/// # Safety
///
/// `ptr` must point at `len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_stream_nodes(ptr: *const u8, len: usize) -> i32 {
    let json = unsafe { bytes(ptr, len) };
    with_printer(|printer| printer.nodes(json))
}

/// # Safety
///
/// `ptr` must point at `len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_stream_open_table(ptr: *const u8, len: usize) -> i32 {
    let json = unsafe { bytes(ptr, len) };
    with_printer(|printer| printer.open_table(json))
}

/// # Safety
///
/// `ptr` must point at `len` readable bytes of JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_stream_rows(ptr: *const u8, len: usize) -> i32 {
    let json = unsafe { bytes(ptr, len) };
    with_printer(|printer| printer.rows(json))
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_stream_close_table() -> i32 {
    with_printer(|printer| printer.close_table())
}

/// Atoms the engine is still holding. Read without a round trip through
/// anything, because this is the number the whole design exists to keep flat
/// and asking for it should be free.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_stream_pending() -> usize {
    PRINTER.with(|slot| slot.borrow().as_ref().map_or(0, Printer::pending))
}

/// Paints what is left and closes the document. Read it with [`imprenta_out_block_ptr`].
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_stream_finish() -> i32 {
    let printer = PRINTER.with(|slot| slot.borrow_mut().take());
    match printer {
        Some(printer) => match printer.finish() {
            Ok(outcome) => publish(outcome),
            Err(e) => fail(e),
        },
        None => fail(JobError::Build("no document is open".into())),
    }
}

// ── Reading the result ──────────────────────────────────────────────────────

/// How many pieces the result is in. Read them with
/// [`imprenta_out_block_ptr`] and [`imprenta_out_block_len`]; concatenated in
/// order they are the file.
///
/// Blocks rather than one buffer, deliberately: handing back a contiguous
/// result meant joining the writer's blocks inside linear memory, which held
/// two copies of the finished document at the peak — and linear memory never
/// shrinks, so the peak was the footprint (issue #7). The host assembles the
/// copy on its own heap, where the memory goes back.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_blocks() -> usize {
    OUT.with(|slot| slot.borrow().pdf.block_count())
}

/// Where the `index`-th piece starts. Null past the end, never a trap.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_block_ptr(index: usize) -> *const u8 {
    OUT.with(|slot| {
        slot.borrow()
            .pdf
            .block(index)
            .map_or(std::ptr::null(), <[u8]>::as_ptr)
    })
}

/// How long the `index`-th piece is. Zero past the end.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_block_len(index: usize) -> usize {
    OUT.with(|slot| slot.borrow().pdf.block(index).map_or(0, <[u8]>::len))
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_pages() -> usize {
    OUT.with(|slot| slot.borrow().pages)
}

/// Anything the engine noticed, as a JSON array of strings.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_diagnostics_ptr() -> *const u8 {
    DIAGNOSTICS.with(|slot| slot.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_diagnostics_len() -> usize {
    DIAGNOSTICS.with(|slot| slot.borrow().len())
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

/// Gives back the finished document without tearing the instance down.
///
/// A pooled instance renders one chunk after another, and an instance that
/// held its last PDF would hold the largest one it ever produced for as long
/// as it lived — WebAssembly memory is never returned to the host, so that is
/// not a matter of a garbage collector eventually noticing.
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_release() -> i32 {
    OUT.with(|slot| {
        let mut out = slot.borrow_mut();
        out.pdf = imprenta_pdf::Pdf::empty();
        out.pages = 0;
        out.diagnostics = Vec::new();
    });
    DIAGNOSTICS.with(|slot| *slot.borrow_mut() = Vec::new());
    succeed()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBOTO: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Regular.ttf");

    const HELLO: &[u8] = br#"{
        "page": { "width": 595, "height": 842 },
        "children": [{ "t": "text", "runs": [{ "text": "Hola" }] }]
    }"#;

    /// The ABI as a host drives it: write into memory we asked for, call,
    /// read back. Every test here goes through the pointers rather than
    /// calling [`job`] directly, because the pointers are the contract.
    fn put(data: &[u8]) -> (*mut u8, usize) {
        let ptr = imprenta_alloc(data.len());
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
        (ptr, data.len())
    }

    fn give_back((ptr, len): (*mut u8, usize)) {
        unsafe { imprenta_dealloc(ptr, len) };
    }

    fn load_roman() {
        imprenta_assets_reset();
        let weight = put(b"regular");
        let data = put(ROBOTO);
        assert_eq!(
            unsafe { imprenta_assets_font(weight.0, weight.1, 0, data.0, data.1) },
            OK
        );
        give_back(weight);
        give_back(data);
    }

    /// The result as the host assembles it: the blocks, in order.
    fn read_out() -> Vec<u8> {
        let mut out = Vec::with_capacity(out_len());
        for index in 0..imprenta_out_blocks() {
            out.extend_from_slice(unsafe {
                std::slice::from_raw_parts(
                    imprenta_out_block_ptr(index),
                    imprenta_out_block_len(index),
                )
            });
        }
        out
    }

    /// The total the blocks add up to, which is what `out_len` used to say.
    fn out_len() -> usize {
        (0..imprenta_out_blocks())
            .map(|index| imprenta_out_block_len(index))
            .sum()
    }

    fn read_error() -> String {
        String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(imprenta_error_ptr(), imprenta_error_len())
        })
        .into_owned()
    }

    /// The tests share one module-wide state, so they must not run at once.
    /// A mutex rather than `--test-threads=1`, which nobody remembers to pass.
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_document_written_into_memory_comes_back_as_a_pdf() {
        let _lock = guard();
        load_roman();
        let ir = put(HELLO);

        assert_eq!(unsafe { imprenta_render(ir.0, ir.1) }, OK);

        assert_eq!(&read_out()[..5], b"%PDF-");
        assert_eq!(imprenta_out_pages(), 1);
        give_back(ir);
    }

    #[test]
    fn a_second_render_works_exactly_like_the_first() {
        // This is the one that the napi/emnapi build failed. It renders one
        // document per instance and then wedges, so the line is worth holding
        // here even though nothing in this crate could plausibly break it.
        let _lock = guard();
        load_roman();
        let ir = put(HELLO);

        assert_eq!(unsafe { imprenta_render(ir.0, ir.1) }, OK);
        let first = read_out();
        assert_eq!(unsafe { imprenta_render(ir.0, ir.1) }, OK);
        let second = read_out();

        assert_eq!(first, second);
        give_back(ir);
    }

    #[test]
    fn the_fonts_survive_between_renders() {
        // The reason the library outlives a call: a pooled instance must not
        // be handed its fonts again for every chunk.
        let _lock = guard();
        load_roman();
        let ir = put(HELLO);

        assert_eq!(unsafe { imprenta_render(ir.0, ir.1) }, OK);
        assert_eq!(
            unsafe { imprenta_render(ir.0, ir.1) },
            OK,
            "{}",
            read_error()
        );

        give_back(ir);
    }

    #[test]
    fn a_malformed_document_leaves_a_message_and_no_corpse() {
        let _lock = guard();
        load_roman();
        let bad = put(b"{ not json");

        assert_eq!(unsafe { imprenta_render(bad.0, bad.1) }, FAILED);

        assert!(read_error().contains("not valid JSON"), "{}", read_error());
        give_back(bad);
    }

    #[test]
    fn rendering_with_no_fonts_says_which_thing_was_missing() {
        let _lock = guard();
        imprenta_assets_reset();
        let ir = put(HELLO);

        assert_eq!(unsafe { imprenta_render(ir.0, ir.1) }, FAILED);

        assert!(read_error().contains("no fonts"), "{}", read_error());
        give_back(ir);
    }

    #[test]
    fn an_unknown_weight_is_refused_when_it_is_given_not_when_it_is_used() {
        let _lock = guard();
        imprenta_assets_reset();
        let weight = put(b"semibold");
        let data = put(ROBOTO);

        assert_eq!(
            unsafe { imprenta_assets_font(weight.0, weight.1, 0, data.0, data.1) },
            FAILED
        );

        assert!(read_error().contains("semibold"), "{}", read_error());
        give_back(weight);
        give_back(data);
    }

    #[test]
    fn a_successful_call_clears_the_last_error() {
        let _lock = guard();
        load_roman();
        let bad = put(b"{ not json");
        let good = put(HELLO);

        unsafe { imprenta_render(bad.0, bad.1) };
        assert!(!read_error().is_empty());
        unsafe { imprenta_render(good.0, good.1) };

        assert_eq!(read_error(), "");
        give_back(bad);
        give_back(good);
    }

    #[test]
    fn what_the_engine_noticed_arrives_as_json() {
        let _lock = guard();
        load_roman();
        const JAPANESE: &str = r#"{
            "page": { "width": 595, "height": 842 },
            "children": [{ "t": "text", "runs": [{ "text": "日本語" }] }]
        }"#;
        let ir = put(JAPANESE.as_bytes());

        assert_eq!(unsafe { imprenta_render(ir.0, ir.1) }, OK);

        let json = unsafe {
            std::slice::from_raw_parts(imprenta_diagnostics_ptr(), imprenta_diagnostics_len())
        }
        .to_vec();
        let reported: Vec<String> = serde_json::from_slice(&json).unwrap();
        assert!(!reported.is_empty(), "{reported:?}");
        give_back(ir);
    }

    #[test]
    fn a_block_index_past_the_end_is_null_and_zero_rather_than_a_trap() {
        // The index arrives over the boundary, where a panic kills the
        // instance. A host that miscounts must get an answer it can check.
        let _lock = guard();
        load_roman();
        let ir = put(HELLO);
        unsafe { imprenta_render(ir.0, ir.1) };
        let count = imprenta_out_blocks();

        assert!(count >= 1);
        assert!(imprenta_out_block_ptr(count).is_null());
        assert_eq!(imprenta_out_block_len(count), 0);

        imprenta_out_release();
        give_back(ir);
    }

    #[test]
    fn releasing_the_result_gives_the_bytes_back() {
        let _lock = guard();
        load_roman();
        let ir = put(HELLO);
        unsafe { imprenta_render(ir.0, ir.1) };
        assert!(out_len() > 0);

        imprenta_out_release();

        assert_eq!(out_len(), 0);
        assert_eq!(imprenta_out_blocks(), 0);
        assert_eq!(imprenta_out_pages(), 0);
        assert_eq!(imprenta_diagnostics_len(), 0);
        give_back(ir);
    }

    /// A table head and `count` rows, as the sharded path sends them.
    fn ledger_rows(from: usize, to: usize) -> Vec<u8> {
        let rows: Vec<String> = (from..to)
            .map(|i| {
                format!(
                    r#"{{"cells":[{{"text":"Prestacion de servicios, asiento {i}"}},{{"text":"1.200,00"}}]}}"#
                )
            })
            .collect();
        format!("[{}]", rows.join(",")).into_bytes()
    }

    const HEAD: &[u8] = br#"{"columns":[{"width":{"unit":"percent","value":0.6}},{"width":{"unit":"percent","value":0.4}}]}"#;
    const SETUP: &[u8] = br#"{"page":{"width":595,"height":842}}"#;

    /// Reads the heights `imprenta_measure_rows` left behind.
    fn read_heights() -> Vec<f32> {
        let bytes = read_out();
        bytes
            .chunks_exact(4)
            .map(|four| f32::from_le_bytes([four[0], four[1], four[2], four[3]]))
            .collect()
    }

    #[test]
    fn measuring_rows_hands_back_one_height_each() {
        // The first half of a sharded render: several instances do this at
        // once on different ranges, and what crosses back is four bytes a row
        // rather than every glyph on it.
        let _lock = guard();
        load_roman();
        let setup = put(SETUP);
        let head = put(HEAD);
        let rows = put(&ledger_rows(0, 300));

        assert_eq!(
            unsafe { imprenta_measure_rows(setup.0, setup.1, head.0, head.1, rows.0, rows.1) },
            OK,
            "{}",
            read_error()
        );

        let heights = read_heights();
        assert_eq!(heights.len(), 300);
        assert!(heights.iter().all(|h| *h > 0.0), "every row has a height");
        give_back(setup);
        give_back(head);
        give_back(rows);
    }

    #[test]
    fn planning_from_those_heights_finds_the_pages() {
        let _lock = guard();
        load_roman();
        let first = put(SETUP);
        let head = put(HEAD);
        let rows = put(&ledger_rows(0, 600));
        unsafe { imprenta_measure_rows(first.0, first.1, head.0, head.1, rows.0, rows.1) };
        let heights = read_out();
        give_back(first);

        let setup = put(SETUP);
        let measured = put(&heights);
        assert_eq!(
            unsafe { imprenta_plan(setup.0, setup.1, measured.0, measured.1) },
            OK,
            "{}",
            read_error()
        );

        let plan: serde_json::Value = serde_json::from_slice(&read_out()).unwrap();
        let pages = plan.as_array().unwrap();
        assert!(pages.len() > 5, "six hundred rows must paginate");
        assert_eq!(pages[0]["firstAtom"], 0, "the first page starts at the top");
        give_back(head);
        give_back(rows);
        give_back(setup);
        give_back(measured);
    }

    #[test]
    fn a_fragment_can_be_told_which_page_it_starts_on() {
        // Phase three. Without this every fragment would number itself from
        // one, and page numbers stamped on afterwards are what this engine
        // exists to replace.
        let _lock = guard();
        load_roman();
        let setup = put(br#"{"page":{"width":595,"height":842},"resume":{"page":7,"total":40}}"#);

        assert_eq!(
            unsafe { imprenta_stream_open(setup.0, setup.1) },
            OK,
            "{}",
            read_error()
        );
        let head = put(HEAD);
        unsafe { imprenta_stream_open_table(head.0, head.1) };
        let rows = put(&ledger_rows(0, 60));
        unsafe { imprenta_stream_rows(rows.0, rows.1) };
        assert_eq!(imprenta_stream_close_table(), OK);
        assert_eq!(imprenta_stream_finish(), OK);

        assert_eq!(&read_out()[..5], b"%PDF-");
        give_back(setup);
        give_back(head);
        give_back(rows);
    }

    #[test]
    fn painting_from_what_was_measured_gives_the_same_fragment() {
        // The optimisation the sharded path exists on. Measuring is three
        // fifths of a render; doing it to plan and again to paint costs that
        // twice, which was the whole margin over the addon. Reusing it must
        // change nothing about the bytes.
        let _lock = guard();
        load_roman();
        let setup = put(SETUP);
        let head = put(HEAD);
        let rows = put(&ledger_rows(0, 200));

        // Measured once, painted from what was kept.
        unsafe { imprenta_measure_rows(setup.0, setup.1, head.0, head.1, rows.0, rows.1) };
        assert_eq!(
            unsafe {
                imprenta_fragment_measured(
                    setup.0,
                    setup.1,
                    head.0,
                    head.1,
                    0,
                    200,
                    std::ptr::null(),
                    0,
                )
            },
            OK,
            "{}",
            read_error()
        );
        let reused = read_out();

        // Measured again, the ordinary way.
        unsafe { imprenta_stream_open(setup.0, setup.1) };
        unsafe { imprenta_stream_open_table(head.0, head.1) };
        unsafe { imprenta_stream_rows(rows.0, rows.1) };
        imprenta_stream_close_table();
        imprenta_stream_finish();
        let measured_again = read_out();

        assert_eq!(reused, measured_again);
        imprenta_measured_release();
        give_back(setup);
        give_back(head);
        give_back(rows);
    }

    #[test]
    fn a_fragment_takes_the_rows_it_never_measured_from_the_host() {
        // The seam. A fragment is cut on a page boundary, and the page at that
        // boundary needs rows the next instance measured. One page's worth,
        // handed over as JSON and measured here.
        let _lock = guard();
        load_roman();
        let setup = put(SETUP);
        let head = put(HEAD);
        let mine = put(&ledger_rows(0, 150));
        unsafe { imprenta_measure_rows(setup.0, setup.1, head.0, head.1, mine.0, mine.1) };
        let extra = put(&ledger_rows(150, 200));

        assert_eq!(
            unsafe {
                imprenta_fragment_measured(
                    setup.0, setup.1, head.0, head.1, 0, 150, extra.0, extra.1,
                )
            },
            OK,
            "{}",
            read_error()
        );
        let across_the_seam = read_out();

        // The same two hundred rows, measured in one go.
        let all = put(&ledger_rows(0, 200));
        unsafe { imprenta_stream_open(setup.0, setup.1) };
        unsafe { imprenta_stream_open_table(head.0, head.1) };
        unsafe { imprenta_stream_rows(all.0, all.1) };
        imprenta_stream_close_table();
        imprenta_stream_finish();

        assert_eq!(across_the_seam, read_out());
        imprenta_measured_release();
        give_back(setup);
        give_back(head);
        give_back(mine);
        give_back(extra);
        give_back(all);
    }

    #[test]
    fn fragments_merge_into_one_document() {
        // Phase four, through the pointers: the host pushes each fragment and
        // asks for the file.
        let _lock = guard();
        load_roman();
        let head = put(HEAD);

        let mut fragments = Vec::new();
        for range in [(0usize, 150usize), (150, 300)] {
            let setup = put(SETUP);
            unsafe { imprenta_stream_open(setup.0, setup.1) };
            unsafe { imprenta_stream_open_table(head.0, head.1) };
            let rows = put(&ledger_rows(range.0, range.1));
            unsafe { imprenta_stream_rows(rows.0, rows.1) };
            imprenta_stream_close_table();
            imprenta_stream_finish();
            fragments.push((read_out(), imprenta_out_pages()));
            give_back(setup);
            give_back(rows);
        }
        let expected: usize = fragments.iter().map(|(_, pages)| pages).sum();

        assert_eq!(imprenta_merge_reset(), OK);
        for (bytes, _) in &fragments {
            let held = put(bytes);
            assert_eq!(unsafe { imprenta_merge_push(held.0, held.1) }, OK);
            give_back(held);
        }
        assert_eq!(imprenta_merge_finish(), OK, "{}", read_error());

        assert_eq!(&read_out()[..5], b"%PDF-");
        assert_eq!(imprenta_out_pages(), expected, "every page survived");
        give_back(head);
    }

    #[test]
    fn merging_nothing_is_an_error_the_host_can_read() {
        let _lock = guard();
        imprenta_merge_reset();

        assert_eq!(imprenta_merge_finish(), FAILED);

        assert!(!read_error().is_empty());
    }

    #[test]
    fn a_document_can_be_fed_in_pieces_through_the_pointers() {
        let _lock = guard();
        load_roman();
        let setup = put(br#"{ "page": { "width": 595, "height": 842 } }"#);

        assert_eq!(unsafe { imprenta_stream_open(setup.0, setup.1) }, OK);
        let nodes = put(br#"[{"t":"text","runs":[{"text":"Hola"}]}]"#);
        assert_eq!(unsafe { imprenta_stream_nodes(nodes.0, nodes.1) }, OK);
        assert_eq!(imprenta_stream_finish(), OK);

        assert_eq!(&read_out()[..5], b"%PDF-");
        assert_eq!(imprenta_out_pages(), 1);
        give_back(setup);
        give_back(nodes);
    }

    #[test]
    fn feeding_with_nothing_open_is_an_error_rather_than_a_panic() {
        let _lock = guard();
        load_roman();
        assert_eq!(imprenta_stream_finish(), FAILED); // clears any open document
        let rows = put(b"[]");

        assert_eq!(unsafe { imprenta_stream_rows(rows.0, rows.1) }, FAILED);

        assert!(
            read_error().contains("no document is open"),
            "{}",
            read_error()
        );
        give_back(rows);
    }

    #[test]
    fn zero_length_input_is_read_as_empty_and_not_dereferenced() {
        let _lock = guard();
        load_roman();

        // A host with nothing to send passes a null pointer; the alternative
        // is every caller having to keep a dummy allocation alive.
        assert_eq!(unsafe { imprenta_render(std::ptr::null(), 0) }, FAILED);
        assert!(!read_error().is_empty());
    }
}
