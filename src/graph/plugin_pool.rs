use audio_graph::{DefaultPortType, Graph, NodeRef, PortRef};
use fnv::FnvHashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{cell::UnsafeCell, hash::Hash};

use basedrop::Shared;

use crate::host::{Host, HostInfo};
use crate::plugin::ext::audio_ports::AudioPortsExtension;
use crate::plugin::{PluginAudioThread, PluginMainThread, PluginSaveState};
use crate::plugin_scanner::PluginFormat;
use crate::ProcessStatus;

use super::schedule::delay_comp_node::DelayCompNode;
use super::PortID;

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

impl From<PluginFormat> for PluginInstanceType {
    fn from(p: PluginFormat) -> Self {
        match p {
            PluginFormat::Internal => PluginInstanceType::Internal,
            PluginFormat::Clap => PluginInstanceType::Clap,
        }
    }
}

/// Used to uniquely identify a plugin instance and for debugging purposes.
pub struct PluginInstanceID {
    pub(crate) node_id: NodeRef,
    pub(crate) format: PluginInstanceType,
    name: Option<Shared<String>>,
}

impl std::fmt::Debug for PluginInstanceID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let id: usize = self.node_id.into();
        match self.format {
            PluginInstanceType::Internal => {
                write!(f, "Int({})_{}", &**self.name.as_ref().unwrap(), id)
            }
            _ => {
                write!(f, "{:?}_{}", self.format, id)
            }
        }
    }
}

impl Clone for PluginInstanceID {
    fn clone(&self) -> Self {
        Self {
            node_id: self.node_id,
            format: self.format,
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

    save_state: Option<PluginSaveState>,
    audio_in_channel_refs: Vec<PortRef>,
    audio_out_channel_refs: Vec<PortRef>,
    audio_ports_ext: Option<AudioPortsExtension>,
    format: PluginFormat,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DelayCompKey {
    pub node_id: NodeRef,
    pub port_i: u16,
    pub delay: u32,
}

#[derive(Clone)]
pub(crate) struct SharedDelayCompNode {
    pub shared: Shared<UnsafeCell<DelayCompNode>>,
    pub active: bool,
}

impl SharedDelayCompNode {
    pub fn new(delay: u32, coll_handle: &basedrop::Handle) -> Self {
        Self {
            shared: Shared::new(coll_handle, UnsafeCell::new(DelayCompNode::new(delay))),
            active: true,
        }
    }

    pub fn delay(&self) -> u32 {
        // Safe because we are only borrowing this immutably.
        let delay_node = unsafe { &*self.shared.get() };
        delay_node.delay()
    }
}

pub(crate) struct PluginInstancePool {
    pub delay_comp_nodes: FnvHashMap<DelayCompKey, SharedDelayCompNode>,

    graph_plugins: Vec<Option<PluginInstance>>,
    free_graph_plugins: Vec<NodeRef>,

    host_info: Shared<HostInfo>,

    coll_handle: basedrop::Handle,

    num_plugins: usize,
}

impl PluginInstancePool {
    pub fn new(
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        num_graph_in_audio_channels: u16,
        num_graph_out_audio_channels: u16,
        coll_handle: basedrop::Handle,
        host_info: Shared<HostInfo>,
    ) -> (Self, PluginInstanceID, PluginInstanceID) {
        let mut new_self = Self {
            delay_comp_nodes: FnvHashMap::default(),
            graph_plugins: Vec::new(),
            free_graph_plugins: Vec::new(),
            host_info,
            coll_handle,
            num_plugins: 0,
        };

        // --- Add the graph input node to the graph --------------------------

        let graph_in_node_id = if let Some(node_id) = new_self.free_graph_plugins.pop() {
            node_id
        } else {
            new_self.graph_plugins.push(None);
            NodeRef::new(new_self.graph_plugins.len())
        };

        let graph_in_id = PluginInstanceID {
            node_id: graph_in_node_id,
            format: PluginInstanceType::GraphInput,
            name: None,
        };

        let graph_in_node_ref = abstract_graph.node(graph_in_id.clone());
        // If this isn't right then I did something wrong.
        assert_eq!(graph_in_node_ref, graph_in_id.node_id);

        let mut graph_in_channel_refs: Vec<PortRef> =
            Vec::with_capacity(usize::from(num_graph_in_audio_channels));
        for i in 0..num_graph_in_audio_channels {
            let port_ref = abstract_graph
                .port(graph_in_node_ref, DefaultPortType::Audio, PortID::AudioOut(i))
                .unwrap();
            graph_in_channel_refs.push(port_ref);
        }

        let node_i: usize = graph_in_id.node_id.into();
        new_self.graph_plugins[node_i] = Some(PluginInstance {
            main_thread: None,
            audio_thread: None,
            save_state: None,
            audio_in_channel_refs: Vec::new(),
            audio_out_channel_refs: graph_in_channel_refs,
            audio_ports_ext: None,
            format: PluginFormat::Internal,
        });

        new_self.num_plugins += 1;

        // --- Add the graph output node to the graph --------------------------

        let graph_out_node_id = if let Some(node_id) = new_self.free_graph_plugins.pop() {
            node_id
        } else {
            new_self.graph_plugins.push(None);
            NodeRef::new(new_self.graph_plugins.len())
        };

        let graph_out_id = PluginInstanceID {
            node_id: graph_out_node_id,
            format: PluginInstanceType::GraphOutput,
            name: None,
        };

        let graph_out_node_ref = abstract_graph.node(graph_out_id.clone());
        // If this isn't right then I did something wrong.
        assert_eq!(graph_out_node_ref, graph_out_id.node_id);

        let mut graph_out_channel_refs: Vec<PortRef> =
            Vec::with_capacity(usize::from(num_graph_out_audio_channels));
        for i in 0..num_graph_out_audio_channels {
            let port_ref = abstract_graph
                .port(graph_out_node_ref, DefaultPortType::Audio, PortID::AudioIn(i))
                .unwrap();
            graph_out_channel_refs.push(port_ref);
        }

        let node_i: usize = graph_out_id.node_id.into();
        new_self.graph_plugins[node_i] = Some(PluginInstance {
            main_thread: None,
            audio_thread: None,
            save_state: None,
            audio_in_channel_refs: graph_out_channel_refs,
            audio_out_channel_refs: Vec::new(),
            audio_ports_ext: None,
            format: PluginFormat::Internal,
        });

        new_self.num_plugins += 1;

        (new_self, graph_in_id, graph_out_id)
    }

    pub fn add_graph_plugin(
        &mut self,
        plugin: Option<Box<dyn PluginMainThread>>,
        mut save_state: PluginSaveState,
        debug_name: Shared<String>,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        activate: bool,
    ) -> PluginInstanceID {
        let node_id = if let Some(node_id) = self.free_graph_plugins.pop() {
            node_id
        } else {
            self.graph_plugins.push(None);
            NodeRef::new(self.graph_plugins.len())
        };

        let id = PluginInstanceID {
            node_id,
            format: save_state.key.format.into(),
            name: Some(debug_name),
        };

        let node_ref = abstract_graph.node(id.clone());
        // If this isn't right then I did something wrong.
        assert_eq!(node_ref, id.node_id);

        let channel = Shared::new(
            &self.coll_handle,
            PluginInstanceChannel {
                restart_requested: AtomicBool::new(false),
                process_requested: AtomicBool::new(false),
                callback_requested: AtomicBool::new(false),
                active: AtomicBool::new(false),
            },
        );

        let (main_thread, audio_ports_ext) = if let Some(plugin) = plugin {
            let host = Host {
                info: Shared::clone(&self.host_info.clone()),
                current_plugin_channel: Shared::clone(&channel),
            };

            let audio_ports_ext = plugin.audio_ports_extension(&host);
            let (num_audio_in, num_audio_out) = audio_ports_ext.total_in_out_channels();

            save_state.audio_in_out_channels = (num_audio_in as u16, num_audio_out as u16);

            (
                Some(PluginMainThreadInstance { plugin, channel, id: id.clone() }),
                Some(audio_ports_ext),
            )
        } else {
            (None, None)
        };

        save_state.activated = false;

        let mut audio_in_channel_refs: Vec<PortRef> =
            Vec::with_capacity(usize::from(save_state.audio_in_out_channels.0));
        let mut audio_out_channel_refs: Vec<PortRef> =
            Vec::with_capacity(usize::from(save_state.audio_in_out_channels.1));
        for i in 0..save_state.audio_in_out_channels.0 {
            let port_ref =
                abstract_graph.port(node_id, DefaultPortType::Audio, PortID::AudioIn(i)).unwrap();
            audio_in_channel_refs.push(port_ref);
        }
        for i in 0..save_state.audio_in_out_channels.1 {
            let port_ref =
                abstract_graph.port(node_id, DefaultPortType::Audio, PortID::AudioOut(i)).unwrap();
            audio_out_channel_refs.push(port_ref);
        }

        let format = save_state.key.format;

        let node_i: usize = node_id.into();
        self.graph_plugins[node_i] = Some(PluginInstance {
            main_thread,
            audio_thread: None,
            save_state: Some(save_state),
            audio_in_channel_refs,
            audio_out_channel_refs,
            audio_ports_ext,
            format,
        });

        self.num_plugins += 1;

        if activate {
            // TODO
        }

        log::debug!("Added plugin instance {:?} to audio graph", &id);

        id
    }

    pub fn remove_graph_plugin(
        &mut self,
        id: &PluginInstanceID,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    ) {
        // Deactivate the plugin first.
        self.deactivate_plugin_instance(id);

        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = self.graph_plugins[node_i].take() {
            // Drop the plugin instance here.
            let _ = plugin_instance;

            // Re-use this node ID for the next new plugin.
            self.free_graph_plugins.push(id.node_id);

            self.num_plugins -= 1;

            abstract_graph.delete_node(id.node_id).unwrap();

            log::debug!("Removed plugin instance {:?} from audio graph", id);
        } else {
            log::warn!("Could not remove plugin instance {:?} from audio graph: Plugin was not found in the graph", id);
        }
    }

    // TODO: custom error type
    pub fn activate_plugin_instance(
        &mut self,
        id: &PluginInstanceID,
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
                let host = Host {
                    info: Shared::clone(&self.host_info.clone()),
                    current_plugin_channel: Shared::clone(&plugin_main_thread.channel),
                };

                match plugin_main_thread.plugin.activate(
                    sample_rate,
                    min_frames,
                    max_frames,
                    &host,
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

                        plugin_instance.save_state.as_mut().unwrap().activated = true;

                        log::trace!("Successfully activated plugin instance {:?}", &id);

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

    pub fn deactivate_plugin_instance(&mut self, id: &PluginInstanceID) {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &mut self.graph_plugins[node_i] {
            if let Some(plugin_main_thread) = &mut plugin_instance.main_thread {
                if !plugin_main_thread.channel.active.load(Ordering::Relaxed) {
                    // Plugin is already inactive.
                    return;
                }

                // Prepare the host handle to accept requests from the plugin.
                let host = Host {
                    info: Shared::clone(&self.host_info.clone()),
                    current_plugin_channel: Shared::clone(&plugin_main_thread.channel),
                };

                plugin_main_thread.plugin.deactivate(&host);

                plugin_main_thread.channel.active.store(false, Ordering::Relaxed);

                plugin_instance.save_state.as_mut().unwrap().activated = false;
            }

            if let Some(plugin_audio_thread) = plugin_instance.audio_thread.take() {
                // Drop the audio thread counterpart of the plugin here. (Note this will not
                // drop the actual instance until the schedule on the audio thread also drops
                // its pointer.)
                let _ = plugin_audio_thread;
            }
        }
    }

    #[inline]
    pub fn is_plugin_loaded(&self, id: &PluginInstanceID) -> Result<bool, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            Ok(plugin_instance.main_thread.is_some())
        } else {
            Err(())
        }
    }

    #[inline]
    pub fn is_plugin_active(&self, id: &PluginInstanceID) -> Result<bool, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            if let Some(plugin_main_thread) = plugin_instance.main_thread.as_ref() {
                Ok(plugin_main_thread.channel.active.load(Ordering::Relaxed))
            } else {
                Ok(false)
            }
        } else {
            Err(())
        }
    }

    #[inline]
    pub fn get_audio_ports_ext(
        &self,
        id: &PluginInstanceID,
    ) -> Result<&Option<AudioPortsExtension>, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            Ok(&plugin_instance.audio_ports_ext)
        } else {
            Err(())
        }
    }

    #[inline]
    pub fn get_audio_in_channel_refs(&self, id: &PluginInstanceID) -> Result<&[PortRef], ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            Ok(&plugin_instance.audio_in_channel_refs)
        } else {
            Err(())
        }
    }

    #[inline]
    pub fn get_audio_out_channel_refs(&self, id: &PluginInstanceID) -> Result<&[PortRef], ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            Ok(&plugin_instance.audio_out_channel_refs)
        } else {
            Err(())
        }
    }

    #[inline]
    pub fn get_plugin_format(&self, id: &PluginInstanceID) -> Result<PluginFormat, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            Ok(plugin_instance.format)
        } else {
            Err(())
        }
    }

    pub fn num_plugins(&self) -> usize {
        self.num_plugins
    }

    #[inline]
    pub fn get_graph_plugin_audio_thread(
        &self,
        id: &PluginInstanceID,
    ) -> Result<Option<&SharedPluginAudioThreadInstance>, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = self.graph_plugins[node_i].as_ref() {
            Ok(plugin_instance.audio_thread.as_ref())
        } else {
            Err(())
        }
    }

    pub fn iter_plugin_ids(&self) -> impl Iterator<Item = NodeRef> + '_ {
        self.graph_plugins
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().map(|_| NodeRef::new(i)))
    }

    pub fn get_graph_plugin_save_state(&self, node_id: NodeRef) -> Result<&PluginSaveState, ()> {
        let node_i: usize = node_id.into();
        if let Some(plugin) = self.graph_plugins[node_i].as_ref() {
            if let Some(save_state) = &plugin.save_state {
                Ok(save_state)
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }

    pub fn host_info(&self) -> &Shared<HostInfo> {
        &self.host_info
    }
}
