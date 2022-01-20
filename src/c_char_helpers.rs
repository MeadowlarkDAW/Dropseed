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

pub(crate) fn c_char_buf_to_str<const BUF_SIZE: usize>(
    buf: &[c_char; BUF_SIZE],
) -> Result<Cow<'_, str>, FromBytesWithNulError> {
    // Safe because c_char and u8 have the same size.
    let c_buf_u8 = unsafe { &*((*buf).as_ptr() as *const [u8; BUF_SIZE]) };

    CStr::from_bytes_with_nul(c_buf_u8).map(|s| s.to_string_lossy())
}
