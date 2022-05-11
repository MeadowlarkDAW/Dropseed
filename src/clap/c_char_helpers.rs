use std::borrow::Cow;
use std::ffi::{CStr, FromBytesWithNulError};
use std::mem::MaybeUninit;
use std::os::raw::c_char;

/// Convert a Rust `str` to a constant-sized `c_char` buffer.
///
/// Returns `None` when the given string does not fit in the buffer.
pub(crate) fn str_to_c_char_buf<const BUF_SIZE: usize>(s: &str) -> Option<[c_char; BUF_SIZE]> {
    let s_bytes = s.as_bytes();

    if s_bytes.len() < BUF_SIZE {
        // Safe because we are null-terminating the string later.
        let mut c_buf: [c_char; BUF_SIZE] =
            unsafe { MaybeUninit::<[c_char; BUF_SIZE]>::uninit().assume_init() };

        // Safe because c_char and u8 have the same size.
        let c_buf_u8 = unsafe { &mut *(c_buf.as_mut_ptr() as *mut [u8; BUF_SIZE]) };

        c_buf_u8[0..s_bytes.len()].copy_from_slice(s_bytes);

        // add null terminator
        c_buf_u8[s_bytes.len()] = 0;

        Some(c_buf)
    } else {
        None
    }
}

pub(crate) fn c_char_buf_to_str<'a, const BUF_SIZE: usize>(
    buf: &'a [c_char; BUF_SIZE],
) -> Result<Cow<'a, str>, FromBytesWithNulError> {
    // Safe because c_char and u8 have the same size.
    let c_buf_u8 = unsafe { &*((*buf).as_ptr() as *const [u8; BUF_SIZE]) };

    CStr::from_bytes_with_nul(c_buf_u8).map(|s| s.to_string_lossy())
}

pub(crate) fn c_char_ptr_to_maybe_str<'a>(
    c_str: *const c_char,
    max_size: usize,
) -> Option<Result<Cow<'a, str>, ()>> {
    // Oh boy, C-style null-terminated strings

    if c_str.is_null() {
        return None;
    }

    // While we *could* use this commented-out method, I want to be safe from
    // malformed plugin metadata.
    //Some(unsafe { Ok(CStr::from_ptr(c_str)) })

    let mut len = None;
    for i in 0..max_size {
        // Here we are assuming that all the bytes checked here up to `max_size`
        // contain data owned by the external plugin.
        //
        // This function is only visible to this crate so I'm not worried about
        // this being misused in other crates.
        //
        // Also we already checked that `c_str` is not null.
        unsafe {
            if *(c_str.add(i) as *const u8) == b"\0"[0] {
                len = Some(i + 1);
                break;
            }
        }
    }

    if let Some(len) = len {
        // Safe because we checked that `c_str` has at-least this length, and
        // because c_char and u8 have the same size.
        let c_buf = unsafe { std::slice::from_raw_parts(c_str as *const u8, len) };

        // Safe because we already checked that the last byte is a null-byte.
        let s = unsafe { CStr::from_bytes_with_nul_unchecked(c_buf) };

        Some(Ok(s.to_string_lossy()))
    } else {
        Some(Err(()))
    }
}
