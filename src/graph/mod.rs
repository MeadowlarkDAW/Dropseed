use audio_graph::{DefaultPortType, Graph, NodeRef};
use basedrop::Shared;
use fnv::FnvHashMap;
use fnv::FnvHashSet;
use rusty_daw_core::SampleRate;
use smallvec::SmallVec;
use std::error::Error;

pub(crate) mod plugin_host;
pub(crate) mod shared_pool;

mod compiler;
mod save_state;
mod schedule;
mod verifier;

use schedule::Schedule;
use shared_pool::{PluginInstanceHostEntry, SharedPool};
use verifier::Verifier;

use crate::graph::plugin_host::PluginInstanceHost;
use crate::graph::shared_pool::SharedPluginHostAudioThread;
use crate::host_request::HostRequest;
use crate::plugin::ext::audio_ports::AudioPortsExtension;

pub use compiler::GraphCompilerError;
pub use plugin_host::ActivatePluginError;
pub use save_state::{AudioGraphSaveState, EdgeSaveState};
pub use schedule::SharedSchedule;
pub use shared_pool::PluginInstanceID;
pub use verifier::VerifyScheduleError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortID {
    AudioIn(u16),
    AudioOut(u16),
}

impl PortID {
    pub fn as_index(&self) -> usize {
        match self {
            PortID::AudioIn(i) => usize::from(*i),
            PortID::AudioOut(i) => usize::from(*i),
        }
    }
}

use crate::{
    host_request::HostInfo,
    plugin::PluginSaveState,
    plugin_scanner::{NewPluginInstanceError, PluginScanner},
};

use self::plugin_host::OnIdleResult;

pub(crate) struct AudioGraph {
    shared_pool: SharedPool,
    host_info: Shared<HostInfo>,
    verifier: Verifier,

    abstract_graph: Graph<PluginInstanceID, PortID, DefaultPortType>,
    coll_handle: basedrop::Handle,

    shared_schedule: SharedSchedule,

    failed_plugin_debug_name: Shared<String>,

    graph_in_node_id: PluginInstanceID,
    graph_out_node_id: PluginInstanceID,

    graph_in_channels: u16,
    graph_out_channels: u16,

    sample_rate: SampleRate,
    min_frames: u32,
    max_frames: u32,
}

impl AudioGraph {
    pub(crate) fn new(
        coll_handle: basedrop::Handle,
        host_info: Shared<HostInfo>,
        graph_in_channels: u16,
        graph_out_channels: u16,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
    ) -> (Self, SharedSchedule) {
        assert!(graph_in_channels > 0);
        assert!(graph_out_channels > 0);

        let failed_plugin_debug_name = Shared::new(&coll_handle, String::from("failed_plugin"));

        let mut abstract_graph = Graph::default();

        let mut shared_pool = SharedPool::new(max_frames as usize, coll_handle.clone());

        // ---  Add the graph input and graph output nodes to the graph  --------------------------

        let mut graph_in_node_id = PluginInstanceID {
            node_ref: audio_graph::NodeRef::new(0),
            format: shared_pool::PluginInstanceType::GraphInput,
            name: None,
        };
        let mut graph_out_node_id = PluginInstanceID {
            node_ref: audio_graph::NodeRef::new(1),
            format: shared_pool::PluginInstanceType::GraphOutput,
            name: None,
        };

        graph_in_node_id.node_ref = abstract_graph.node(graph_in_node_id.clone());
        graph_out_node_id.node_ref = abstract_graph.node(graph_out_node_id.clone());

        abstract_graph.set_node_ident(graph_in_node_id.node_ref, graph_in_node_id.clone()).unwrap();
        abstract_graph
            .set_node_ident(graph_out_node_id.node_ref, graph_out_node_id.clone())
            .unwrap();

        let graph_in_out_channel_refs: Vec<audio_graph::PortRef> = (0..graph_in_channels)
            .map(|i| {
                abstract_graph
                    .port(graph_in_node_id.node_ref, DefaultPortType::Audio, PortID::AudioOut(i))
                    .unwrap()
            })
            .collect();
        let graph_out_in_channel_refs: Vec<audio_graph::PortRef> = (0..graph_out_channels)
            .map(|i| {
                abstract_graph
                    .port(graph_out_node_id.node_ref, DefaultPortType::Audio, PortID::AudioIn(i))
                    .unwrap()
            })
            .collect();

        let _ = shared_pool.plugins.insert(
            graph_in_node_id.clone(),
            PluginInstanceHostEntry {
                plugin_host: PluginInstanceHost::new(
                    graph_in_node_id.clone(),
                    None,
                    HostRequest::new(Shared::clone(&host_info)),
                    &coll_handle,
                ),
                audio_thread: None,
                audio_in_channel_refs: Vec::new(),
                audio_out_channel_refs: graph_in_out_channel_refs,
            },
        );
        let _ = shared_pool.plugins.insert(
            graph_out_node_id.clone(),
            PluginInstanceHostEntry {
                plugin_host: PluginInstanceHost::new(
                    graph_out_node_id.clone(),
                    None,
                    HostRequest::new(Shared::clone(&host_info)),
                    &coll_handle,
                ),
                audio_thread: None,
                audio_in_channel_refs: graph_out_in_channel_refs,
                audio_out_channel_refs: Vec::new(),
            },
        );

        // ----------------------------------------------------------------------------------------

        let (shared_schedule, shared_schedule_clone) = SharedSchedule::new(
            Schedule::empty(max_frames as usize, Shared::clone(&host_info)),
            &coll_handle,
        );

        let new_self = Self {
            shared_pool,
            host_info,
            verifier: Verifier::new(),
            abstract_graph,
            coll_handle,
            shared_schedule,
            failed_plugin_debug_name,
            graph_in_node_id,
            graph_out_node_id,
            graph_in_channels,
            graph_out_channels,
            sample_rate,
            min_frames,
            max_frames,
        };

        (new_self, shared_schedule_clone)
    }

    pub fn graph_in_node_id(&self) -> &PluginInstanceID {
        &self.graph_in_node_id
    }

    pub fn graph_out_node_id(&self) -> &PluginInstanceID {
        &self.graph_out_node_id
    }

    pub fn add_new_plugin_instance(
        &mut self,
        save_state: PluginSaveState,
        plugin_scanner: &mut PluginScanner,
        activate: bool,
        fallback_to_other_formats: bool,
    ) -> NewPluginRes {
        let temp_id = PluginInstanceID {
            node_ref: audio_graph::NodeRef::new(0),
            format: shared_pool::PluginInstanceType::Unloaded,
            name: None,
        };

        let node_ref = self.abstract_graph.node(temp_id);

        let mut res = plugin_scanner.create_plugin(save_state, node_ref, fallback_to_other_formats);

        let plugin_id = res.plugin_host.id.clone();

        match res.status {
            Ok(()) => {
                log::debug!("Loaded plugin {:?} successfully", &res.plugin_host.id);
            }
            Err(e) => {
                log::error!(
                    "Failed to load plugin {:?} from save state: {}",
                    &res.plugin_host.id,
                    e
                );
            }
        }

        let entry = PluginInstanceHostEntry {
            plugin_host: res.plugin_host,
            audio_thread: None,
            audio_in_channel_refs: Vec::new(),
            audio_out_channel_refs: Vec::new(),
        };

        if self.shared_pool.plugins.insert(plugin_id.clone(), entry).is_some() {
            panic!("Something went wrong when allocating a new slot for a plugin");
        }

        let activation_status = if activate {
            self.activate_plugin_instance(&plugin_id).unwrap()
        } else {
            PluginActivationStatus::Inactive
        };

        match &activation_status {
            PluginActivationStatus::Activated { .. } => {}
            _ => {
                // Try to retrieve the number of channels from a previously working
                // save state.

                let (num_in_channels, num_out_channels) = save_state.audio_in_out_channels;

                let entry = self.shared_pool.plugins.get_mut(&plugin_id).unwrap();

                entry.audio_in_channel_refs = (0..num_in_channels)
                    .map(|i| {
                        self.abstract_graph
                            .port(plugin_id.node_ref, DefaultPortType::Audio, PortID::AudioIn(i))
                            .unwrap()
                    })
                    .collect();
                entry.audio_out_channel_refs = (0..num_out_channels)
                    .map(|i| {
                        self.abstract_graph
                            .port(plugin_id.node_ref, DefaultPortType::Audio, PortID::AudioOut(i))
                            .unwrap()
                    })
                    .collect();
            }
        }

        NewPluginRes { plugin_id, status: activation_status }
    }

    pub fn activate_plugin_instance(
        &mut self,
        id: &PluginInstanceID,
    ) -> Result<PluginActivationStatus, ()> {
        let entry = if let Some(entry) = self.shared_pool.plugins.get_mut(id) {
            entry
        } else {
            return Err(());
        };

        if let Err(e) = entry.plugin_host.can_activate() {
            return Ok(PluginActivationStatus::ActivationError(e));
        }

        let (plugin_audio_thread, activation_status) = match entry.plugin_host.audio_ports_ext() {
            Ok(audio_ports_ext) => {
                match entry.plugin_host.activate(
                    self.sample_rate,
                    self.min_frames as u32,
                    self.max_frames as u32,
                    &self.coll_handle,
                ) {
                    Ok(plugin_audio_thread) => (
                        Some(SharedPluginHostAudioThread::new(
                            plugin_audio_thread,
                            &self.coll_handle,
                        )),
                        PluginActivationStatus::Activated { audio_ports: audio_ports_ext },
                    ),
                    Err(e) => (None, PluginActivationStatus::ActivationError(e)),
                }
            }
            Err(e) => (
                None,
                PluginActivationStatus::ActivationError(
                    ActivatePluginError::PluginFailedToGetAudioPortsExt(e),
                ),
            ),
        };

        entry.audio_thread = plugin_audio_thread;

        if let PluginActivationStatus::Activated { audio_ports } = &activation_status {
            // Update the number of channels (ports) in our abstract graph.

            let num_in_channels = audio_ports.total_in_channels();
            let num_out_channels = audio_ports.total_out_channels();

            if entry.audio_in_channel_refs.len() < num_in_channels {
                let old_len = entry.audio_in_channel_refs.len();
                for i in old_len as u16..num_in_channels as u16 {
                    self.abstract_graph
                        .port(
                            entry.plugin_host.id.node_ref,
                            DefaultPortType::Audio,
                            PortID::AudioIn(i),
                        )
                        .unwrap();
                }
            } else if entry.audio_in_channel_refs.len() > num_in_channels {
                let num_to_remove = entry.audio_in_channel_refs.len() - num_in_channels;
                for _ in 0..num_to_remove {
                    let port_ref = entry.audio_in_channel_refs.pop().unwrap();
                    self.abstract_graph.delete_port(port_ref).unwrap();
                }
            }

            if entry.audio_out_channel_refs.len() < num_out_channels {
                let old_len = entry.audio_out_channel_refs.len();
                for i in old_len as u16..num_out_channels as u16 {
                    self.abstract_graph
                        .port(
                            entry.plugin_host.id.node_ref,
                            DefaultPortType::Audio,
                            PortID::AudioOut(i),
                        )
                        .unwrap();
                }
            } else if entry.audio_out_channel_refs.len() > num_out_channels {
                let num_to_remove = entry.audio_out_channel_refs.len() - num_out_channels;
                for _ in 0..num_to_remove {
                    let port_ref = entry.audio_out_channel_refs.pop().unwrap();
                    self.abstract_graph.delete_port(port_ref).unwrap();
                }
            }
        }

        Ok(activation_status)
    }

    /// Remove the given plugins from the graph.
    ///
    /// This will also automatically disconnect all edges that were connected to these
    /// plugins.
    ///
    /// Requests to remove the "graph input/output" nodes with the IDs `AudioGraph::graph_in_node_id()`
    /// and `AudioGraph::graph_out_node_id()` will be ignored.
    pub fn remove_plugin_instances(
        &mut self,
        plugin_ids: &[PluginInstanceID],
        affected_plugins: &mut FnvHashSet<PluginInstanceID>,
    ) -> FnvHashSet<PluginInstanceID> {
        let mut removed_plugins: FnvHashSet<PluginInstanceID> = FnvHashSet::default();

        for id in plugin_ids.iter() {
            if id == &self.graph_in_node_id || id == &self.graph_out_node_id {
                if id == &self.graph_in_node_id {
                    log::warn!("Ignored request to remove the graph in node from the audio graph");
                } else {
                    log::warn!("Ignored request to remove the graph out node from the audio graph");
                }
                continue;
            }

            if removed_plugins.insert(id.clone()) {
                if let Ok(edges) = self.get_plugin_edges(&id) {
                    for e in edges.incoming {
                        let _ = affected_plugins.insert(e.src_plugin_id.clone());
                    }
                    for e in edges.outgoing {
                        let _ = affected_plugins.insert(e.dst_plugin_id.clone());
                    }

                    self.shared_pool.plugins.remove(&id);
                    self.abstract_graph.delete_node(id.node_ref);
                } else {
                    let _ = removed_plugins.remove(&id);
                    log::warn!("Ignored request to remove plugin instance {:?}: Plugin is already removed.", id);
                }
            }
        }

        removed_plugins
    }

    pub fn connect_edge(&mut self, edge: &Edge) -> Result<(), ConnectEdgeError> {
        let src_entry = if let Some(entry) = self.shared_pool.plugins.get(&edge.src_plugin_id) {
            entry
        } else {
            return Err(ConnectEdgeError::SrcPluginDoesNotExist);
        };
        let dst_entry = if let Some(entry) = self.shared_pool.plugins.get(&edge.dst_plugin_id) {
            entry
        } else {
            return Err(ConnectEdgeError::DstPluginDoesNotExist);
        };

        match edge.edge_type {
            DefaultPortType::Audio => {
                let src_channel_ref = if let Some(ch_ref) =
                    src_entry.audio_out_channel_refs.get(usize::from(edge.src_channel))
                {
                    ch_ref
                } else {
                    return Err(ConnectEdgeError::SrcChannelOutOfBounds(
                        edge.src_channel,
                        src_entry.audio_out_channel_refs.len() as u16,
                    ));
                };
                let dst_channel_ref = if let Some(ch_ref) =
                    dst_entry.audio_in_channel_refs.get(usize::from(edge.dst_channel))
                {
                    ch_ref
                } else {
                    return Err(ConnectEdgeError::DstChannelOutOfBounds(
                        edge.dst_channel,
                        dst_entry.audio_in_channel_refs.len() as u16,
                    ));
                };

                match self.abstract_graph.connect(*src_channel_ref, *dst_channel_ref) {
                    Ok(()) => {
                        log::trace!(
                            "Successfully connected edge: {:?}",
                            Edge {
                                edge_type: DefaultPortType::Audio,
                                src_plugin_id: edge.src_plugin_id.clone(),
                                dst_plugin_id: edge.dst_plugin_id.clone(),
                                src_channel: edge.src_channel,
                                dst_channel: edge.dst_channel,
                            }
                        );

                        Ok(())
                    }
                    Err(e) => {
                        if let audio_graph::Error::Cycle = e {
                            Err(ConnectEdgeError::Cycle)
                        } else {
                            log::error!("Unexpected edge connect error: {}", e);
                            Err(ConnectEdgeError::Unkown)
                        }
                    }
                }
            }
            DefaultPortType::Event => {
                todo!()
            }
        }
    }

    pub fn disconnect_edge(&mut self, edge: &Edge) -> bool {
        let mut found_ports = None;
        if let Ok(edges) = self.abstract_graph.node_edges(edge.src_plugin_id.node_ref) {
            // Find the corresponding edge.
            for e in edges.iter() {
                if e.dst_node != edge.dst_plugin_id.node_ref {
                    continue;
                }
                if e.src_node != edge.src_plugin_id.node_ref {
                    continue;
                }
                if e.port_type != edge.edge_type {
                    continue;
                }
                if self.abstract_graph.port_ident(e.src_port).unwrap().as_index()
                    != usize::from(edge.src_channel)
                {
                    continue;
                }
                if self.abstract_graph.port_ident(e.dst_port).unwrap().as_index()
                    != usize::from(edge.dst_channel)
                {
                    continue;
                }

                found_ports = Some((e.src_port, e.dst_port));
                break;
            }
        }

        if let Some((src_port, dst_port)) = found_ports {
            if let Err(e) = self.abstract_graph.disconnect(src_port, dst_port) {
                log::error!("Unexpected error while disconnecting edge {:?}: {}", edge, e);
            } else {
                log::trace!("Successfully disconnected edge: {:?}", edge);
            }
            true
        } else {
            log::warn!("Could not disconnect edge: {:?}: Edge was not found in the graph", edge);
            false
        }
    }

    pub fn get_plugin_edges(&self, id: &PluginInstanceID) -> Result<PluginEdges, ()> {
        if let Ok(edges) = self.abstract_graph.node_edges(id.node_ref) {
            let mut incoming: SmallVec<[Edge; 8]> = SmallVec::new();
            let mut outgoing: SmallVec<[Edge; 8]> = SmallVec::new();

            for edge in edges.iter() {
                let src_channel =
                    self.abstract_graph.port_ident(edge.src_port).unwrap().as_index() as u16;
                let dst_channel =
                    self.abstract_graph.port_ident(edge.dst_port).unwrap().as_index() as u16;

                if edge.src_node == id.node_ref {
                    outgoing.push(Edge {
                        edge_type: edge.port_type,
                        src_plugin_id: id.clone(),
                        dst_plugin_id: self
                            .abstract_graph
                            .node_ident(edge.dst_node)
                            .unwrap()
                            .clone(),
                        src_channel,
                        dst_channel,
                    });
                } else {
                    incoming.push(Edge {
                        edge_type: edge.port_type,
                        src_plugin_id: self
                            .abstract_graph
                            .node_ident(edge.src_node)
                            .unwrap()
                            .clone(),
                        dst_plugin_id: id.clone(),
                        src_channel,
                        dst_channel,
                    });
                }
            }

            Ok(PluginEdges { incoming, outgoing })
        } else {
            Err(())
        }
    }

    pub fn get_plugin_save_state(&self, id: &PluginInstanceID) -> Result<&PluginSaveState, ()> {
        self.plugin_pool.get_graph_plugin_save_state(id.node_id)
    }

    pub fn collect_save_state(&self) -> AudioGraphSaveState {
        log::trace!("Collecting audio graph save state...");

        let mut plugins: Vec<PluginSaveState> = Vec::with_capacity(self.plugin_pool.num_plugins());
        let mut edges: Vec<EdgeSaveState> = Vec::with_capacity(self.plugin_pool.num_plugins() * 3);

        let mut node_id_to_index: FnvHashMap<NodeRef, usize> = FnvHashMap::default();
        node_id_to_index.reserve(self.plugin_pool.num_plugins());

        for (index, plugin_id) in self.plugin_pool.iter_plugin_ids().enumerate() {
            if let Some(_) = node_id_to_index.insert(plugin_id.node_id, index) {
                // In theory this should never happen.
                panic!("More than one plugin with node id: {:?}", plugin_id.node_id);
            }

            if plugin_id.node_id == self.graph_in_node_id.node_id
                || plugin_id.node_id == self.graph_out_node_id.node_id
            {
                continue;
            }

            let save_state =
                self.plugin_pool.get_graph_plugin_save_state(plugin_id.node_id).unwrap().clone();
            plugins.push(save_state);
        }

        // Iterate again to get all the edges.
        for plugin_id in self.plugin_pool.iter_plugin_ids() {
            for edge in self.abstract_graph.node_edges(plugin_id.node_id).unwrap() {
                edges.push(EdgeSaveState {
                    edge_type: DefaultPortType::Audio,
                    src_plugin_i: *node_id_to_index.get(&edge.src_node).unwrap(),
                    dst_plugin_i: *node_id_to_index.get(&edge.dst_node).unwrap(),
                    src_channel: self.abstract_graph.port_ident(edge.src_port).unwrap().as_index()
                        as u16,
                    dst_channel: self.abstract_graph.port_ident(edge.dst_port).unwrap().as_index()
                        as u16,
                });
            }
        }

        AudioGraphSaveState { plugins, edges }
    }

    pub fn restore_from_save_state(
        &mut self,
        save_state: &AudioGraphSaveState,
        plugin_scanner: &mut PluginScanner,
        fallback_to_other_formats: bool,
    ) -> (Vec<NewPluginRes>, Vec<(PluginInstanceID, PluginEdges)>) {
        log::info!("Restoring audio graph from save state...");

        self.abstract_graph = Graph::default();
        /*
        let (plugin_pool, graph_in_id, graph_out_id) = PluginInstancePool::new(
            &mut self.abstract_graph,
            self.graph_in_channels,
            self.graph_out_channels,
            self.coll_handle.clone(),
            Shared::clone(&self.host_info),
            self.sample_rate,
            self.min_frames,
            self.max_frames,
        );
        */
        self.plugin_pool = plugin_pool;
        self.graph_in_node_id = graph_in_id;
        self.graph_out_node_id = graph_out_id;

        let mut plugin_results: Vec<NewPluginRes> = Vec::with_capacity(save_state.plugins.len());

        for plugin_save_state in save_state.plugins.iter() {
            plugin_results.push(self.add_new_plugin_instance(
                &plugin_save_state.key,
                Some(plugin_save_state.clone()),
                plugin_scanner,
                plugin_save_state.activation_requested,
                fallback_to_other_formats,
            ));
        }

        for (i, edge_save_state) in save_state.edges.iter().enumerate() {
            if edge_save_state.src_plugin_i >= plugin_results.len() + 2 {
                log::error!(
                    "Could not connect edge from save state {:?}, Source plugin does not exist",
                    edge_save_state
                );
                continue;
            }
            if edge_save_state.dst_plugin_i >= plugin_results.len() + 2 {
                log::error!("Could not connect edge from save state {:?}, Destination plugin does not exist", edge_save_state);
                continue;
            }

            let src_plugin_id = if edge_save_state.src_plugin_i == 0 {
                self.graph_in_node_id.clone()
            } else if edge_save_state.src_plugin_i == 1 {
                self.graph_out_node_id.clone()
            } else {
                plugin_results[edge_save_state.src_plugin_i - 2].plugin_id.clone()
            };

            let dst_plugin_id = if edge_save_state.dst_plugin_i == 0 {
                self.graph_in_node_id.clone()
            } else if edge_save_state.dst_plugin_i == 1 {
                self.graph_out_node_id.clone()
            } else {
                plugin_results[edge_save_state.dst_plugin_i - 2].plugin_id.clone()
            };

            let edge = Edge {
                edge_type: edge_save_state.edge_type,
                src_plugin_id,
                dst_plugin_id,
                src_channel: edge_save_state.src_channel,
                dst_channel: edge_save_state.dst_channel,
            };

            if let Err(e) = self.connect_edge(&edge) {
                log::error!("Could not connect edge from save state {:?}, {}", edge_save_state, e);
            }
        }

        let plugins_new_edges: Vec<(PluginInstanceID, PluginEdges)> = plugin_results
            .iter()
            .map(|plugin_res| {
                let id = plugin_res.plugin_id.clone();
                let edges = self.get_plugin_edges(&id).unwrap();
                (id, edges)
            })
            .collect();

        (plugin_results, plugins_new_edges)
    }

    /// Compile the audio graph into a schedule that is sent to the audio thread.
    ///
    /// If an error is returned then the graph **MUST** be restored with the previous
    /// working save state.
    pub(crate) fn compile(&mut self) -> Result<(), GraphCompilerError> {
        match compiler::compile_graph(
            &mut self.shared_pool,
            &mut self.abstract_graph,
            &self.graph_in_node_id,
            &self.graph_out_node_id,
            &mut self.verifier,
        ) {
            Ok(schedule) => {
                log::debug!("Successfully compiled new schedule:\n{:?}", &schedule);

                self.shared_schedule.set_new_schedule(schedule, &self.coll_handle);
                Ok(())
            }
            Err(e) => {
                // Replace the current schedule with an emtpy one now that the graph
                // is in an invalid state.
                self.shared_schedule.set_new_schedule(
                    Schedule::empty(self.max_frames as usize, Shared::clone(&self.host_info)),
                    &self.coll_handle,
                );
                Err(e)
            }
        }
    }

    pub(crate) fn on_main_thread(&mut self) -> SmallVec<[(PluginInstanceID, OnIdleResult); 4]> {
        let mut changed_plugins: SmallVec<[(PluginInstanceID, OnIdleResult); 4]> = SmallVec::new();

        for plugin in self.shared_pool.plugins.values_mut() {
            match plugin.plugin_host.on_idle(
                self.sample_rate,
                self.min_frames,
                self.max_frames,
                &self.coll_handle,
            ) {
                OnIdleResult::Ok => {}
                res => changed_plugins.push((plugin.plugin_host.id.clone(), res)),
            }
        }

        changed_plugins
    }
}

impl Drop for AudioGraph {
    fn drop(&mut self) {
        /*
        self.shared_schedule.set_new_schedule(
            Schedule::empty(self.max_frames, Shared::clone(&self.host_info)),
            &self.coll_handle,
        );
        */
    }
}

#[derive(Debug)]
pub enum PluginActivationStatus {
    /// This means the plugin successfully activated and returned
    /// its new audio/event port configuration.
    Activated { audio_ports: AudioPortsExtension },

    /// This means that the plugin loaded but did not activate yet. This
    /// can happen when the user loads a project with a deactivated
    /// plugin.
    Inactive,

    /// There was an error loading the plugin.
    LoadError(NewPluginInstanceError),

    /// There was an error activating the plugin.
    ActivationError(ActivatePluginError),
}

#[derive(Debug)]
pub struct NewPluginRes {
    pub plugin_id: PluginInstanceID,

    pub status: PluginActivationStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginEdges {
    pub incoming: SmallVec<[Edge; 8]>,
    pub outgoing: SmallVec<[Edge; 8]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub edge_type: DefaultPortType,

    pub src_plugin_id: PluginInstanceID,
    pub dst_plugin_id: PluginInstanceID,

    pub src_channel: u16,
    pub dst_channel: u16,
}

#[derive(Debug)]
pub enum ConnectEdgeError {
    SrcPluginDoesNotExist,
    DstPluginDoesNotExist,
    /// (requested channel index, total number of output channels on source plugin)
    SrcChannelOutOfBounds(u16, u16),
    /// (requested channel index, total number of input channels on destination plugin)
    DstChannelOutOfBounds(u16, u16),
    Cycle,
    Unkown,
}

impl Error for ConnectEdgeError {}

impl std::fmt::Display for ConnectEdgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            ConnectEdgeError::SrcPluginDoesNotExist => {
                write!(f, "Could not add edge to graph: Source plugin does not exist")
            }
            ConnectEdgeError::DstPluginDoesNotExist => {
                write!(f, "Could not add edge to graph: Destination plugin does not exist")
            }
            ConnectEdgeError::SrcChannelOutOfBounds(i, max_i) => {
                write!(f, "Could not add edge to graph: Index {} out of bounds of source plugin with {} output channels", i, max_i)
            }
            ConnectEdgeError::DstChannelOutOfBounds(i, max_i) => {
                write!(f, "Could not add edge to graph: Index {} out of bounds of destination plugin with {} input channels", i, max_i)
            }
            ConnectEdgeError::Cycle => {
                write!(f, "Could not add edge to graph: Cycle detected")
            }
            ConnectEdgeError::Unkown => {
                write!(f, "Could not add edge to graph: Unkown error")
            }
        }
    }
}

#[derive(Debug)]
pub enum InsertPluginBetweenError {
    SrcPluginNotFound(PluginInstanceID),
    DstPluginNotFound(PluginInstanceID),
}

impl Error for InsertPluginBetweenError {}

impl std::fmt::Display for InsertPluginBetweenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            InsertPluginBetweenError::SrcPluginNotFound(id) => {
                write!(f, "Could not insert new plugin instance: The source plugin with ID {:?} was not found", &id)
            }
            InsertPluginBetweenError::DstPluginNotFound(id) => {
                write!(f, "Could not insert new plugin instance: The destination plugin with ID {:?} was not found", &id)
            }
        }
    }
}

#[derive(Debug)]
pub enum RemovePluginBetweenError {
    PluginNotFound(PluginInstanceID),
    SrcPluginNotFound(PluginInstanceID),
    DstPluginNotFound(PluginInstanceID),
}

impl Error for RemovePluginBetweenError {}

impl std::fmt::Display for RemovePluginBetweenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            RemovePluginBetweenError::PluginNotFound(id) => {
                write!(
                    f,
                    "Could not remove plugin instance: The plugin with ID {:?} was not found",
                    &id
                )
            }
            RemovePluginBetweenError::SrcPluginNotFound(id) => {
                write!(f, "Could not remove plugin instance: The source plugin with ID {:?} was not found", &id)
            }
            RemovePluginBetweenError::DstPluginNotFound(id) => {
                write!(f, "Could not remove plugin instance: The destination plugin with ID {:?} was not found", &id)
            }
        }
    }
}
