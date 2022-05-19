use audio_graph::{DefaultPortType, Graph, NodeRef, PortRef};
use basedrop::Shared;
use fnv::FnvHashMap;
use rusty_daw_core::SampleRate;
use smallvec::SmallVec;
use std::error::Error;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{cell::UnsafeCell, hash::Hash};

use crate::graph::PluginActivationStatus;
use crate::host_request::{HostInfo, HostRequest};
use crate::plugin::ext::audio_ports::AudioPortsExtension;
use crate::plugin::{PluginAudioThread, PluginMainThread, PluginSaveState};
use crate::plugin_scanner::PluginFormat;

use super::schedule::delay_comp_node::DelayCompNode;
use super::{PluginEdges, PortID};

// TODO: Clean this up.

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
    pub deactivation_requested: AtomicBool,
    pub plugin_state: SharedPluginState,
    // TODO: parameter stuff
}

impl PluginInstanceChannel {
    pub fn new() -> Self {
        Self {
            restart_requested: AtomicBool::new(false),
            process_requested: AtomicBool::new(false),
            callback_requested: AtomicBool::new(false),
            deactivation_requested: AtomicBool::new(false),
            plugin_state: SharedPluginState::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum PluginState {
    /// The plugin is inactive, only the main thread uses it
    Inactive = 0,

    /// Activation failed
    InactiveWithError = 1,

    /// The plugin is active and sleeping, the audio engine can call start_processing()
    ActiveAndSleeping = 2,

    /// The plugin is processing
    ActiveAndProcessing = 3,

    /// The plugin is processing, but will be put to sleep the next time all input buffers
    /// are silent.
    ActiveAndWaitingForQuiet = 4,

    /// The plugin is processing, but will be put to sleep at the end of the plugin's tail.
    ActiveAndWaitingForTail = 5,

    /// The plugin did process but is in error
    ActiveWithError = 6,

    /// The plugin is not used anymore by the audio engine and can be deactivated on the main
    /// thread
    ActiveAndReadyToDeactivate = 7,
}

impl PluginState {
    pub fn is_active(&self) -> bool {
        match self {
            PluginState::Inactive | PluginState::InactiveWithError => false,
            _ => true,
        }
    }

    pub fn is_processing(&self) -> bool {
        match self {
            PluginState::ActiveAndProcessing
            | PluginState::ActiveAndWaitingForQuiet
            | PluginState::ActiveAndWaitingForTail => true,
            _ => false,
        }
    }

    pub fn is_sleeping(&self) -> bool {
        *self == PluginState::ActiveAndSleeping
    }
}

#[derive(Debug)]
pub(crate) struct SharedPluginState {
    state: AtomicU32,
}

impl SharedPluginState {
    pub fn new() -> Self {
        Self { state: AtomicU32::new(0) }
    }

    #[inline]
    pub fn get(&self) -> PluginState {
        let s = self.state.load(Ordering::Relaxed);

        // Safe because we set `#[repr(u32)]` on this enum, and this AtomicU32
        // can never be set to a value that is out of range.
        unsafe { *(&s as *const u32 as *const PluginState) }
    }

    #[inline]
    pub fn set(&self, state: PluginState) {
        // Safe because we set `#[repr(u32)]` on this enum.
        let s = unsafe { *(&state as *const PluginState as *const u32) };

        self.state.store(s, Ordering::Relaxed);
    }
}

pub(crate) enum PluginMainThreadType {
    Internal(Box<dyn PluginMainThread>),
    #[cfg(feature = "clap-host")]
    Clap(crate::clap::plugin::ClapPluginMainThread),
}

pub(crate) struct PluginAudioThreadInstance {
    pub plugin: UnsafeCell<Box<dyn PluginAudioThread>>,
    pub host_request: HostRequest,
}

#[derive(Clone)]
pub(crate) struct SharedPluginAudioThreadInstance {
    pub shared: Shared<PluginAudioThreadInstance>,
    id: PluginInstanceID,
}

impl SharedPluginAudioThreadInstance {
    fn new(
        plugin: Box<dyn PluginAudioThread>,
        id: PluginInstanceID,
        host_request: HostRequest,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        Self {
            shared: Shared::new(
                &coll_handle,
                PluginAudioThreadInstance { plugin: UnsafeCell::new(plugin), host_request },
            ),
            id,
        }
    }

    pub fn id(&self) -> &PluginInstanceID {
        &self.id
    }
}

pub(crate) enum PluginAudioThreadType {
    Internal(SharedPluginAudioThreadInstance),
    #[cfg(feature = "clap-host")]
    Clap(crate::clap::plugin::ClapPluginAudioThread),
}

struct LoadedPluginInstance {
    main_thread: PluginMainThreadType,
    audio_thread: Option<PluginAudioThreadType>,
    host_request: HostRequest,
    channel: Shared<PluginInstanceChannel>,
    save_state: PluginSaveState,
    audio_ports_ext: AudioPortsExtension,
}

struct PluginInstance {
    loaded: Option<LoadedPluginInstance>,

    audio_in_channel_refs: Vec<PortRef>,
    audio_out_channel_refs: Vec<PortRef>,
    format: PluginFormat,

    remove_requested: bool,

    id: PluginInstanceID,
}

impl PluginInstance {
    pub fn new(
        plugin_and_host_request: Option<(PluginMainThreadType, HostRequest)>,
        mut save_state: PluginSaveState,
        id: PluginInstanceID,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        activate: bool,
    ) -> Self {
        if activate {
            save_state.activation_requested = true
        }
        let format = save_state.key.format;

        let loaded = if let Some((plugin, host_request)) = plugin_and_host_request {
            match &plugin {
                PluginMainThreadType::Internal(p) => match p.audio_ports_extension(&host_request) {
                    Ok(audio_ports_ext) => {
                        let num_audio_in = audio_ports_ext.total_in_channels();
                        let num_audio_out = audio_ports_ext.total_out_channels();

                        save_state.audio_in_out_channels =
                            (num_audio_in as u16, num_audio_out as u16);

                        Some(LoadedPluginInstance {
                            main_thread: PluginMainThreadType::Internal(*p),
                            audio_thread: None,
                            host_request,
                            channel: Shared::clone(&host_request.plugin_channel),
                            save_state,
                            audio_ports_ext,
                        })
                    }
                    Err(e) => {
                        log::error!("Failed to load plugin instance, plugin return error while getting the audio ports extension: {}", e);
                        None
                    }
                },
                #[cfg(feature = "clap-host")]
                PluginMainThreadType::Clap(p) => match p.audio_ports_extension() {
                    Ok(audio_ports_ext) => {
                        let num_audio_in = audio_ports_ext.total_in_channels();
                        let num_audio_out = audio_ports_ext.total_out_channels();

                        save_state.audio_in_out_channels =
                            (num_audio_in as u16, num_audio_out as u16);

                        Some(LoadedPluginInstance {
                            main_thread: PluginMainThreadType::Clap(*p),
                            audio_thread: None,
                            host_request,
                            channel: Shared::clone(&host_request.plugin_channel),
                            save_state,
                            audio_ports_ext,
                        })
                    }
                    Err(e) => {
                        log::error!("Failed to load plugin instance, plugin return error while getting the audio ports extension: {}", e);
                        None
                    }
                },
            }
        } else {
            None
        };

        let mut audio_in_channel_refs: Vec<PortRef> =
            Vec::with_capacity(usize::from(save_state.audio_in_out_channels.0));
        let mut audio_out_channel_refs: Vec<PortRef> =
            Vec::with_capacity(usize::from(save_state.audio_in_out_channels.1));
        let (audio_in_channels, audio_out_channels) = if let Some(plugin) = &loaded {
            let audio_in_channels = plugin.audio_ports_ext.total_in_channels();
            let audio_out_channels = plugin.audio_ports_ext.total_out_channels();
            save_state.audio_in_out_channels =
                (audio_in_channels as u16, audio_out_channels as u16);
            (audio_in_channels as u16, audio_out_channels as u16)
        } else {
            // If the plugin failed to load, try to retrieve the number of channels
            // from the save state
            save_state.audio_in_out_channels
        };
        for i in 0..audio_in_channels {
            let port_ref = abstract_graph
                .port(id.node_id, DefaultPortType::Audio, PortID::AudioIn(i))
                .unwrap();
            audio_in_channel_refs.push(port_ref);
        }
        for i in 0..audio_out_channels {
            let port_ref = abstract_graph
                .port(id.node_id, DefaultPortType::Audio, PortID::AudioOut(i))
                .unwrap();
            audio_out_channel_refs.push(port_ref);
        }

        PluginInstance {
            loaded,
            audio_in_channel_refs,
            audio_out_channel_refs,
            format,
            id,
            remove_requested: false,
        }
    }

    pub fn can_activate(&self) -> bool {
        if let Some(plugin) = &self.loaded {
            if plugin.channel.plugin_state.get().is_active() {
                return false;
            }
            if plugin.channel.restart_requested.load(Ordering::Relaxed) {
                return false;
            }
            if self.remove_requested {
                return false;
            }
            true
        } else {
            false
        }
    }

    pub fn can_remove(&self) -> bool {
        if let Some(plugin) = &self.loaded {
            if plugin.channel.plugin_state.get().is_active() {
                return false;
            }
            if plugin.channel.restart_requested.load(Ordering::Relaxed) {
                return false;
            }
        }
        true
    }

    pub fn request_remove(&mut self) {
        self.remove_requested = true;
        self.deactivate();
    }

    pub fn activate(
        &mut self,
        sample_rate: SampleRate,
        min_block_size: usize,
        max_block_size: usize,
        check_for_port_change: bool,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        coll_handle: &basedrop::Handle,
    ) -> PluginActivationStatus {
        assert!(self.can_activate());

        let plugin = self.loaded.as_mut().unwrap();

        plugin.save_state.activation_requested = true;

        log::trace!("Activating plugin instance {:?}", &self.id);

        let status = if check_for_port_change {
            let new_audio_ports_ext = match &plugin.main_thread {
                PluginMainThreadType::Internal(p) => {
                    match p.audio_ports_extension(&plugin.host_request) {
                        Ok(ext) => ext,
                        Err(e) => {
                            return PluginActivationStatus::Error(PluginActivationError {
                                plugin_id: self.id.clone(),
                                error: e,
                            });
                        }
                    }
                }
                #[cfg(feature = "clap-host")]
                PluginMainThreadType::Clap(p) => match p.audio_ports_extension() {
                    Ok(ext) => ext,
                    Err(e) => {
                        return PluginActivationStatus::Error(PluginActivationError {
                            plugin_id: self.id.clone(),
                            error: e,
                        });
                    }
                },
            };

            if new_audio_ports_ext != plugin.audio_ports_ext {
                plugin.audio_ports_ext = new_audio_ports_ext;

                // Make sure the abstract graph has the updated number of ports.

                let node_id = self.id.node_id;

                let audio_in_channels = plugin.audio_ports_ext.total_in_channels();
                let audio_out_channels = plugin.audio_ports_ext.total_out_channels();

                if audio_in_channels > self.audio_in_channel_refs.len() {
                    let len = self.audio_in_channel_refs.len() as u16;
                    for i in len..audio_in_channels as u16 {
                        let port_ref = abstract_graph
                            .port(node_id, DefaultPortType::Audio, PortID::AudioIn(i))
                            .unwrap();
                        self.audio_in_channel_refs.push(port_ref);
                    }
                } else if audio_in_channels < self.audio_in_channel_refs.len() {
                    let n_to_remove = self.audio_in_channel_refs.len() - audio_in_channels;
                    for _ in 0..n_to_remove {
                        let port_ref = self.audio_in_channel_refs.pop().unwrap();
                        if let Err(e) = abstract_graph.delete_port(port_ref) {
                            log::error!(
                                "Unexpected error while removing port from abstract graph: {}",
                                e
                            );
                        }
                    }
                }

                if audio_out_channels > self.audio_out_channel_refs.len() {
                    let len = self.audio_in_channel_refs.len() as u16;
                    for i in len..audio_out_channels as u16 {
                        let port_ref = abstract_graph
                            .port(node_id, DefaultPortType::Audio, PortID::AudioOut(i))
                            .unwrap();
                        self.audio_out_channel_refs.push(port_ref);
                    }
                } else if audio_out_channels < self.audio_out_channel_refs.len() {
                    let n_to_remove = self.audio_out_channel_refs.len() - audio_out_channels;
                    for _ in 0..n_to_remove {
                        let port_ref = self.audio_out_channel_refs.pop().unwrap();
                        if let Err(e) = abstract_graph.delete_port(port_ref) {
                            log::error!(
                                "Unexpected error while removing port from abstract graph: {}",
                                e
                            );
                        }
                    }
                }

                plugin.save_state.audio_in_out_channels =
                    (audio_in_channels as u16, audio_out_channels as u16);

                PluginActivationStatus::ActivatedWithNewPortConfig {
                    audio_ports: plugin.audio_ports_ext.clone(),
                }
            } else {
                PluginActivationStatus::Activated
            }
        } else {
            PluginActivationStatus::Activated
        };

        match &mut plugin.main_thread {
            PluginMainThreadType::Internal(p) => match p.activate(
                sample_rate,
                min_block_size,
                max_block_size,
                &plugin.host_request,
                coll_handle,
            ) {
                Ok(plugin_audio_thread) => {
                    plugin.channel.deactivation_requested.store(false, Ordering::Relaxed);

                    plugin.channel.process_requested.store(true, Ordering::Relaxed);
                    plugin.channel.plugin_state.set(PluginState::ActiveAndSleeping);

                    plugin.audio_thread = Some(PluginAudioThreadType::Internal(
                        SharedPluginAudioThreadInstance::new(
                            plugin_audio_thread,
                            self.id.clone(),
                            plugin.host_request.clone(),
                            coll_handle,
                        ),
                    ));

                    log::debug!("Successfully activated plugin instance {:?}", &self.id);
                }
                Err(e) => {
                    plugin.channel.plugin_state.set(PluginState::InactiveWithError);

                    return PluginActivationStatus::Error(PluginActivationError {
                        plugin_id: self.id.clone(),
                        error: e,
                    });
                }
            },
            #[cfg(feature = "clap-host")]
            PluginMainThreadType::Clap(p) => {
                match p.activate(sample_rate, min_block_size, max_block_size) {
                    Ok(plugin_audio_thread) => {
                        plugin.channel.deactivation_requested.store(false, Ordering::Relaxed);

                        plugin.channel.process_requested.store(true, Ordering::Relaxed);
                        plugin.channel.plugin_state.set(PluginState::ActiveAndSleeping);

                        plugin.audio_thread =
                            Some(PluginAudioThreadType::Clap(plugin_audio_thread));

                        log::debug!("Successfully activated plugin instance {:?}", &self.id);
                    }
                    Err(e) => {
                        plugin.channel.plugin_state.set(PluginState::InactiveWithError);

                        return PluginActivationStatus::Error(PluginActivationError {
                            plugin_id: self.id.clone(),
                            error: e,
                        });
                    }
                }
            }
        }

        status
    }

    pub fn deactivate(&mut self) {
        if let Some(plugin) = &mut self.loaded {
            let state = plugin.channel.plugin_state.get();

            if !state.is_active() {
                return;
            }

            if state.is_processing() || state.is_sleeping() {
                // Wait until the audio thread is done using the plugin to deactivate.
                plugin.channel.deactivation_requested.store(true, Ordering::Relaxed);
            } else {
                // Safe to deactivate now.
                match &mut plugin.main_thread {
                    PluginMainThreadType::Internal(p) => p.deactivate(&plugin.host_request),
                    PluginMainThreadType::Clap(p) => p.deactivate(),
                }

                plugin.audio_thread = None;

                plugin.channel.plugin_state.set(PluginState::Inactive);
            }
        }
    }

    pub fn on_main_thread(
        &mut self,
        sample_rate: SampleRate,
        min_block_size: usize,
        max_block_size: usize,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        coll_handle: &basedrop::Handle,
    ) -> Option<MainThreadStatus> {
        if self.loaded.is_none() {
            return None;
        }

        let plugin = self.loaded.as_mut().unwrap();

        let did_deactivate = if plugin.channel.deactivation_requested.load(Ordering::Relaxed) {
            let state = plugin.channel.plugin_state.get();

            if !(state.is_processing() || state.is_sleeping()) {
                // The audio thread has finished using the plugin, it is safe to deactivate now.
                match &mut plugin.main_thread {
                    PluginMainThreadType::Internal(p) => p.deactivate(&plugin.host_request),
                    #[cfg(feature = "clap-host")]
                    PluginMainThreadType::Clap(p) => p.deactivate(),
                }

                plugin.audio_thread = None;

                plugin.channel.plugin_state.set(PluginState::Inactive);

                plugin.channel.deactivation_requested.store(false, Ordering::Relaxed);

                true
            } else {
                false
            }
        } else {
            false
        };

        if plugin.channel.callback_requested.load(Ordering::Relaxed) {
            log::trace!("Got callback request from plugin {:?}", &self.id);

            plugin.channel.callback_requested.store(false, Ordering::Relaxed);

            match &mut plugin.main_thread {
                PluginMainThreadType::Internal(p) => p.on_main_thread(&plugin.host_request),
                #[cfg(feature = "clap-host")]
                PluginMainThreadType::Clap(p) => p.on_main_thread(),
            }
        }

        if plugin.channel.restart_requested.load(Ordering::Relaxed) && !self.remove_requested {
            log::trace!("Got restart request from plugin {:?}", &self.id);

            let state = plugin.channel.plugin_state.get();

            if state.is_active() {
                // Wait for the plugin to deactivate before reactivating.
                self.deactivate();
            }

            if !state.is_active() {
                // Safe to restart now.
                plugin.channel.restart_requested.store(false, Ordering::Relaxed);

                let res = self.activate(
                    sample_rate,
                    min_block_size,
                    max_block_size,
                    true,
                    abstract_graph,
                    coll_handle,
                );

                return Some(MainThreadStatus::Activated(res));
            }
        }

        if self.remove_requested {
            if self.can_remove() {
                return Some(MainThreadStatus::Removed);
            }
        }

        if did_deactivate {
            Some(MainThreadStatus::Deactivated)
        } else {
            None
        }
    }
}

pub enum MainThreadStatus {
    Deactivated,
    Activated(PluginActivationStatus),
    Removed,
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
    plugins_to_remove: Vec<PluginInstanceID>,

    host_info: Shared<HostInfo>,

    coll_handle: basedrop::Handle,

    num_plugins: usize,

    sample_rate: SampleRate,
    min_frames: usize,
    max_frames: usize,
}

impl PluginInstancePool {
    pub fn new(
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        num_graph_in_audio_channels: u16,
        num_graph_out_audio_channels: u16,
        coll_handle: basedrop::Handle,
        host_info: Shared<HostInfo>,
        sample_rate: SampleRate,
        min_frames: usize,
        max_frames: usize,
    ) -> (Self, PluginInstanceID, PluginInstanceID) {
        let mut new_self = Self {
            delay_comp_nodes: FnvHashMap::default(),
            graph_plugins: Vec::new(),
            free_graph_plugins: Vec::new(),
            plugins_to_remove: Vec::new(),
            host_info,
            coll_handle,
            num_plugins: 0,
            sample_rate,
            min_frames,
            max_frames,
        };

        // --- Add the graph input node to the graph --------------------------

        let graph_in_node_id = if let Some(node_id) = new_self.free_graph_plugins.pop() {
            node_id
        } else {
            new_self.graph_plugins.push(None);
            NodeRef::new(new_self.graph_plugins.len() - 1)
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
            loaded: None,
            audio_in_channel_refs: Vec::new(),
            audio_out_channel_refs: graph_in_channel_refs,
            format: PluginFormat::Internal,
            id: graph_in_id.clone(),
            remove_requested: false,
        });

        new_self.num_plugins += 1;

        // --- Add the graph output node to the graph --------------------------

        let graph_out_node_id = if let Some(node_id) = new_self.free_graph_plugins.pop() {
            node_id
        } else {
            new_self.graph_plugins.push(None);
            NodeRef::new(new_self.graph_plugins.len() - 1)
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
            loaded: None,
            audio_in_channel_refs: graph_out_channel_refs,
            audio_out_channel_refs: Vec::new(),
            format: PluginFormat::Internal,
            id: graph_out_id.clone(),
            remove_requested: false,
        });

        new_self.num_plugins += 1;

        (new_self, graph_in_id, graph_out_id)
    }

    pub fn add_plugin_instance(
        &mut self,
        plugin_and_host_request: Option<(PluginMainThreadType, HostRequest)>,
        mut save_state: PluginSaveState,
        debug_name: Shared<String>,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        activate: bool,
    ) -> (PluginInstanceID, PluginActivationStatus) {
        let node_id = if let Some(node_id) = self.free_graph_plugins.pop() {
            node_id
        } else {
            self.graph_plugins.push(None);
            NodeRef::new(self.graph_plugins.len() - 1)
        };

        let id = PluginInstanceID {
            node_id,
            format: save_state.key.format.into(),
            name: Some(debug_name),
        };

        let node_ref = abstract_graph.node(id.clone());
        // If this isn't right then I did something wrong.
        assert_eq!(node_ref, id.node_id);

        let mut plugin = PluginInstance::new(
            plugin_and_host_request,
            save_state,
            id.clone(),
            abstract_graph,
            activate,
        );

        let activation_status = if activate && plugin.can_activate() {
            plugin.activate(
                self.sample_rate,
                self.min_frames,
                self.max_frames,
                false,
                abstract_graph,
                &self.coll_handle,
            )
        } else {
            PluginActivationStatus::Inactive
        };

        let node_i: usize = node_id.into();
        self.graph_plugins[node_i] = Some(plugin);

        self.num_plugins += 1;

        log::debug!("Added plugin instance {:?} to audio graph", &id);

        (id, activation_status)
    }

    pub fn remove_plugin_instance(
        &mut self,
        id: &PluginInstanceID,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    ) {
        let node_i: usize = id.node_id.into();
        let do_remove_now = if let Some(plugin_instance) = self.graph_plugins[node_i].as_mut() {
            plugin_instance.request_remove();
            plugin_instance.can_remove()
        } else {
            log::debug!("Ignored request to remove plugin instance {:?} from audio graph: plugin was already removed", id);
            false
        };

        if do_remove_now {
            self.remove_plugin_instance(id, abstract_graph);
        }
    }

    pub fn activate_plugin_instance(
        &mut self,
        id: &PluginInstanceID,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
        check_for_port_change: bool,
    ) -> PluginActivationStatus {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &mut self.graph_plugins[node_i] {
            if plugin_instance.can_activate() {
                plugin_instance.activate(
                    self.sample_rate,
                    self.min_frames,
                    self.max_frames,
                    check_for_port_change,
                    abstract_graph,
                    &self.coll_handle,
                )
            } else {
                PluginActivationStatus::Error(PluginActivationError {
                    plugin_id: id.clone(),
                    error: "Plugin is not in an activatable state".into(),
                })
            }
        } else {
            PluginActivationStatus::Error(PluginActivationError {
                plugin_id: id.clone(),
                error: "Unexpected error: Plugin instance does not exist in the audio graph".into(),
            })
        }
    }

    pub fn deactivate_plugin_instance(&mut self, id: &PluginInstanceID) {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &mut self.graph_plugins[node_i] {
            plugin_instance.deactivate();
        }
    }

    #[inline]
    pub fn get_audio_ports_ext(
        &self,
        id: &PluginInstanceID,
    ) -> Result<Option<&AudioPortsExtension>, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            if let Some(loaded) = &plugin_instance.loaded {
                Ok(Some(&loaded.audio_ports_ext))
            } else {
                Ok(None)
            }
        } else {
            Err(())
        }
    }

    #[inline]
    pub fn get_id_by_ref(&self, id: NodeRef) -> Result<PluginInstanceID, ()> {
        let node_i: usize = id.into();
        if let Some(plugin_instance) = &self.graph_plugins[node_i] {
            Ok(plugin_instance.id.clone())
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
    ) -> Result<Option<&PluginAudioThreadType>, ()> {
        let node_i: usize = id.node_id.into();
        if let Some(plugin_instance) = self.graph_plugins[node_i].as_ref() {
            if let Some(plugin) = plugin_instance.loaded.as_ref() {
                Ok(plugin.audio_thread.as_ref())
            } else {
                Ok(None)
            }
        } else {
            Err(())
        }
    }

    pub fn iter_plugin_ids(&self) -> impl Iterator<Item = &PluginInstanceID> + '_ {
        self.graph_plugins.iter().filter_map(|p| p.as_ref().map(|p| &p.id))
    }

    pub fn get_graph_plugin_save_state(&self, node_id: NodeRef) -> Result<&PluginSaveState, ()> {
        let node_i: usize = node_id.into();
        if let Some(plugin) = self.graph_plugins[node_i].as_ref() {
            if let Some(loaded) = &plugin.loaded {
                Ok(&loaded.save_state)
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

    pub fn on_main_thread(
        &mut self,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    ) -> SmallVec<[(PluginInstanceID, MainThreadStatus); 4]> {
        //log::trace!("Engine start on main thread calls...");

        let mut changed_plugins_status: SmallVec<[(PluginInstanceID, MainThreadStatus); 4]> =
            SmallVec::new();
        let mut plugins_to_remove: SmallVec<[PluginInstanceID; 4]> = SmallVec::new();

        // TODO: Find a more optimal way to poll for requests? We can't just use an spsc message
        // channel because CLAP plugins use the same host pointer for requests in the audio thread
        // and in the main thread. Is there some thread-safe way to get a list of only the plugins
        // that have requested something?
        for plugin in self.graph_plugins.iter_mut().filter_map(|p| p.as_mut()) {
            let status = plugin.on_main_thread(
                self.sample_rate,
                self.min_frames,
                self.max_frames,
                abstract_graph,
                &self.coll_handle,
            );

            if let Some(status) = status.take() {
                if let MainThreadStatus::Removed = status {
                    // Remove the plugin from the graph.
                    plugins_to_remove.push(plugin.id.clone());
                }
                changed_plugins_status.push((plugin.id.clone(), status));
            }
        }

        for id in plugins_to_remove.iter() {
            self.remove_plugin(&id, abstract_graph);
        }

        changed_plugins_status
    }

    fn remove_plugin(
        &mut self,
        id: &PluginInstanceID,
        abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    ) {
        let node_i: usize = id.node_id.into();

        // Drop the plugin instance here.
        self.graph_plugins[node_i] = None;

        // Re-use this node ID for the next new plugin.
        self.free_graph_plugins.push(id.node_id);

        self.num_plugins -= 1;

        abstract_graph.delete_node(id.node_id).unwrap();

        log::debug!("Removed plugin instance {:?} from audio graph", id);
    }
}

impl Drop for PluginInstancePool {
    fn drop(&mut self) {
        let ids: Vec<PluginInstanceID> = self.iter_plugin_ids().map(|id| id.clone()).collect();
        for id in ids.iter() {
            self.deactivate_plugin_instance(id);
        }

        std::thread::sleep(std::time::Duration::from_secs(4));
    }
}

#[derive(Debug)]
pub struct PluginActivatedInfo {
    pub id: PluginInstanceID,
    pub edges: PluginEdges,
    pub save_state: PluginSaveState,
}

#[derive(Debug)]
pub struct PluginActivationError {
    pub plugin_id: PluginInstanceID,
    pub error: Box<dyn Error>,
}

impl Error for PluginActivationError {}

impl std::fmt::Display for PluginActivationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to activate plugin instance {:?}: {}", &self.plugin_id, &self.error)
    }
}
