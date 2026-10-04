//! Plain WebAssembly ABI for the Rust ESC/POS engine.

use imprenta_escpos::{Image, Outcome};
use std::cell::RefCell;

thread_local! {
    static IMAGES: RefCell<Vec<Image>> = const { RefCell::new(Vec::new()) };
    static OUT: RefCell<Outcome> = RefCell::new(Outcome::default());
    static META: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn fail(error: impl std::fmt::Display) -> i32 {
    imprenta_out_release();
    ERROR.with(|slot| *slot.borrow_mut() = error.to_string().into_bytes());
    0
}
fn succeed() -> i32 {
    ERROR.with(|slot| slot.borrow_mut().clear());
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr` must have been returned by imprenta_alloc with this same length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_dealloc(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
    }
}

/// # Safety
/// Pointer must address len readable bytes; a null pointer denotes an empty slice.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_assets_reset() -> i32 {
    IMAGES.with(|slot| slot.borrow_mut().clear());
    succeed()
}

/// # Safety
/// Pointers must address the declared readable byte counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_assets_image(
    name_ptr: *const u8,
    name_len: usize,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    let name = match std::str::from_utf8(unsafe { bytes(name_ptr, name_len) }) {
        Ok(name) => name.to_owned(),
        Err(error) => return fail(error),
    };
    IMAGES.with(|slot| {
        slot.borrow_mut().push(Image {
            name,
            data: unsafe { bytes(data_ptr, data_len) }.to_vec(),
        })
    });
    succeed()
}

/// # Safety
/// Input must address len readable JSON bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imprenta_render(ptr: *const u8, len: usize) -> i32 {
    let result = IMAGES.with(|slot| {
        imprenta_escpos::render_with_images(unsafe { bytes(ptr, len) }, &slot.borrow())
    });
    match result {
        Ok(out) => {
            let metadata = serde_json::json!({ "profile": out.profile, "tickets": out.tickets, "diagnostics": out.diagnostics });
            let metadata = match serde_json::to_vec(&metadata) {
                Ok(meta) => meta,
                Err(error) => return fail(error),
            };
            META.with(|slot| *slot.borrow_mut() = metadata);
            OUT.with(|slot| *slot.borrow_mut() = out);
            succeed()
        }
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_ptr() -> *const u8 {
    OUT.with(|slot| slot.borrow().escpos.as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_len() -> usize {
    OUT.with(|slot| slot.borrow().escpos.len())
}
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_meta_ptr() -> *const u8 {
    META.with(|slot| slot.borrow().as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_meta_len() -> usize {
    META.with(|slot| slot.borrow().len())
}
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_error_ptr() -> *const u8 {
    ERROR.with(|slot| slot.borrow().as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_error_len() -> usize {
    ERROR.with(|slot| slot.borrow().len())
}
#[unsafe(no_mangle)]
pub extern "C" fn imprenta_out_release() -> i32 {
    OUT.with(|slot| *slot.borrow_mut() = Outcome::default());
    META.with(|slot| *slot.borrow_mut() = Vec::new());
    succeed()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put(data: &[u8]) -> (*mut u8, usize) {
        let ptr = imprenta_alloc(data.len());
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
        (ptr, data.len())
    }
    #[test]
    fn abi_publishes_copies_and_releases_results() {
        let (ptr, len) = put(br#"{"children":[{"type":"text","text":"Hello"}]}"#);
        assert_eq!(unsafe { imprenta_render(ptr, len) }, 1);
        assert!(imprenta_out_len() > 0);
        let first = unsafe { bytes(imprenta_out_ptr(), imprenta_out_len()) }.to_vec();
        assert_eq!(unsafe { imprenta_render(ptr, len) }, 1);
        assert_eq!(
            unsafe { bytes(imprenta_out_ptr(), imprenta_out_len()) },
            first
        );
        assert!(imprenta_meta_len() > 0);
        imprenta_out_release();
        assert_eq!(imprenta_out_len(), 0);
        assert_eq!(imprenta_meta_len(), 0);
        unsafe { imprenta_dealloc(ptr, len) };
    }
    #[test]
    fn bad_input_clears_output_and_next_job_recovers() {
        let (ptr, len) = put(b"{invalid");
        assert_eq!(unsafe { imprenta_render(ptr, len) }, 0);
        assert!(imprenta_error_len() > 0);
        assert_eq!(imprenta_out_len(), 0);
        unsafe { imprenta_dealloc(ptr, len) };
        let (ptr, len) = put(br#"{"children":[]}"#);
        assert_eq!(unsafe { imprenta_render(ptr, len) }, 1);
        assert_eq!(imprenta_error_len(), 0);
        unsafe { imprenta_dealloc(ptr, len) };
    }
}
