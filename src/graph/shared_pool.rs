use atomic_refcell::AtomicRefCell;
use basedrop::Shared;
use fnv::FnvHashMap;
use std::hash::Hash;

use dropseed_core::plugin::buffer::{DebugBufferID, DebugBufferType, SharedBuffer};
use dropseed_core::plugin::{PluginInstanceID, ProcEvent};

use super::plugin_host::{PluginInstanceHost, PluginInstanceHostAudioThread};
use super::schedule::delay_comp_node::DelayCompNode;
use super::PortChannelID;

pub(crate) struct SharedPluginHostAudioThread {
    pub plugin: Shared<AtomicRefCell<PluginInstanceHostAudioThread>>,
}

impl SharedPluginHostAudioThread {
    pub fn new(plugin: PluginInstanceHostAudioThread, coll_handle: &basedrop::Handle) -> Self {
        Self { plugin: Shared::new(coll_handle, AtomicRefCell::new(plugin)) }
    }
}

impl SharedPluginHostAudioThread {
    pub fn id(&self) -> PluginInstanceID {
        self.plugin.borrow().id.clone()
    }
}

impl Clone for SharedPluginHostAudioThread {
    fn clone(&self) -> Self {
        Self { plugin: Shared::clone(&self.plugin) }
    }
}

pub(crate) struct PluginInstanceHostEntry {
    pub plugin_host: PluginInstanceHost,
    //pub audio_thread: Option<SharedPluginHostAudioThread>,
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
    pub src_node_ref: usize,
    pub port_stable_id: u32,
    pub port_channel_index: u16,
    pub delay: u32,
}

pub(crate) struct SharedDelayCompNode {
    pub node: Shared<AtomicRefCell<DelayCompNode>>,
    pub active: bool,
}

impl SharedDelayCompNode {
    pub fn new(delay: u32, coll_handle: &basedrop::Handle) -> Self {
        Self {
            node: Shared::new(coll_handle, AtomicRefCell::new(DelayCompNode::new(delay))),
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
        self.node.borrow().delay()
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
            *slot = Some(SharedBuffer::_new_f32(
                self.audio_buffer_size as usize,
                DebugBufferID { buffer_type: DebugBufferType::Audio32, index: index as u32 },
                &self.coll_handle,
            ));

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
            *slot = Some(SharedBuffer::_new_f32(
                self.audio_buffer_size as usize,
                DebugBufferID {
                    buffer_type: DebugBufferType::IntermediaryAudio32,
                    index: index as u32,
                },
                &self.coll_handle,
            ));

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
            *slot = Some(SharedBuffer::_new(
                self.note_buffer_size,
                DebugBufferID { buffer_type: DebugBufferType::NoteBuffer, index: index as u32 },
                &self.coll_handle,
            ));

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
            *slot = Some(SharedBuffer::_new(
                self.automation_buffer_size,
                DebugBufferID {
                    buffer_type: DebugBufferType::ParamAutomation,
                    index: index as u32,
                },
                &self.coll_handle,
            ));

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
