use smallvec::SmallVec;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::graph::shared_pool::SharedBuffer;

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

    unsafe fn f32_unchecked(&self) -> &SmallVec<[SharedBuffer<f32>; 2]> {
        if let RawAudioChannelBuffers::F32(b) = &self {
            b
        } else {
            #[cfg(debug_assertions)]
            std::unreachable!();

            #[cfg(not(debug_assertions))]
            std::hint::unreachable_unchecked();
        }
    }

    unsafe fn f32_unchecked_mut(&mut self) -> &mut SmallVec<[SharedBuffer<f32>; 2]> {
        if let RawAudioChannelBuffers::F32(b) = self {
            b
        } else {
            #[cfg(debug_assertions)]
            std::unreachable!();

            #[cfg(not(debug_assertions))]
            std::hint::unreachable_unchecked();
        }
    }
}

pub enum MonoBuffer<'a> {
    F32(&'a [f32]),
    F64(&'a [f64]),
}

pub enum MonoBufferMut<'a> {
    F32(&'a mut [f32]),
    F64(&'a mut [f64]),
}

pub enum StereoBuffer<'a> {
    F32(&'a [f32], &'a [f32]),
    F64(&'a [f64], &'a [f64]),
}

pub enum StereoBufferMut<'a> {
    F32(&'a mut [f32], &'a mut [f32]),
    F64(&'a mut [f64], &'a mut [f64]),
}

pub struct AudioPortBuffer {
    pub(crate) raw_channels: RawAudioChannelBuffers,

    latency: u32,

    constant_mask: Arc<AtomicU64>,
}

impl std::fmt::Debug for AudioPortBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| b.buffer.1)).finish()
            }
            RawAudioChannelBuffers::F64(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| b.buffer.1)).finish()
            }
        }
    }
}

impl AudioPortBuffer {
    pub(crate) fn new(buffers: SmallVec<[SharedBuffer<f32>; 2]>, latency: u32) -> Self {
        Self {
            raw_channels: RawAudioChannelBuffers::F32(buffers),
            latency,
            constant_mask: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn num_channels(&self) -> usize {
        self.raw_channels.num_channels()
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn constant_mask(&self) -> u64 {
        self.constant_mask.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn channel<'a>(&self, index: usize) -> Option<MonoBuffer<'a>> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                b.get(index).map(|b| MonoBuffer::F32(unsafe { &*b.buffer.0.get() }))
            }
            RawAudioChannelBuffers::F64(b) => {
                b.get(index).map(|b| MonoBuffer::F64(unsafe { &*b.buffer.0.get() }))
            }
        }
    }

    #[inline]
    pub fn mono<'a>(&self) -> MonoBuffer<'a> {
        // Safe because we are guaranteed to have at-least one channel.
        unsafe {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(b) => {
                    MonoBuffer::F32(&*b.get_unchecked(0).buffer.0.get())
                }
                RawAudioChannelBuffers::F64(b) => {
                    MonoBuffer::F64(&*b.get_unchecked(0).buffer.0.get())
                }
            }
        }
    }

    #[inline]
    pub fn stereo<'a>(&self) -> Option<StereoBuffer<'a>> {
        unsafe {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(b) => {
                    if b.len() > 1 {
                        Some(StereoBuffer::F32(
                            &*b.get_unchecked(0).buffer.0.get(),
                            &*b.get_unchecked(1).buffer.0.get(),
                        ))
                    } else {
                        None
                    }
                }
                RawAudioChannelBuffers::F64(b) => {
                    if b.len() > 1 {
                        Some(StereoBuffer::F64(
                            &*b.get_unchecked(0).buffer.0.get(),
                            &*b.get_unchecked(1).buffer.0.get(),
                        ))
                    } else {
                        None
                    }
                }
            }
        }
    }

    #[inline]
    pub unsafe fn mono_f32_unchecked(&self) -> &[f32] {
        &*self.raw_channels.f32_unchecked().get_unchecked(0).buffer.0.get()
    }

    #[inline]
    pub unsafe fn stereo_f32_unchecked(&self) -> (&[f32], &[f32]) {
        (
            &*self.raw_channels.f32_unchecked().get_unchecked(0).buffer.0.get(),
            &*self.raw_channels.f32_unchecked().get_unchecked(1).buffer.0.get(),
        )
    }

    pub fn is_silent(&self, frames: usize) -> bool {
        // TODO
        false
    }

    // TODO: Helper methods to retrieve more than 2 channels at once
}

pub struct AudioPortBufferMut {
    pub(crate) raw_channels: RawAudioChannelBuffers,

    latency: u32,

    constant_mask: Arc<AtomicU64>,
}

impl std::fmt::Debug for AudioPortBufferMut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| b.buffer.1)).finish()
            }
            RawAudioChannelBuffers::F64(buffers) => {
                f.debug_list().entries(buffers.iter().map(|b| b.buffer.1)).finish()
            }
        }
    }
}

impl AudioPortBufferMut {
    pub(crate) fn new(buffers: SmallVec<[SharedBuffer<f32>; 2]>, latency: u32) -> Self {
        Self {
            raw_channels: RawAudioChannelBuffers::F32(buffers),
            latency,
            constant_mask: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn num_channels(&self) -> usize {
        self.raw_channels.num_channels()
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn constant_mask(&self) -> u64 {
        self.constant_mask.load(Ordering::Relaxed)
    }

    pub fn set_constant_mask(&mut self, mask: u64) {
        self.constant_mask.store(mask, Ordering::Relaxed);
    }

    #[inline]
    pub fn channel<'a>(&self, index: usize) -> Option<MonoBuffer<'a>> {
        match &self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                b.get(index).map(|b| MonoBuffer::F32(unsafe { &*b.buffer.0.get() }))
            }
            RawAudioChannelBuffers::F64(b) => {
                b.get(index).map(|b| MonoBuffer::F64(unsafe { &*b.buffer.0.get() }))
            }
        }
    }

    #[inline]
    pub fn channel_mut<'a>(&mut self, index: usize) -> Option<MonoBufferMut<'a>> {
        match &mut self.raw_channels {
            RawAudioChannelBuffers::F32(b) => {
                b.get(index).map(|b| MonoBufferMut::F32(unsafe { &mut *b.buffer.0.get() }))
            }
            RawAudioChannelBuffers::F64(b) => {
                b.get(index).map(|b| MonoBufferMut::F64(unsafe { &mut *b.buffer.0.get() }))
            }
        }
    }

    #[inline]
    pub fn mono<'a>(&self) -> MonoBuffer<'a> {
        // Safe because we are guaranteed to have at-least one channel.
        unsafe {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(b) => {
                    MonoBuffer::F32(&*b.get_unchecked(0).buffer.0.get())
                }
                RawAudioChannelBuffers::F64(b) => {
                    MonoBuffer::F64(&*b.get_unchecked(0).buffer.0.get())
                }
            }
        }
    }

    #[inline]
    pub fn mono_mut<'a>(&mut self) -> MonoBufferMut<'a> {
        // Safe because we are guaranteed to have at-least one channel.
        unsafe {
            match &mut self.raw_channels {
                RawAudioChannelBuffers::F32(b) => {
                    MonoBufferMut::F32(&mut *b.get_unchecked(0).buffer.0.get())
                }
                RawAudioChannelBuffers::F64(b) => {
                    MonoBufferMut::F64(&mut *b.get_unchecked(0).buffer.0.get())
                }
            }
        }
    }

    #[inline]
    pub fn stereo<'a>(&self) -> Option<StereoBuffer<'a>> {
        unsafe {
            match &self.raw_channels {
                RawAudioChannelBuffers::F32(b) => {
                    if b.len() > 1 {
                        Some(StereoBuffer::F32(
                            &*b.get_unchecked(0).buffer.0.get(),
                            &*b.get_unchecked(1).buffer.0.get(),
                        ))
                    } else {
                        None
                    }
                }
                RawAudioChannelBuffers::F64(b) => {
                    if b.len() > 1 {
                        Some(StereoBuffer::F64(
                            &*b.get_unchecked(0).buffer.0.get(),
                            &*b.get_unchecked(1).buffer.0.get(),
                        ))
                    } else {
                        None
                    }
                }
            }
        }
    }

    #[inline]
    pub fn stereo_mut<'a>(&mut self) -> Option<StereoBufferMut<'a>> {
        unsafe {
            match &mut self.raw_channels {
                RawAudioChannelBuffers::F32(b) => {
                    if b.len() > 1 {
                        Some(StereoBufferMut::F32(
                            &mut *b.get_unchecked(0).buffer.0.get(),
                            &mut *b.get_unchecked(1).buffer.0.get(),
                        ))
                    } else {
                        None
                    }
                }
                RawAudioChannelBuffers::F64(b) => {
                    if b.len() > 1 {
                        Some(StereoBufferMut::F64(
                            &mut *b.get_unchecked(0).buffer.0.get(),
                            &mut *b.get_unchecked(1).buffer.0.get(),
                        ))
                    } else {
                        None
                    }
                }
            }
        }
    }

    #[inline]
    pub unsafe fn mono_f32_unchecked(&self) -> &[f32] {
        &*self.raw_channels.f32_unchecked().get_unchecked(0).buffer.0.get()
    }

    #[inline]
    pub unsafe fn mono_f32_unchecked_mut(&mut self) -> &mut [f32] {
        &mut *self.raw_channels.f32_unchecked_mut().get_unchecked(0).buffer.0.get()
    }

    #[inline]
    pub unsafe fn stereo_f32_unchecked(&self) -> (&[f32], &[f32]) {
        (
            &*self.raw_channels.f32_unchecked().get_unchecked(0).buffer.0.get(),
            &*self.raw_channels.f32_unchecked().get_unchecked(1).buffer.0.get(),
        )
    }

    #[inline]
    pub unsafe fn stereo_f32_unchecked_mut(&mut self) -> (&mut [f32], &mut [f32]) {
        (
            &mut *self.raw_channels.f32_unchecked_mut().get_unchecked(0).buffer.0.get(),
            &mut *self.raw_channels.f32_unchecked_mut().get_unchecked(1).buffer.0.get(),
        )
    }

    pub unsafe fn clear_all_unchecked(&mut self, frames: usize) {
        // TODO: set silence flags

        match &self.raw_channels {
            RawAudioChannelBuffers::F32(buffers) => {
                for rc_buf in buffers.iter() {
                    let buf = rc_buf.slice_from_frames_unchecked_mut(frames);
                    buf.fill(0.0);
                }
            }
            RawAudioChannelBuffers::F64(buffers) => {
                for rc_buf in buffers.iter() {
                    let buf = rc_buf.slice_from_frames_unchecked_mut(frames);
                    buf.fill(0.0);
                }
            }
        }
    }

    pub fn is_silent(&self, frames: usize) -> bool {
        // TODO
        false
    }

    // TODO: Helper methods to retrieve more than 2 channels at once
}
