use audio_graph::NodeRef;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{cell::UnsafeCell, hash::Hash};

use basedrop::Shared;

use crate::host::Host;
use crate::plugin::{PluginAudioThread, PluginMainThread};
use crate::plugin_scanner::PluginType;
use crate::ProcessStatus;

/// Used for debugging and verifying purposes.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginInstanceType {
    Internal,
    Clap,
    Sum,
    DelayComp,
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
            }
        )
    }
}

impl From<PluginType> for PluginInstanceType {
    fn from(p: PluginType) -> Self {
        match p {
            PluginType::Internal => PluginInstanceType::Internal,
            PluginType::Clap => PluginInstanceType::Clap,
        }
    }
}

/// Used to uniquely identify a plugin instance and for debugging purposes.
pub struct PluginInstanceID {
    pub(crate) node_id: NodeRef,
    plugin_type: PluginInstanceType,
    name: Option<Shared<String>>,
}

impl std::fmt::Debug for PluginInstanceID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let id: usize = self.node_id.into();
        match self.plugin_type {
            PluginInstanceType::Internal => {
                write!(f, "Int({})_{}", &**self.name.as_ref().unwrap(), id)
            }
            _ => {
                write!(f, "{:?}_{}", self.plugin_type, id)
            }
        }
    }
}

impl Clone for PluginInstanceID {
    fn clone(&self) -> Self {
        Self {
            node_id: self.node_id,
            plugin_type: self.plugin_type,
            name: self.name.as_ref().map(|n| Shared::clone(n)),
        }
    }
}

impl PartialEq for PluginInstanceID {
    fn eq(&self, other: &Self) -> bool {
        self.node_id.eq(&other.node_id)
    }
}

impl Eq for PluginInstanceID {}

impl Hash for PluginInstanceID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node_id.hash(state)
    }
}

/// Used to sync plugin state between the main thread and audio thread counterparts.
pub(crate) struct PluginInstanceChannel {
    pub restart_requested: AtomicBool,
    pub process_requested: AtomicBool,
    pub callback_requested: AtomicBool,

    active: AtomicBool,
    // TODO: parameter stuff
}

pub(crate) struct PluginMainThreadInstance {
    pub plugin: Box<dyn PluginMainThread>,
    pub channel: Shared<PluginInstanceChannel>,

    id: PluginInstanceID,
}

pub(crate) struct PluginAudioThreadInstance {
    pub plugin: Box<dyn PluginAudioThread>,
    pub channel: Shared<PluginInstanceChannel>,
    pub last_process_status: ProcessStatus,
}

#[derive(Clone)]
pub(crate) struct SharedPluginAudioThreadInstance {
    shared: Shared<UnsafeCell<PluginAudioThreadInstance>>,
    id: PluginInstanceID,
}

impl SharedPluginAudioThreadInstance {
    fn new(
        plugin: Box<dyn PluginAudioThread>,
        id: PluginInstanceID,
        channel: Shared<PluginInstanceChannel>,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        Self {
            shared: Shared::new(
                &coll_handle,
                UnsafeCell::new(PluginAudioThreadInstance {
                    plugin,
                    channel,
                    last_process_status: ProcessStatus::Continue,
                }),
            ),
            id,
        }
    }

    pub fn id(&self) -> &PluginInstanceID {
        &self.id
    }

    #[inline]
    pub unsafe fn borrow_mut(&self) -> &mut PluginAudioThreadInstance {
        &mut (*self.shared.get())
    }
}

struct PluginInstance {
    main_thread: Option<PluginMainThreadInstance>,
    audio_thread: Option<SharedPluginAudioThreadInstance>,
}

pub(crate) struct PluginInstancePool {
    graph_plugins: Vec<Option<PluginInstance>>,

    free_graph_plugins: Vec<NodeRef>,

    coll_handle: basedrop::Handle,
}

impl PluginInstancePool {
    pub fn new(coll_handle: basedrop::Handle) -> Self {
        Self { graph_plugins: Vec::new(), free_graph_plugins: Vec::new(), coll_handle }
    }

    pub fn add_graph_plugin(
        &mut self,
        plugin: Box<dyn PluginMainThread>,
        plugin_type: PluginType,
        debug_name: Shared<String>,
    ) -> PluginInstanceID {
        let node_id = if let Some(node_id) = self.free_graph_plugins.pop() {
            node_id
        } else {
            self.graph_plugins.push(None);
            NodeRef::new(self.graph_plugins.len())
        };

        let id =
            PluginInstanceID { node_id, plugin_type: plugin_type.into(), name: Some(debug_name) };

        let channel = Shared::new(
            &self.coll_handle,
            PluginInstanceChannel {
                restart_requested: AtomicBool::new(false),
                process_requested: AtomicBool::new(false),
                callback_requested: AtomicBool::new(false),
                active: AtomicBool::new(false),
            },
        );

        let main_thread = Some(PluginMainThreadInstance { plugin, channel, id: id.clone() });

        let node_i: usize = node_id.into();
        self.graph_plugins[node_i] = Some(PluginInstance { main_thread, audio_thread: None });

        id
    }

    /*
    /// Only used for the special `Sum` and `DelayComp` types.
    pub fn new_plugin_instance_from_audio(
        &mut self,
        plugin: Box<dyn PluginAudioThread>,
        plugin_type: PluginInstanceType,
    ) -> PluginInstanceID {
        let id = PluginInstanceID { id: self.id_accumulator, plugin_type, name: None };
        self.id_accumulator += 1;

        let channel = Shared::new(
            &self.coll_handle,
            PluginInstanceChannel {
                restart_requested: AtomicBool::new(false),
                process_requested: AtomicBool::new(false),
                callback_requested: AtomicBool::new(false),
                active: AtomicBool::new(true),
            },
        );

        let audio_thread =
            SharedPluginAudioThreadInstance::new(plugin, id.clone(), channel, &self.coll_handle);

        let _ = self.plugin_instances.insert(
            id.clone(),
            PluginInstance { main_thread: None, audio_thread: Some(audio_thread) },
        );

        id
    }
    */

    pub fn remove_graph_plugin(&mut self, id: &PluginInstanceID, host: &mut Host) {
        // Deactivate the plugin first.
        self.deactivate_plugin_instance(id, host);

        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = self.graph_plugins[node_i].take() {
            // Drop the plugin instance here.
            let _ = plugin_instance;

            // Re-use this node ID for the next new plugin.
            self.free_graph_plugins.push(id.node_id);
        }
    }

    // TODO: custom error type
    pub fn activate_plugin_instance(
        &mut self,
        id: &PluginInstanceID,
        host: &mut Host,
        sample_rate: f64,
        min_frames: usize,
        max_frames: usize,
    ) -> Result<SharedPluginAudioThreadInstance, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &mut self.graph_plugins[node_i] {
            if let Some(plugin_main_thread) = &mut plugin_instance.main_thread {
                if plugin_main_thread.channel.active.load(Ordering::Relaxed) {
                    log::warn!("Tried to activate plugin that is already active");

                    // Cannot activate plugin that is already active.
                    return Err(());
                }

                // Prepare the host handle to accept requests from the plugin.
                host.current_plugin_channel = Shared::clone(&plugin_main_thread.channel);

                match plugin_main_thread.plugin.activate(
                    sample_rate,
                    min_frames,
                    max_frames,
                    host,
                    &self.coll_handle,
                ) {
                    Ok(plugin_audio_thread) => {
                        let new_audio_thread = SharedPluginAudioThreadInstance::new(
                            plugin_audio_thread,
                            plugin_main_thread.id.clone(),
                            Shared::clone(&plugin_main_thread.channel),
                            &self.coll_handle,
                        );

                        plugin_main_thread.channel.active.store(true, Ordering::Relaxed);

                        // Store the plugin audio thread instance for future schedules.
                        plugin_instance.audio_thread = Some(new_audio_thread.clone());

                        return Ok(new_audio_thread);
                    }
                    Err(e) => {
                        log::error!(
                            "Error while activating plugin instance {:?}: {}",
                            plugin_main_thread.id,
                            e
                        );
                        return Err(());
                    }
                }
            }
        }

        return Err(());
    }

    pub fn deactivate_plugin_instance(&mut self, id: &PluginInstanceID, host: &mut Host) {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &mut self.graph_plugins[node_i] {
            if let Some(plugin_main_thread) = &mut plugin_instance.main_thread {
                if !plugin_main_thread.channel.active.load(Ordering::Relaxed) {
                    // Plugin is already inactive.
                    return;
                }

                // Prepare the host handle to accept requests from the plugin.
                host.current_plugin_channel = Shared::clone(&plugin_main_thread.channel);

                plugin_main_thread.plugin.deactivate(host);

                plugin_main_thread.channel.active.store(false, Ordering::Relaxed);
            }

            if let Some(plugin_audio_thread) = plugin_instance.audio_thread.take() {
                // Drop the audio thread counterpart of the plugin here. (Note this will not
                // drop the actual instance until the schedule on the audio thread also drops
                // its pointer.)
                let _ = plugin_audio_thread;
            }
        }
    }

    pub fn get_graph_plugin_audio_thread(
        &self,
        node_id: NodeRef,
    ) -> Option<&SharedPluginAudioThreadInstance> {
        let node_i: usize = node_id.into();
        self.graph_plugins[node_i].as_ref().unwrap().audio_thread.as_ref()
    }
}
