use audio_graph::NodeRef;
use basedrop::Shared;
use fnv::FnvHashMap;
use std::cell::UnsafeCell;
use std::hash::Hash;

use crate::plugin_scanner::PluginFormat;

use super::plugin_host::{PluginInstanceHost, PluginInstanceHostAudioThread};
use super::schedule::delay_comp_node::DelayCompNode;

/// Used for debugging and verifying purposes.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum DebugBufferType {
    Audio32,
    Audio64,
    IntermediaryAudio32,
}
impl std::fmt::Debug for DebugBufferType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DebugBufferType::Audio32 => write!(f, "f"),
            DebugBufferType::Audio64 => write!(f, "d"),
            DebugBufferType::IntermediaryAudio32 => write!(f, "if"),
        }
    }
}

/// Used for debugging and verifying purposes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DebugBufferID {
    buffer_type: DebugBufferType,
    index: u32,
}

impl std::fmt::Debug for DebugBufferID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}({})", self.buffer_type, self.index)
    }
}

pub(crate) struct SharedBuffer<T: Clone + Copy + Send + Default + 'static> {
    pub buffer: Shared<(UnsafeCell<Vec<T>>, DebugBufferID)>,
}

impl<T: Clone + Copy + Send + Default + 'static> SharedBuffer<T> {
    #[inline]
    pub unsafe fn slice_from_frames_unchecked(&self, frames: usize) -> &[T] {
        #[cfg(debug_assertions)]
        return &(&*self.buffer.0.get())[0..frames];

        #[cfg(not(debug_assertions))]
        return std::slice::from_raw_parts((*self.buffer.0.get()).as_ptr(), frames);
    }

    #[inline]
    pub unsafe fn slice_from_frames_unchecked_mut(&self, frames: usize) -> &mut [T] {
        #[cfg(debug_assertions)]
        return &mut (&mut *self.buffer.0.get())[0..frames];

        #[cfg(not(debug_assertions))]
        return std::slice::from_raw_parts_mut((*self.buffer.0.get()).as_mut_ptr(), frames);
    }

    pub fn id(&self) -> &DebugBufferID {
        &self.buffer.1
    }
}

impl<T: Clone + Copy + Send + Default + 'static> Clone for SharedBuffer<T> {
    fn clone(&self) -> Self {
        Self { buffer: Shared::clone(&self.buffer) }
    }
}

/// Used for debugging and verifying purposes.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PluginInstanceType {
    Internal,
    Clap,
    Unloaded,
    GraphInput,
    GraphOutput,
}

impl std::fmt::Debug for PluginInstanceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                PluginInstanceType::Internal => "Int",
                PluginInstanceType::Clap => "CLAP",
                &PluginInstanceType::Unloaded => "Unloaded",
                PluginInstanceType::GraphInput => "GraphIn",
                PluginInstanceType::GraphOutput => "GraphOut",
            }
        )
    }
}

impl From<PluginFormat> for PluginInstanceType {
    fn from(f: PluginFormat) -> Self {
        match f {
            PluginFormat::Internal => PluginInstanceType::Internal,
            PluginFormat::Clap => PluginInstanceType::Clap,
        }
    }
}

/// Used to uniquely identify a plugin instance and for debugging purposes.
pub struct PluginInstanceID {
    pub(crate) node_ref: audio_graph::NodeRef,
    pub(crate) format: PluginInstanceType,
    pub(crate) name: Option<Shared<String>>,
}

impl std::fmt::Debug for PluginInstanceID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.format {
            PluginInstanceType::Internal => {
                write!(f, "Int({})_{}", &**self.name.as_ref().unwrap(), self.node_ref.as_usize())
            }
            _ => {
                write!(f, "{:?}_{}", self.format, self.node_ref.as_usize())
            }
        }
    }
}

impl Clone for PluginInstanceID {
    fn clone(&self) -> Self {
        Self {
            node_ref: self.node_ref,
            format: self.format,
            name: self.name.as_ref().map(|n| Shared::clone(n)),
        }
    }
}

impl PartialEq for PluginInstanceID {
    fn eq(&self, other: &Self) -> bool {
        self.node_ref.eq(&other.node_ref)
    }
}

impl Eq for PluginInstanceID {}

impl Hash for PluginInstanceID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node_ref.hash(state)
    }
}

pub(crate) struct SharedPluginHostAudioThread {
    pub plugin: Shared<UnsafeCell<PluginInstanceHostAudioThread>>,
    pub task_version: u64,
}

impl SharedPluginHostAudioThread {
    pub fn new(plugin: PluginInstanceHostAudioThread, coll_handle: &basedrop::Handle) -> Self {
        Self { plugin: Shared::new(coll_handle, UnsafeCell::new(plugin)), task_version: 0 }
    }
}

impl SharedPluginHostAudioThread {
    pub fn id(&self) -> &PluginInstanceID {
        // Safe because we are just borrowing this immutably.
        unsafe { &(*self.plugin.get()).id }
    }
}

impl Clone for SharedPluginHostAudioThread {
    fn clone(&self) -> Self {
        Self { plugin: Shared::clone(&self.plugin), task_version: self.task_version + 1 }
    }
}

pub(crate) struct PluginInstanceHostEntry {
    pub plugin_host: PluginInstanceHost,
    pub audio_thread: Option<SharedPluginHostAudioThread>,

    pub audio_in_channel_refs: Vec<audio_graph::PortRef>,
    pub audio_out_channel_refs: Vec<audio_graph::PortRef>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DelayCompKey {
    pub src_node_ref: NodeRef,
    pub port_i: u16,
    pub delay: u32,
}

pub(crate) struct SharedDelayCompNode {
    pub node: Shared<UnsafeCell<DelayCompNode>>,
    pub active: bool,
}

impl SharedDelayCompNode {
    pub fn new(delay: u32, coll_handle: &basedrop::Handle) -> Self {
        Self {
            node: Shared::new(coll_handle, UnsafeCell::new(DelayCompNode::new(delay))),
            active: true,
        }
    }
}

impl Clone for SharedDelayCompNode {
    fn clone(&self) -> Self {
        Self { node: Shared::clone(&self.node), active: self.active }
    }
}

impl SharedDelayCompNode {
    pub fn delay(&self) -> u32 {
        // Safe because we are just borrowing this immutably.
        unsafe { (*self.node.get()).delay() }
    }
}

pub(crate) struct SharedPluginPool {
    pub plugins: FnvHashMap<PluginInstanceID, PluginInstanceHostEntry>,
    pub delay_comp_nodes: FnvHashMap<DelayCompKey, SharedDelayCompNode>,
}

impl SharedPluginPool {
    pub fn new() -> Self {
        Self { plugins: FnvHashMap::default(), delay_comp_nodes: FnvHashMap::default() }
    }
}

pub(crate) struct SharedBufferPool {
    pub buffer_size: usize,

    audio_buffers_f32: Vec<Option<SharedBuffer<f32>>>,
    audio_buffers_f64: Vec<Option<SharedBuffer<f64>>>,

    intermediary_audio_f32: Vec<Option<SharedBuffer<f32>>>,

    coll_handle: basedrop::Handle,
}

impl SharedBufferPool {
    pub fn new(buffer_size: usize, coll_handle: basedrop::Handle) -> Self {
        assert_ne!(buffer_size, 0);

        Self {
            audio_buffers_f32: Vec::new(),
            audio_buffers_f64: Vec::new(),
            intermediary_audio_f32: Vec::new(),
            buffer_size,
            coll_handle,
        }
    }

    pub fn audio_f32(&mut self, index: usize) -> SharedBuffer<f32> {
        if self.audio_buffers_f32.len() <= index {
            let n_new_slots = (index + 1) - self.audio_buffers_f32.len();
            for _ in 0..n_new_slots {
                self.audio_buffers_f32.push(None);
            }
        }

        let slot = &mut self.audio_buffers_f32[index];

        if let Some(b) = slot {
            b.clone()
        } else {
            *slot = Some(SharedBuffer {
                buffer: Shared::new(
                    &self.coll_handle,
                    (
                        UnsafeCell::new(vec![0.0; self.buffer_size]),
                        DebugBufferID {
                            buffer_type: DebugBufferType::Audio32,
                            index: index as u32,
                        },
                    ),
                ),
            });

            slot.as_ref().unwrap().clone()
        }
    }

    pub fn audio_f64(&mut self, index: usize) -> SharedBuffer<f64> {
        if self.audio_buffers_f64.len() <= index {
            let n_new_slots = (index + 1) - self.audio_buffers_f64.len();
            for _ in 0..n_new_slots {
                self.audio_buffers_f64.push(None);
            }
        }

        let slot = &mut self.audio_buffers_f64[index];

        if let Some(b) = slot {
            b.clone()
        } else {
            *slot = Some(SharedBuffer {
                buffer: Shared::new(
                    &self.coll_handle,
                    (
                        UnsafeCell::new(vec![0.0; self.buffer_size]),
                        DebugBufferID {
                            buffer_type: DebugBufferType::Audio64,
                            index: index as u32,
                        },
                    ),
                ),
            });

            slot.as_ref().unwrap().clone()
        }
    }

    pub fn intermediary_audio_f32(&mut self, index: usize) -> SharedBuffer<f32> {
        if self.intermediary_audio_f32.len() <= index {
            let n_new_slots = (index + 1) - self.intermediary_audio_f32.len();
            for _ in 0..n_new_slots {
                self.intermediary_audio_f32.push(None);
            }
        }

        let slot = &mut self.intermediary_audio_f32[index];

        if let Some(b) = slot {
            b.clone()
        } else {
            *slot = Some(SharedBuffer {
                buffer: Shared::new(
                    &self.coll_handle,
                    (
                        UnsafeCell::new(vec![0.0; self.buffer_size]),
                        DebugBufferID {
                            buffer_type: DebugBufferType::IntermediaryAudio32,
                            index: index as u32,
                        },
                    ),
                ),
            });

            slot.as_ref().unwrap().clone()
        }
    }

    pub fn remove_excess_audio_buffers(
        &mut self,
        max_buffer_index: usize,
        total_intermediary_buffers: usize,
    ) {
        if self.audio_buffers_f32.len() > max_buffer_index + 1 {
            let n_slots_to_remove = self.audio_buffers_f32.len() - (max_buffer_index + 1);
            for _ in 0..n_slots_to_remove {
                let _ = self.audio_buffers_f32.pop();
            }
        }
        if self.audio_buffers_f64.len() > max_buffer_index + 1 {
            let n_slots_to_remove = self.audio_buffers_f64.len() - (max_buffer_index + 1);
            for _ in 0..n_slots_to_remove {
                let _ = self.audio_buffers_f64.pop();
            }
        }
        if self.intermediary_audio_f32.len() > total_intermediary_buffers {
            let n_slots_to_remove = self.intermediary_audio_f32.len() - total_intermediary_buffers;
            for _ in 0..n_slots_to_remove {
                let _ = self.intermediary_audio_f32.pop();
            }
        }
    }
}
