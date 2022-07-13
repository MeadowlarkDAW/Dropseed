use atomic_refcell::{AtomicRef, AtomicRefMut};
use smallvec::SmallVec;

use crate::graph::shared_pool::SharedBuffer;

#[allow(unused)]
pub(crate) enum RawAudioChannelBuffers {
    F32(SmallVec<[SharedBuffer<f32>; 2]>),
    F64(SmallVec<[SharedBuffer<f64>; 2]>),
}

impl RawAudioChannelBuffers {
    fn num_channels(&self) -> usize {
        match self {
            RawAudioChannelBuffers::F32(c) => c.len(),
            RawAudioChannelBuffers::F64(c) => c.len(),
        }
    }
}

pub enum MonoBuffer<'a> {
    F32(AtomicRef<'a, Vec<f32>>),
    F64(AtomicRef<'a, Vec<f64>>),
}

pub enum MonoBufferMut<'a> {
    F32(AtomicRefMut<'a, Vec<f32>>),
    F64(AtomicRefMut<'a, Vec<f64>>),
}

pub enum StereoBuffer<'a> {
    F32(AtomicRef<'a, Vec<f32>>, AtomicRef<'a, Vec<f32>>),
    F64(AtomicRef<'a, Vec<f64>>, AtomicRef<'a, Vec<f64>>),
}

pub enum StereoBufferMut<'a> {
    F32(AtomicRefMut<'a, Vec<f32>>, AtomicRefMut<'a, Vec<f32>>),
    F64(AtomicRefMut<'a, Vec<f64>>, AtomicRefMut<'a, Vec<f64>>),
}

pub struct AudioPortBuffer {
    pub(crate) raw_channels: RawAudioChannelBuffers,

    latency: u32,

    constant_mask: u64,
}

impl std::fmt::Debug for AudioPortBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| &b.buffer.debug_id)).finish()
            }
            RawAudioChannelBuffers::F64(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| &b.buffer.debug_id)).finish()
            }
        }
    }
}

impl AudioPortBuffer {
    pub(crate) fn new(buffers: SmallVec<[SharedBuffer<f32>; 2]>, latency: u32) -> Self {
        Self { raw_channels: RawAudioChannelBuffers::F32(buffers), latency, constant_mask: 0 }
    }

    pub(crate) fn sync_constant_mask_from_buffers(&mut self) {
        self.constant_mask = 0;

        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    if buf.is_constant() {
                        self.constant_mask |= 1 << i;
                    }
                }
            }
            RawAudioChannelBuffers::F64(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    if buf.is_constant() {
                        self.constant_mask |= 1 << i;
                    }
                }
            }
        }
    }

    /*
    pub(crate) fn sync_constant_mask_to_buffers(&mut self) {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    buf.set_constant((self.constant_mask & (1 << i)) == 1);
                }
            }
            RawAudioChannelBuffers::F64(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    if buf.is_constant() {
                        buf.set_constant((self.constant_mask & (1 << i)) == 1);
                    }
                }
            }
        }
    }
    */

    pub fn num_channels(&self) -> usize {
        self.raw_channels.num_channels()
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn constant_mask(&self) -> u64 {
        self.constant_mask
    }

    #[inline]
    pub fn channel(&self, index: usize) -> Option<MonoBuffer> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                b.get(index).map(|b| MonoBuffer::F32(b.buffer.data.borrow()))
            }
            RawAudioChannelBuffers::F64(b) => {
                b.get(index).map(|b| MonoBuffer::F64(b.buffer.data.borrow()))
            }
        }
    }

    #[inline]
    pub fn mono(&self) -> MonoBuffer {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => MonoBuffer::F32(b[0].buffer.data.borrow()),
            RawAudioChannelBuffers::F64(b) => MonoBuffer::F64(b[0].buffer.data.borrow()),
        }
    }

    #[inline]
    pub fn stereo(&self) -> Option<StereoBuffer> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                if b.len() > 1 {
                    Some(StereoBuffer::F32(b[0].buffer.data.borrow(), b[1].buffer.data.borrow()))
                } else {
                    None
                }
            }
            RawAudioChannelBuffers::F64(b) => {
                if b.len() > 1 {
                    Some(StereoBuffer::F64(b[0].buffer.data.borrow(), b[1].buffer.data.borrow()))
                } else {
                    None
                }
            }
        }
    }

    #[inline]
    pub fn mono_f32(&self) -> Option<AtomicRef<Vec<f32>>> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => Some(b[0].buffer.data.borrow()),
            _ => None,
        }
    }

    #[inline]
    pub fn stereo_f32(&self) -> Option<(AtomicRef<Vec<f32>>, AtomicRef<Vec<f32>>)> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                if b.len() > 1 {
                    Some((b[0].buffer.data.borrow(), b[1].buffer.data.borrow()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn is_silent(&self, frames: usize) -> bool {
        if self.constant_mask == 0 {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        if buf_ref[0] != 0.0 {
                            return false;
                        }
                    }
                }
                RawAudioChannelBuffers::F64(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        if buf_ref[0] != 0.0 {
                            return false;
                        }
                    }
                }
            }
        } else {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        let f = frames.min(buf_ref.len());
                        let buf = &buf_ref[0..f];
                        for x in buf.iter() {
                            if *x != 0.0 {
                                return false;
                            }
                        }
                    }
                }
                RawAudioChannelBuffers::F64(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        let f = frames.min(buf_ref.len());
                        let buf = &buf_ref[0..f];
                        for x in buf.iter() {
                            if *x != 0.0 {
                                return false;
                            }
                        }
                    }
                }
            }
        }

        true
    }

    // TODO: Helper methods to retrieve more than 2 channels at once
}

pub struct AudioPortBufferMut {
    pub(crate) raw_channels: RawAudioChannelBuffers,

    latency: u32,

    constant_mask: u64,
}

impl std::fmt::Debug for AudioPortBufferMut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| &b.buffer.debug_id)).finish()
            }
            RawAudioChannelBuffers::F64(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| &b.buffer.debug_id)).finish()
            }
        }
    }
}

impl AudioPortBufferMut {
    pub(crate) fn new(buffers: SmallVec<[SharedBuffer<f32>; 2]>, latency: u32) -> Self {
        Self { raw_channels: RawAudioChannelBuffers::F32(buffers), latency, constant_mask: 0 }
    }

    /*
    pub(crate) fn sync_constant_mask_from_buffers(&mut self) {
        self.constant_mask = 0;

        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    if buf.is_constant() {
                        self.constant_mask |= 1 << i;
                    }
                }
            }
            RawAudioChannelBuffers::F64(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    if buf.is_constant() {
                        self.constant_mask |= 1 << i;
                    }
                }
            }
        }
    }
    */

    pub(crate) fn sync_constant_mask_to_buffers(&mut self) {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    buf.set_constant((self.constant_mask & (1 << i)) == 1);
                }
            }
            RawAudioChannelBuffers::F64(buffers) => {
                for (i, buf) in buffers.iter().enumerate() {
                    if buf.is_constant() {
                        buf.set_constant((self.constant_mask & (1 << i)) == 1);
                    }
                }
            }
        }
    }

    pub fn num_channels(&self) -> usize {
        self.raw_channels.num_channels()
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn constant_mask(&self) -> u64 {
        // TODO: Can we use relaxed ordering here?
        self.constant_mask
    }

    pub fn set_constant_mask(&mut self, mask: u64) {
        // TODO: Can we use relaxed ordering here?
        self.constant_mask = mask;
    }

    #[inline]
    pub fn channel(&self, index: usize) -> Option<MonoBuffer> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                b.get(index).map(|b| MonoBuffer::F32(b.buffer.data.borrow()))
            }
            RawAudioChannelBuffers::F64(b) => {
                b.get(index).map(|b| MonoBuffer::F64(b.buffer.data.borrow()))
            }
        }
    }

    #[inline]
    pub fn channel_mut(&mut self, index: usize) -> Option<MonoBufferMut> {
        match &mut self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                b.get(index).map(|b| MonoBufferMut::F32(b.buffer.data.borrow_mut()))
            }
            RawAudioChannelBuffers::F64(b) => {
                b.get(index).map(|b| MonoBufferMut::F64(b.buffer.data.borrow_mut()))
            }
        }
    }

    #[inline]
    pub fn mono(&self) -> MonoBuffer {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => MonoBuffer::F32(b[0].buffer.data.borrow()),
            RawAudioChannelBuffers::F64(b) => MonoBuffer::F64(b[0].buffer.data.borrow()),
        }
    }

    #[inline]
    pub fn mono_mut(&mut self) -> MonoBufferMut {
        match &mut self.raw_channels {
            RawAudioChannelBuffers::F32(b) => MonoBufferMut::F32(b[0].buffer.data.borrow_mut()),
            RawAudioChannelBuffers::F64(b) => MonoBufferMut::F64(b[0].buffer.data.borrow_mut()),
        }
    }

    #[inline]
    pub fn stereo(&self) -> Option<StereoBuffer> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                if b.len() > 1 {
                    Some(StereoBuffer::F32(b[0].buffer.data.borrow(), b[1].buffer.data.borrow()))
                } else {
                    None
                }
            }
            RawAudioChannelBuffers::F64(b) => {
                if b.len() > 1 {
                    Some(StereoBuffer::F64(b[0].buffer.data.borrow(), b[1].buffer.data.borrow()))
                } else {
                    None
                }
            }
        }
    }

    #[inline]
    pub fn stereo_mut(&mut self) -> Option<StereoBufferMut> {
        match &mut self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                if b.len() > 1 {
                    Some(StereoBufferMut::F32(
                        b[0].buffer.data.borrow_mut(),
                        b[1].buffer.data.borrow_mut(),
                    ))
                } else {
                    None
                }
            }
            RawAudioChannelBuffers::F64(b) => {
                if b.len() > 1 {
                    Some(StereoBufferMut::F64(
                        b[0].buffer.data.borrow_mut(),
                        b[1].buffer.data.borrow_mut(),
                    ))
                } else {
                    None
                }
            }
        }
    }

    #[inline]
    pub fn channel_f32(&self, index: usize) -> Option<AtomicRef<Vec<f32>>> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => b.get(index).map(|b| b.buffer.data.borrow()),
            _ => None,
        }
    }

    #[inline]
    pub fn channel_f32_mut(&mut self, index: usize) -> Option<AtomicRefMut<Vec<f32>>> {
        match &mut self.raw_channels {
            RawAudioChannelBuffers::F32(b) => b.get_mut(index).map(|b| b.buffer.data.borrow_mut()),
            _ => None,
        }
    }

    #[inline]
    pub fn mono_f32(&self) -> Option<AtomicRef<Vec<f32>>> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => Some(b[0].buffer.data.borrow()),
            _ => None,
        }
    }

    #[inline]
    pub fn mono_f32_mut(&mut self) -> Option<AtomicRefMut<Vec<f32>>> {
        match &mut self.raw_channels {
            RawAudioChannelBuffers::F32(b) => Some(b[0].buffer.data.borrow_mut()),
            _ => None,
        }
    }

    #[inline]
    pub fn stereo_f32(&self) -> Option<(AtomicRef<Vec<f32>>, AtomicRef<Vec<f32>>)> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                if b.len() > 1 {
                    Some((b[0].buffer.data.borrow(), b[1].buffer.data.borrow()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[inline]
    pub fn stereo_f32_mut(&mut self) -> Option<(AtomicRefMut<Vec<f32>>, AtomicRefMut<Vec<f32>>)> {
        match &mut self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                if b.len() > 1 {
                    Some((b[0].buffer.data.borrow_mut(), b[1].buffer.data.borrow_mut()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn clear_all(&mut self, frames: usize) {
        // TODO: set silence flags

        self.set_constant_mask(0);

        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                for rc_buf in buffers.iter() {
                    rc_buf.clear_f32(frames);
                }
            }
            RawAudioChannelBuffers::F64(buffers) => {
                for rc_buf in buffers.iter() {
                    rc_buf.clear_f64(frames);
                }
            }
        }
    }

    pub fn is_silent(&self, frames: usize) -> bool {
        if self.constant_mask == 0 {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        if buf_ref[0] != 0.0 {
                            return false;
                        }
                    }
                }
                RawAudioChannelBuffers::F64(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        if buf_ref[0] != 0.0 {
                            return false;
                        }
                    }
                }
            }
        } else {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        let f = frames.min(buf_ref.len());
                        let buf = &buf_ref[0..f];
                        for x in buf.iter() {
                            if *x != 0.0 {
                                return false;
                            }
                        }
                    }
                }
                RawAudioChannelBuffers::F64(buffers) => {
                    for rc_buf in buffers.iter() {
                        let buf_ref = rc_buf.borrow();
                        let f = frames.min(buf_ref.len());
                        let buf = &buf_ref[0..f];
                        for x in buf.iter() {
                            if *x != 0.0 {
                                return false;
                            }
                        }
                    }
                }
            }
        }

        true
    }

    // TODO: Helper methods to retrieve more than 2 channels at once
}
