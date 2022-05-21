use basedrop::Shared;
use fnv::FnvHashMap;
use std::cell::UnsafeCell;
use std::hash::Hash;

use super::plugin_host::PluginHost;

/// Used for debugging and verifying purposes.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum DebugBufferType {
    Audio32,
    Audio64,
}
impl std::fmt::Debug for DebugBufferType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DebugBufferType::Audio32 => write!(f, "A32"),
            DebugBufferType::Audio64 => write!(f, "A64"),
        }
    }
}

/// Used for debugging and verifying purposes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DebugBufferID {
    buffer_type: DebugBufferType,
    index: u32,
}

impl std::fmt::Debug for DebugBufferID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}-{}", self.buffer_type, self.index)
    }
}

pub(crate) struct SharedBuffer<T: Clone + Copy + Send + Default + 'static> {
    pub buffer: Shared<(UnsafeCell<Vec<T>>, DebugBufferID)>,
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
    Sum,
    DelayComp,
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
                PluginInstanceType::Sum => "Sum",
                PluginInstanceType::DelayComp => "Dly",
                PluginInstanceType::GraphInput => "GraphIn",
                PluginInstanceType::GraphOutput => "GraphOut",
            }
        )
    }
}

/// Used to uniquely identify a plugin instance and for debugging purposes.
pub struct PluginInstanceID {
    pub(crate) node_index: usize,
    pub(crate) format: PluginInstanceType,
    name: Option<Shared<String>>,
}

impl std::fmt::Debug for PluginInstanceID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.format {
            PluginInstanceType::Internal => {
                write!(f, "Int({})_{}", &**self.name.as_ref().unwrap(), self.node_index)
            }
            _ => {
                write!(f, "{:?}_{}", self.format, self.node_index)
            }
        }
    }
}

impl Clone for PluginInstanceID {
    fn clone(&self) -> Self {
        Self {
            node_index: self.node_index,
            format: self.format,
            name: self.name.as_ref().map(|n| Shared::clone(n)),
        }
    }
}

impl PartialEq for PluginInstanceID {
    fn eq(&self, other: &Self) -> bool {
        self.node_index.eq(&other.node_index)
    }
}

impl Eq for PluginInstanceID {}

impl Hash for PluginInstanceID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node_index.hash(state)
    }
}

struct SharedPluginHost {
    plugin: Shared<UnsafeCell<PluginHost>>,
}

impl Clone for SharedPluginHost {
    fn clone(&self) -> Self {
        Self { plugin: Shared::clone(&self.plugin) }
    }
}

pub(crate) struct SharedPool {
    pub plugins: FnvHashMap<PluginInstanceID, SharedPluginHost>,

    audio_buffers_f32: Vec<Option<SharedBuffer<f32>>>,
    audio_buffers_f64: Vec<Option<SharedBuffer<f64>>>,

    buffer_size: usize,

    coll_handle: basedrop::Handle,
}

impl SharedPool {
    pub fn new(buffer_size: usize, coll_handle: basedrop::Handle) -> Self {
        assert_ne!(buffer_size, 0);

        Self {
            plugins: FnvHashMap::default(),
            audio_buffers_f32: Vec::new(),
            audio_buffers_f64: Vec::new(),
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

        let slot = self.audio_buffers_f32.get_unchecked_mut(index);

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

            slot.unwrap().clone()
        }
    }

    pub fn audio_f64(&mut self, index: usize) -> SharedBuffer<f64> {
        if self.audio_buffers_f64.len() <= index {
            let n_new_slots = (index + 1) - self.audio_buffers_f64.len();
            for _ in 0..n_new_slots {
                self.audio_buffers_f64.push(None);
            }
        }

        let slot = self.audio_buffers_f64.get_unchecked_mut(index);

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

            slot.unwrap().clone()
        }
    }

    pub fn remove_excess_audio_buffers(&mut self, max_buffer_index: usize) {
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
    }
}
