use std::ffi::c_void;
use std::pin::Pin;

use clap_sys::stream::clap_istream as RawClapIStream;
use clap_sys::stream::clap_ostream as RawClapOStream;

struct ClapIStreamCtx {
    buffer: *const Vec<u8>,
    total_bytes_read: usize,
}

pub(crate) struct ClapIStream {
    raw: Pin<Box<RawClapIStream>>,
    _ctx: Pin<Box<ClapIStreamCtx>>,
}

impl ClapIStream {
    pub fn new(in_buffer: &Vec<u8>) -> Self {
        let ctx = Pin::new(Box::new(ClapIStreamCtx { buffer: in_buffer, total_bytes_read: 0 }));

        Self {
            raw: Pin::new(Box::new(RawClapIStream {
                ctx: &*ctx as *const ClapIStreamCtx as *mut c_void,
                read,
            })),
            _ctx: ctx,
        }
    }

    pub fn raw(&self) -> *const RawClapIStream {
        &*self.raw
    }
}

struct ClapOStreamCtx {
    buffer: *mut Vec<u8>,
}

pub(crate) struct ClapOStream {
    raw: Pin<Box<RawClapOStream>>,
    _ctx: Pin<Box<ClapOStreamCtx>>,
}

impl ClapOStream {
    pub fn new(out_buffer: &mut Vec<u8>) -> Self {
        let ctx = Pin::new(Box::new(ClapOStreamCtx { buffer: out_buffer }));

        Self {
            raw: Pin::new(Box::new(RawClapOStream {
                ctx: &*ctx as *const ClapOStreamCtx as *mut c_void,
                write,
            })),
            _ctx: ctx,
        }
    }

    pub fn raw(&self) -> *const RawClapOStream {
        &*self.raw
    }
}

unsafe extern "C" fn read(
    stream: *const RawClapIStream,
    out_buffer: *mut c_void,
    size: u64,
) -> i64 {
    if stream.is_null() {
        log::warn!(
            "Received a null clap_istream pointer from plugin in call to clap_istream->read()"
        );
        return -1;
    }

    let stream = &*stream;

    if stream.ctx.is_null() {
        log::warn!(
            "Received a null clap_istream.ctx pointer from plugin in call to clap_istream->read()"
        );
        return -1;
    }

    if out_buffer.is_null() {
        log::warn!(
            "Received a null void *buffer pointer from plugin in call to clap_istream->read()"
        );
        return -1;
    }

    let ctx: &mut ClapIStreamCtx = &mut *(stream.ctx as *mut ClapIStreamCtx);

    let in_buffer: &Vec<u8> = &*(ctx.buffer);

    if ctx.total_bytes_read >= in_buffer.len() {
        return 0;
    }

    let out_buffer = std::slice::from_raw_parts_mut(out_buffer as *mut u8, size as usize);

    let read_bytes = (in_buffer.len() - ctx.total_bytes_read).min(out_buffer.len());

    out_buffer[0..read_bytes]
        .copy_from_slice(&in_buffer[ctx.total_bytes_read..ctx.total_bytes_read + read_bytes]);

    ctx.total_bytes_read += read_bytes;

    read_bytes as i64
}

unsafe extern "C" fn write(
    stream: *const RawClapOStream,
    in_buffer: *const c_void,
    size: u64,
) -> i64 {
    if stream.is_null() {
        log::warn!(
            "Received a null clap_ostream pointer from plugin in call to clap_ostream->write()"
        );
        return -1;
    }

    let stream = &*stream;

    if stream.ctx.is_null() {
        log::warn!(
            "Received a null clap_ostream.ctx pointer from plugin in call to clap_ostream->write()"
        );
        return -1;
    }

    if in_buffer.is_null() {
        log::warn!(
            "Received a null const void *buffer pointer from plugin in call to clap_ostream->write()"
        );
        return -1;
    }

    let ctx: &mut ClapOStreamCtx = &mut *(stream.ctx as *mut ClapOStreamCtx);

    let out_buffer: &mut Vec<u8> = &mut *(ctx.buffer);

    let in_buffer = std::slice::from_raw_parts(in_buffer as *const u8, size as usize);

    out_buffer.extend_from_slice(in_buffer);

    size as i64
}
