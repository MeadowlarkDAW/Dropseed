use std::cell::UnsafeCell;

use audio_graph::NodeRef;
use basedrop::Shared;
use fnv::FnvHashMap;
use smallvec::SmallVec;

use clap_sys::audio_buffer::clap_audio_buffer;

use crate::ProcInfo;

// ----- SAFETY NOTE  -------------------------------------------------------
//
// We're using Shared (reference counted) pointers to buffers wrapped in
// UnsafeCell's for passing buffers around, as well as storing plain ol'
// raw pointers for passing these buffers to external plugiins. Before you
// yell at me, let me explain why:
//
// A large goal is to have first-class support for the CLAP plugin spec.
// Using raw pointers (or more specifically the types defined in the CLAP
// C bindings) will allow us to host CLAP plugins without needing to do
// any kind of conversion between specs at runtime, (just pass the raw
// pointer to the clap plugin).
//
// Also managing mutable smart pointers across multiple threads in Rust
// without using any unsafe code is just a pain.
//
// That being said, here are the reasons why safety is upheld:
//
// - Access to these UnsafeCell's and raw pointers are constrained to this
// file only. Access to buffers outside of this file are provided via a
// safe abstraction.
//   - Borrowing channels immutably and mutably through the ProcAudioBuffer
//     require borrowing `self` as immutable or mutable. Thus the rules of
//     mutable aliasing of individual channels are enforced for the user.
//   - Borrowing the raw buffers for use in external plugins is not
//     technically unsafe. Only once the user tries to dereference those
//     pointers does it count as unsafe, and Rust will still require the
//     user to use an unsafe block to do so.
// - Every time a new Schedule is compiled, it is sent to the
// Verifier to verify there exists no data races in the schedule. If a
// violation is found then the schedule is discarded and the graph is
// reverted to the previous working state. More specifically the verifer
// checks these things:
//   - Makes sure that a buffer ID does not appear twice within the same
//     ScheduledTask.
//   - Makes sure that a buffer ID does not appear in multiple parallel
//     ScheduledThreadTask's. (once we get multithreaded scheduling)
// - The lifetime of each buffer is kept track of using basedrop's Shared
// reference counting pointer type. Thus the garbage collector only
// deallocates a certain buffer once all references to that buffer are
// dropped (which only happens once the rt thread drops an old schedule
// and replaces it with a new one at the top of the process cycle).
//
// --------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct SharedAudioBuffer<T: Clone + Copy + Send + Default + 'static> {
    buffer: Shared<(UnsafeCell<Vec<T>>, UniqueBufferID)>,
}

impl<T: Clone + Copy + Send + Default + 'static> SharedAudioBuffer<T> {
    fn new(id: UniqueBufferID, max_frames: usize, coll_handle: &basedrop::Handle) -> Self {
        Self {
            buffer: Shared::new(
                coll_handle,
                (UnsafeCell::new(vec![Default::default(); max_frames]), id),
            ),
        }
    }

    pub fn unique_id(&self) -> UniqueBufferID {
        self.buffer.1
    }

    #[inline]
    pub(crate) fn borrow(&self, proc_info: &ProcInfo) -> &[T] {
        // Please refer to the "SAFETY NOTE" above on why this is safe.
        //
        // In addition the host will never set `proc_info.frames` to something
        // higher than the maximum frame size (which is what the Vec's initial
        // capacity is set to).
        unsafe {
            std::slice::from_raw_parts((*self.buffer.0.get()).as_slice().as_ptr(), proc_info.frames)
        }
    }

    #[inline]
    pub(crate) fn borrow_mut(&self, proc_info: &ProcInfo) -> &mut [T] {
        // Please refer to the "SAFETY NOTE" above on why this is safe.
        //
        // In addition the host will never set `proc_info.frames` to something
        // higher than the maximum frame size (which is what the Vec's initial
        // capacity is set to).
        unsafe {
            std::slice::from_raw_parts_mut(
                (*self.buffer.0.get()).as_mut_slice().as_mut_ptr(),
                proc_info.frames,
            )
        }
    }
}

/// An audio port buffer for use with external clap plugins.
pub(crate) struct ClapAudioBuffer {
    raw: clap_audio_buffer,

    raw_buffers: SmallVec<[*const f32; 2]>,

    /// We keep a reference-counted pointer to the same buffers in `raw` so
    /// the garbage collector can know when it is safe to deallocate unused
    /// buffers.
    rc_buffers: SmallVec<[SharedAudioBuffer<f32>; 2]>,

    frames: usize,
}

impl std::fmt::Debug for ClapAudioBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.rc_buffers.iter().map(|b| b.buffer.1)).finish()
    }
}

impl ClapAudioBuffer {
    pub(crate) fn new(buffers: SmallVec<[SharedAudioBuffer<f32>; 2]>, latency: u32) -> Self {
        assert_ne!(buffers.len(), 0);

        let raw_buffers: SmallVec<[*const f32; 2]> = buffers
            .iter()
            .map(|b| {
                // Please refer to the "SAFETY NOTE" above on why this is safe.
                //
                // Also, basedrop's `Shared` pointer never moves its contents.
                unsafe { (*b.buffer.0.get()).as_ptr() }
            })
            .collect();

        let raw = clap_audio_buffer {
            data32: std::ptr::null(),
            data64: std::ptr::null(),
            channel_count: buffers.len() as u32,
            latency,
            constant_mask: 0,
        };

        Self { raw, raw_buffers, rc_buffers: buffers, frames: 0 }
    }

    pub(crate) fn as_raw(&mut self) -> *const clap_audio_buffer {
        self.raw.data32 = self.raw_buffers.as_ptr();
        &self.raw
    }
}

/// An audio port buffer for use with internal plugins.
pub struct AudioPortBuffer {
    latency: u32,     // latency from/to the audio interface
    silent_mask: u64, // mask & (1 << N) to test if channel N is silent

    rc_buffers: SmallVec<[SharedAudioBuffer<f32>; 2]>,
}

impl std::fmt::Debug for AudioPortBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.rc_buffers.iter().map(|b| b.buffer.1)).finish()
    }
}

impl AudioPortBuffer {
    pub(crate) fn new(buffers: SmallVec<[SharedAudioBuffer<f32>; 2]>, latency: u32) -> Self {
        assert_ne!(buffers.len(), 0);
        buffers.len() as u32;

        Self {
            /*
            raw_buffers,
            channel_count,
            */
            latency,
            silent_mask: 0,
            rc_buffers: buffers,
        }
    }

    pub(crate) fn rc_buffers(&self) -> &[SharedAudioBuffer<f32>] {
        &self.rc_buffers
    }

    /// The number of channels in this buffer.
    pub fn channel_count(&self) -> usize {
        self.rc_buffers.len()
    }

    /// The latency from/to the audio interface.
    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// Immutably borrow a channel.
    ///
    /// This will return `None` if the channel with the given index does not exist.
    #[inline]
    pub fn channel(&self, channel: usize, proc_info: &ProcInfo) -> Option<&[f32]> {
        self.rc_buffers.get(channel).map(|b| b.borrow(proc_info))
    }

    /// Immutably borrow a channel without checking that the channel with the given index exists.
    #[inline]
    pub unsafe fn channel_unchecked(&self, channel: usize, proc_info: &ProcInfo) -> &[f32] {
        self.rc_buffers.get_unchecked(channel).borrow(proc_info)
    }

    /// Mutably borrow a channel.
    ///
    /// This will return `None` if the channel with the given index does not exist.
    #[inline]
    pub fn channel_mut(&mut self, channel: usize, proc_info: &ProcInfo) -> Option<&mut [f32]> {
        self.rc_buffers.get(channel).map(|b| b.borrow_mut(proc_info))
    }

    /// Mutably borrow a channel without checking that the channel with the given index exists.
    #[inline]
    pub unsafe fn channel_unchecked_mut(
        &mut self,
        channel: usize,
        proc_info: &ProcInfo,
    ) -> &mut [f32] {
        self.rc_buffers.get_unchecked(channel).borrow_mut(proc_info)
    }

    /// Immutably borrow the first/only channel in this buffer.
    #[inline]
    pub fn mono(&self, proc_info: &ProcInfo) -> &[f32] {
        // This is safe because we assert in the constructor that the number of
        // buffers is never 0.
        unsafe { self.rc_buffers.get_unchecked(0).borrow(proc_info) }
    }

    /// Mutably borrow the first/only channel in this buffer.
    #[inline]
    pub fn mono_mut(&mut self, proc_info: &ProcInfo) -> &mut [f32] {
        // This is safe because we assert in the constructor that the number of
        // buffers is never 0.
        unsafe { self.rc_buffers.get_unchecked(0).borrow_mut(proc_info) }
    }

    /// Immutably borrow the first two (or only two) channels in this buffer.
    ///
    /// This will return an error if the buffer is mono.
    #[inline]
    pub fn stereo(&self, proc_info: &ProcInfo) -> Option<(&[f32], &[f32])> {
        if self.rc_buffers.len() > 1 {
            Some((self.rc_buffers[0].borrow(proc_info), self.rc_buffers[1].borrow(proc_info)))
        } else {
            None
        }
    }

    /// Immutably borrow the first two (or only two) channels in this buffer without
    /// checking if the buffer is not mono.
    #[inline]
    pub unsafe fn stereo_unchecked(&self, proc_info: &ProcInfo) -> (&[f32], &[f32]) {
        (
            self.rc_buffers.get_unchecked(0).borrow(proc_info),
            self.rc_buffers.get_unchecked(1).borrow(proc_info),
        )
    }

    /// Mutably borrow the first two (or only two) channels in this buffer.
    ///
    /// This will return an error if the buffer is mono.
    #[inline]
    pub fn stereo_mut(&mut self, proc_info: &ProcInfo) -> Option<(&mut [f32], &mut [f32])> {
        if self.rc_buffers.len() > 1 {
            Some((
                self.rc_buffers[0].borrow_mut(proc_info),
                self.rc_buffers[1].borrow_mut(proc_info),
            ))
        } else {
            None
        }
    }

    /// Mutably borrow the first two (or only two) channels in this buffer without
    /// checking if the buffer is not mono.
    #[inline]
    pub unsafe fn stereo_unchecked_mut(
        &mut self,
        proc_info: &ProcInfo,
    ) -> (&mut [f32], &mut [f32]) {
        (
            self.rc_buffers.get_unchecked(0).borrow_mut(proc_info),
            self.rc_buffers.get_unchecked(1).borrow_mut(proc_info),
        )
    }

    #[inline]
    pub fn clear(&mut self, proc_info: &ProcInfo) {
        for b in self.rc_buffers.iter() {
            b.borrow_mut(proc_info).fill(0.0);
        }
    }

    // TODO: Methods for borrowing more than 2 channel buffers at a time.
}

/// Used for debugging and verifying purposes.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum UniqueBufferType {
    /// An audio buffer directly referenced by the AudioGraph
    /// compiler (by buffer index).
    Graph,

    /// An audio buffer created for any intermediary steps.
    Intermediary,

    /// An audio buffer created for an unconnected input port.
    EmptyInput,
}

impl std::fmt::Debug for UniqueBufferType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                UniqueBufferType::Graph => "G",
                UniqueBufferType::Intermediary => "I",
                UniqueBufferType::EmptyInput => "E",
            }
        )
    }
}

/// Used for debugging and verifying purposes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UniqueBufferID {
    pub buffer_type: UniqueBufferType,
    pub index: u32,
}

impl std::fmt::Debug for UniqueBufferID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}_{}", self.buffer_type, self.index)
    }
}

pub(crate) struct AudioBufferPool {
    pub coll_handle: basedrop::Handle,

    graph_buffers: Vec<SharedAudioBuffer<f32>>,
    intermediary_buffers: Vec<SharedAudioBuffer<f32>>,
    empty_input_buffers: Vec<SharedAudioBuffer<f32>>,

    max_block_size: usize,
}

impl AudioBufferPool {
    pub fn new(coll_handle: basedrop::Handle, max_block_size: usize) -> Self {
        Self {
            graph_buffers: Vec::new(),
            intermediary_buffers: Vec::new(),
            empty_input_buffers: Vec::new(),

            max_block_size,
            coll_handle,
        }
    }

    /// Retrieve an audio buffer directly referenced by the AudioGraph
    /// compiler (by buffer index).
    pub fn get_graph_buffer(&mut self, buffer_index: usize) -> SharedAudioBuffer<f32> {
        // Resize if buffer does not exist.
        if self.graph_buffers.len() <= buffer_index {
            let n_new_buffers = (buffer_index + 1) - self.graph_buffers.len();
            for _ in 0..n_new_buffers {
                self.graph_buffers.push(SharedAudioBuffer::new(
                    UniqueBufferID {
                        buffer_type: UniqueBufferType::Graph,
                        index: buffer_index as u32,
                    },
                    self.max_block_size,
                    &self.coll_handle,
                ));
            }
        }

        self.graph_buffers[buffer_index].clone()
    }

    /// Retrieve an audio buffer used for any intermediary steps.
    pub fn get_intermediary_buffer(&mut self, buffer_index: usize) -> SharedAudioBuffer<f32> {
        // Resize if buffer does not exist.
        if self.intermediary_buffers.len() <= buffer_index {
            let n_new_buffers = (buffer_index + 1) - self.intermediary_buffers.len();
            for _ in 0..n_new_buffers {
                self.intermediary_buffers.push(SharedAudioBuffer::new(
                    UniqueBufferID {
                        buffer_type: UniqueBufferType::Intermediary,
                        index: buffer_index as u32,
                    },
                    self.max_block_size,
                    &self.coll_handle,
                ));
            }
        }

        self.intermediary_buffers[buffer_index].clone()
    }

    /// Retrieve an audio buffer used for any intermediary steps.
    pub fn get_empty_input_buffer(&mut self, buffer_index: usize) -> SharedAudioBuffer<f32> {
        // Resize if buffer does not exist.
        if self.empty_input_buffers.len() <= buffer_index {
            let n_new_buffers = (buffer_index + 1) - self.empty_input_buffers.len();
            for _ in 0..n_new_buffers {
                self.empty_input_buffers.push(SharedAudioBuffer::new(
                    UniqueBufferID {
                        buffer_type: UniqueBufferType::EmptyInput,
                        index: buffer_index as u32,
                    },
                    self.max_block_size,
                    &self.coll_handle,
                ));
            }
        }

        self.empty_input_buffers[buffer_index].clone()
    }

    pub fn remove_unneeded_graph_buffers(
        &mut self,
        max_graph_audio_buffer_id: usize,
    ) -> Result<(), usize> {
        if max_graph_audio_buffer_id < self.graph_buffers.len() {
            let n_to_remove = self.graph_buffers.len() - (max_graph_audio_buffer_id + 1);
            for _ in 0..n_to_remove {
                let _ = self.graph_buffers.pop();
            }
            Ok(())
        } else {
            Err(self.graph_buffers.len())
        }
    }

    pub fn remove_unneeded_intermediary_buffers(
        &mut self,
        total_intermediary_buffers: usize,
    ) -> Result<(), usize> {
        if total_intermediary_buffers <= self.intermediary_buffers.len() {
            let n_to_remove = self.intermediary_buffers.len() - total_intermediary_buffers;
            for _ in 0..n_to_remove {
                let _ = self.intermediary_buffers.pop();
            }
            Ok(())
        } else {
            Err(self.graph_buffers.len())
        }
    }

    pub fn max_block_size(&self) -> usize {
        self.max_block_size
    }
}
