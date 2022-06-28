use audio_graph::NodeRef;
use basedrop::Shared;
use fnv::FnvHashMap;
use maybe_atomic_refcell::{MaybeAtomicRef, MaybeAtomicRefCell, MaybeAtomicRefMut};
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::engine::plugin_scanner::PluginFormat;
use crate::ProcEvent;

use super::plugin_host::{PluginInstanceHost, PluginInstanceHostAudioThread};
use super::schedule::delay_comp_node::DelayCompNode;
use super::PortChannelID;

/// Used for debugging and verifying purposes.
#[repr(u8)]
#[allow(unused)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum DebugBufferType {
    Audio32,
    Audio64,
    IntermediaryAudio32,
    ParamAutomation,
    NoteBuffer,
}
impl std::fmt::Debug for DebugBufferType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DebugBufferType::Audio32 => write!(f, "f"),
            DebugBufferType::Audio64 => write!(f, "d"),
            DebugBufferType::IntermediaryAudio32 => write!(f, "if"),
            DebugBufferType::ParamAutomation => write!(f, "pa"),
            DebugBufferType::NoteBuffer => write!(f, "n"),
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

pub(crate) struct Buffer<T: Clone + Copy + Send + 'static> {
    pub data: MaybeAtomicRefCell<Vec<T>>,
    pub is_constant: AtomicBool,
    pub debug_id: DebugBufferID,
}

impl<T: Clone + Copy + Send + 'static> Buffer<T> {
    pub fn new(max_frames: usize, debug_id: DebugBufferID) -> Self {
        Self {
            data: MaybeAtomicRefCell::new(Vec::with_capacity(max_frames)),
            is_constant: AtomicBool::new(true),
            debug_id,
        }
    }
}

impl Buffer<f32> {
    pub fn new_f32(max_frames: usize, debug_id: DebugBufferID) -> Self {
        Self {
            data: MaybeAtomicRefCell::new(vec![0.0; max_frames]),
            is_constant: AtomicBool::new(true),
            debug_id,
        }
    }
}

impl Buffer<f64> {
    pub fn new_f64(max_frames: usize, debug_id: DebugBufferID) -> Self {
        Self {
            data: MaybeAtomicRefCell::new(vec![0.0; max_frames]),
            is_constant: AtomicBool::new(true),
            debug_id,
        }
    }
}

pub(crate) struct SharedBuffer<T: Clone + Copy + Send + 'static> {
    pub buffer: Shared<Buffer<T>>,
}

impl<T: Clone + Copy + Send + 'static> SharedBuffer<T> {
    #[inline]
    pub unsafe fn borrow<'a>(&'a self) -> MaybeAtomicRef<'a, Vec<T>> {
        self.buffer.data.borrow()
    }

    #[inline]
    pub unsafe fn borrow_mut<'a>(&'a self) -> MaybeAtomicRefMut<'a, Vec<T>> {
        self.buffer.data.borrow_mut()
    }

    #[inline]
    pub fn set_constant(&self, is_constant: bool) {
        // TODO: Can we use relaxed ordering?
        self.buffer.is_constant.store(is_constant, Ordering::SeqCst);
    }

    #[inline]
    pub fn is_constant(&self) -> bool {
        // TODO: Can we use relaxed ordering?
        self.buffer.is_constant.load(Ordering::SeqCst)
    }

    #[inline]
    pub fn max_frames(&self) -> usize {
        unsafe { self.borrow().len() }
    }

    pub fn id(&self) -> &DebugBufferID {
        &self.buffer.debug_id
    }
}

impl SharedBuffer<f32> {
    pub unsafe fn clear_f32(&self, frames: usize) {
        let mut buf_ref = self.borrow_mut();

        #[cfg(debug_assertions)]
        let buf = &mut buf_ref[0..frames];
        #[cfg(not(debug_assertions))]
        let buf = std::slice::from_raw_parts_mut(buf_ref.as_mut_ptr(), frames);

        buf.fill(0.0);

        self.set_constant(true);
    }
}

impl SharedBuffer<f64> {
    pub unsafe fn clear_f64(&self, frames: usize) {
        let mut buf_ref = self.borrow_mut();

        #[cfg(debug_assertions)]
        let buf = &mut buf_ref[0..frames];
        #[cfg(not(debug_assertions))]
        let buf = std::slice::from_raw_parts_mut(buf_ref.as_mut_ptr(), frames);

        buf.fill(0.0);

        self.set_constant(true);
    }
}

impl<T: Clone + Copy + Send + 'static> Clone for SharedBuffer<T> {
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
                PluginInstanceType::Internal => "INT",
                PluginInstanceType::Clap => "CLAP",
                PluginInstanceType::Unloaded => "UNLOADED",
                PluginInstanceType::GraphInput => "GRAPH_IN",
                PluginInstanceType::GraphOutput => "GRAPH_OUT",
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
    pub(crate) rdn: Shared<String>,
}

impl PluginInstanceID {
    pub fn rdn(&self) -> &str {
        self.rdn.as_str()
    }
}

impl std::fmt::Debug for PluginInstanceID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.format {
            PluginInstanceType::Internal => {
                write!(f, "INT({})({})", &**self.rdn, self.node_ref.as_usize())
            }
            PluginInstanceType::Clap => {
                write!(f, "CLAP({})({})", &**self.rdn, self.node_ref.as_usize())
            }
            PluginInstanceType::Unloaded => {
                write!(f, "UNLOADED({})", self.node_ref.as_usize())
            }
            PluginInstanceType::GraphInput => {
                write!(f, "GRAPH_IN")
            }
            PluginInstanceType::GraphOutput => {
                write!(f, "GRAPH_OUT")
            }
        }
    }
}

impl Clone for PluginInstanceID {
    fn clone(&self) -> Self {
        Self { node_ref: self.node_ref, format: self.format, rdn: Shared::clone(&self.rdn) }
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
    pub plugin: Shared<MaybeAtomicRefCell<PluginInstanceHostAudioThread>>,
    pub task_version: u64,
}

impl SharedPluginHostAudioThread {
    pub fn new(plugin: PluginInstanceHostAudioThread, coll_handle: &basedrop::Handle) -> Self {
        Self { plugin: Shared::new(coll_handle, MaybeAtomicRefCell::new(plugin)), task_version: 0 }
    }
}

impl SharedPluginHostAudioThread {
    pub fn id(&self) -> PluginInstanceID {
        // Safe because we are just borrowing this immutably.
        unsafe { self.plugin.borrow().id.clone() }
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

    pub port_channels_refs: FnvHashMap<PortChannelID, audio_graph::PortRef>,
    pub main_audio_in_port_refs: Vec<audio_graph::PortRef>,
    pub main_audio_out_port_refs: Vec<audio_graph::PortRef>,
    pub automation_in_port_ref: Option<audio_graph::PortRef>,
    pub automation_out_port_ref: Option<audio_graph::PortRef>,
    pub main_note_in_port_ref: Option<audio_graph::PortRef>,
    pub main_note_out_port_ref: Option<audio_graph::PortRef>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DelayCompKey {
    pub src_node_ref: NodeRef,
    pub port_stable_id: u32,
    pub port_channel_index: u16,
    pub delay: u32,
}

pub(crate) struct SharedDelayCompNode {
    pub node: Shared<MaybeAtomicRefCell<DelayCompNode>>,
    pub active: bool,
}

impl SharedDelayCompNode {
    pub fn new(delay: u32, coll_handle: &basedrop::Handle) -> Self {
        Self {
            node: Shared::new(coll_handle, MaybeAtomicRefCell::new(DelayCompNode::new(delay))),
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
        unsafe { self.node.borrow().delay() }
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
    pub audio_buffer_size: u32,
    pub note_buffer_size: usize,
    pub automation_buffer_size: usize,

    audio_buffers_f32: Vec<Option<SharedBuffer<f32>>>,
    audio_buffers_f64: Vec<Option<SharedBuffer<f64>>>,

    intermediary_audio_f32: Vec<Option<SharedBuffer<f32>>>,

    automation_buffers: Vec<Option<SharedBuffer<ProcEvent>>>,
    note_buffers: Vec<Option<SharedBuffer<ProcEvent>>>,

    coll_handle: basedrop::Handle,
}

impl SharedBufferPool {
    pub fn new(
        audio_buffer_size: u32,
        note_buffer_size: usize,
        automation_buffer_size: usize,
        coll_handle: basedrop::Handle,
    ) -> Self {
        assert_ne!(audio_buffer_size, 0);
        assert_ne!(note_buffer_size, 0);
        assert_ne!(automation_buffer_size, 0);

        Self {
            audio_buffers_f32: Vec::new(),
            audio_buffers_f64: Vec::new(),
            intermediary_audio_f32: Vec::new(),
            automation_buffers: Vec::new(),
            note_buffers: Vec::new(),
            audio_buffer_size,
            note_buffer_size,
            automation_buffer_size,
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
                    Buffer::new_f32(
                        self.audio_buffer_size as usize,
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

    /*
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
                    Buffer::new_f64(
                        self.audio_buffer_size,
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
    */

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
                    Buffer::new_f32(
                        self.audio_buffer_size as usize,
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

    pub fn note_buffer(&mut self, index: usize) -> SharedBuffer<ProcEvent> {
        if self.note_buffers.len() <= index {
            let n_new_slots = (index + 1) - self.note_buffers.len();
            for _ in 0..n_new_slots {
                self.note_buffers.push(None);
            }
        }

        let slot = &mut self.note_buffers[index];

        if let Some(b) = slot {
            b.clone()
        } else {
            *slot = Some(SharedBuffer {
                buffer: Shared::new(
                    &self.coll_handle,
                    Buffer::new(
                        self.note_buffer_size,
                        DebugBufferID {
                            buffer_type: DebugBufferType::NoteBuffer,
                            index: index as u32,
                        },
                    ),
                ),
            });

            slot.as_ref().unwrap().clone()
        }
    }

    pub fn automation_buffer(&mut self, index: usize) -> SharedBuffer<ProcEvent> {
        if self.automation_buffers.len() <= index {
            let n_new_slots = (index + 1) - self.automation_buffers.len();
            for _ in 0..n_new_slots {
                self.automation_buffers.push(None);
            }
        }

        let slot = &mut self.automation_buffers[index];

        if let Some(b) = slot {
            b.clone()
        } else {
            *slot = Some(SharedBuffer {
                buffer: Shared::new(
                    &self.coll_handle,
                    Buffer::new(
                        self.automation_buffer_size,
                        DebugBufferID {
                            buffer_type: DebugBufferType::ParamAutomation,
                            index: index as u32,
                        },
                    ),
                ),
            });

            slot.as_ref().unwrap().clone()
        }
    }

    pub fn remove_excess_buffers(
        &mut self,
        max_buffer_index: usize,
        total_intermediary_buffers: usize,
        max_note_buffer_index: usize,
        max_automation_buffer_index: usize,
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
        if self.note_buffers.len() > max_note_buffer_index + 1 {
            let n_slots_to_remove = self.note_buffers.len() - (max_note_buffer_index + 1);
            for _ in 0..n_slots_to_remove {
                let _ = self.note_buffers.pop();
            }
        }
        if self.automation_buffers.len() > max_automation_buffer_index + 1 {
            let n_slots_to_remove =
                self.automation_buffers.len() - (max_automation_buffer_index + 1);
            for _ in 0..n_slots_to_remove {
                let _ = self.automation_buffers.pop();
            }
        }
    }
}
