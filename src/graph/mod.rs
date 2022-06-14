use audio_graph::{DefaultPortType, Graph, NodeRef};
use basedrop::Shared;
use crossbeam::channel::Sender;
use fnv::FnvHashMap;
use fnv::FnvHashSet;
use meadowlark_core_types::SampleRate;
use smallvec::SmallVec;
use std::error::Error;

pub(crate) mod plugin_host;
pub(crate) mod schedule;
pub(crate) mod shared_pool;

mod compiler;
mod save_state;
mod verifier;

use plugin_host::OnIdleResult;
use schedule::{Schedule, SharedSchedule};
use shared_pool::{PluginInstanceHostEntry, SharedBufferPool, SharedPluginPool};
use verifier::Verifier;

use crate::engine::events::from_engine::{DSEngineEvent, PluginEvent};
use crate::engine::plugin_scanner::{NewPluginInstanceError, PluginScanner};
use crate::graph::plugin_host::PluginInstanceHost;
use crate::graph::shared_pool::SharedPluginHostAudioThread;
use crate::plugin::ext::audio_ports::PluginAudioPortsExt;
use crate::plugin::host_request::{HostInfo, HostRequest};
use crate::plugin::PluginSaveState;
use crate::utils::thread_id::SharedThreadIDs;
use crate::ParamID;

pub use compiler::GraphCompilerError;
pub use plugin_host::{
    ActivatePluginError, ParamGestureInfo, ParamModifiedInfo, PluginHandle, PluginParamsExt,
};
pub use save_state::{AudioGraphSaveState, EdgeSaveState};
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

pub(crate) struct AudioGraph {
    shared_plugin_pool: SharedPluginPool,
    shared_buffer_pool: SharedBufferPool,
    host_info: Shared<HostInfo>,
    verifier: Verifier,

    abstract_graph: Graph<PluginInstanceID, PortID, DefaultPortType>,
    coll_handle: basedrop::Handle,

    shared_schedule: SharedSchedule,

    graph_in_node_id: PluginInstanceID,
    graph_out_node_id: PluginInstanceID,

    graph_in_channels: u16,
    graph_out_channels: u16,

    graph_in_rdn: Shared<String>,
    graph_out_rdn: Shared<String>,
    temp_rdn: Shared<String>,

    sample_rate: SampleRate,
    min_frames: u32,
    max_frames: u32,
}

impl AudioGraph {
    pub fn new(
        coll_handle: basedrop::Handle,
        host_info: Shared<HostInfo>,
        graph_in_channels: u16,
        graph_out_channels: u16,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        thread_ids: SharedThreadIDs,
    ) -> (Self, SharedSchedule) {
        assert!(graph_in_channels > 0);
        assert!(graph_out_channels > 0);

        let abstract_graph = Graph::default();

        let shared_plugin_pool = SharedPluginPool::new();
        let shared_buffer_pool = SharedBufferPool::new(max_frames as usize, coll_handle.clone());

        let (shared_schedule, shared_schedule_clone) =
            SharedSchedule::new(Schedule::empty(max_frames as usize), thread_ids, &coll_handle);

        let graph_in_rdn = Shared::new(&coll_handle, String::from("org.rustydaw.graph_in_node"));
        let graph_out_rdn = Shared::new(&coll_handle, String::from("org.rustydaw.graph_out_node"));
        let temp_rdn = Shared::new(&coll_handle, String::from("org.rustydaw.temporary_plugin_rdn"));

        // These will get overwritten in the `reset()` method.
        let graph_in_node_id = PluginInstanceID {
            node_ref: audio_graph::NodeRef::new(0),
            format: shared_pool::PluginInstanceType::GraphInput,
            rdn: Shared::clone(&graph_in_rdn),
        };
        let graph_out_node_id = PluginInstanceID {
            node_ref: audio_graph::NodeRef::new(1),
            format: shared_pool::PluginInstanceType::GraphOutput,
            rdn: Shared::clone(&graph_out_rdn),
        };

        let mut new_self = Self {
            shared_plugin_pool,
            shared_buffer_pool,
            host_info,
            verifier: Verifier::new(),
            abstract_graph,
            coll_handle,
            shared_schedule,
            graph_in_node_id,
            graph_out_node_id,
            graph_in_channels,
            graph_out_channels,
            graph_in_rdn,
            graph_out_rdn,
            temp_rdn,
            sample_rate,
            min_frames,
            max_frames,
        };

        new_self.reset();

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
            rdn: Shared::clone(&self.temp_rdn),
        };

        let node_ref = self.abstract_graph.node(temp_id);

        let (backup_in_channels, backup_out_channels) = save_state.audio_in_out_channels;

        let res = plugin_scanner.create_plugin(save_state, node_ref, fallback_to_other_formats);

        let plugin_id = res.plugin_host.id.clone();

        self.abstract_graph.set_node_ident(node_ref, plugin_id.clone()).unwrap();

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

        if self.shared_plugin_pool.plugins.insert(plugin_id.clone(), entry).is_some() {
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

                let entry = self.shared_plugin_pool.plugins.get_mut(&plugin_id).unwrap();

                entry.audio_in_channel_refs = (0..backup_in_channels)
                    .map(|i| {
                        self.abstract_graph
                            .port(plugin_id.node_ref, DefaultPortType::Audio, PortID::AudioIn(i))
                            .unwrap()
                    })
                    .collect();
                entry.audio_out_channel_refs = (0..backup_out_channels)
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
        let entry = if let Some(entry) = self.shared_plugin_pool.plugins.get_mut(id) {
            entry
        } else {
            return Err(());
        };

        if let Err(e) = entry.plugin_host.can_activate() {
            return Ok(PluginActivationStatus::ActivationError(e));
        }

        let (plugin_audio_thread, activation_status) = match entry.plugin_host.activate(
            self.sample_rate,
            self.min_frames as u32,
            self.max_frames as u32,
            &self.coll_handle,
        ) {
            Ok((plugin_audio_thread, new_handle, new_param_values)) => (
                Some(SharedPluginHostAudioThread::new(plugin_audio_thread, &self.coll_handle)),
                PluginActivationStatus::Activated { new_handle, new_param_values },
            ),
            Err(e) => (None, PluginActivationStatus::ActivationError(e)),
        };

        entry.audio_thread = plugin_audio_thread;

        if let PluginActivationStatus::Activated { new_handle, .. } = &activation_status {
            // Update the number of channels (ports) in our abstract graph.

            update_plugin_ports(&mut self.abstract_graph, entry, new_handle.audio_ports());
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

                    if let Some(plugin) = self.shared_plugin_pool.plugins.get_mut(&id) {
                        plugin.plugin_host.schedule_remove();
                    }

                    if let Err(e) = self.abstract_graph.delete_node(id.node_ref) {
                        log::error!("Abstract node failed to delete node: {}", e);
                    }
                } else {
                    let _ = removed_plugins.remove(&id);
                    log::warn!("Ignored request to remove plugin instance {:?}: Plugin is already removed.", id);
                }
            }
        }

        removed_plugins
    }

    pub fn connect_edge(&mut self, edge: &Edge) -> Result<(), ConnectEdgeError> {
        let src_entry =
            if let Some(entry) = self.shared_plugin_pool.plugins.get(&edge.src_plugin_id) {
                entry
            } else {
                return Err(ConnectEdgeError::SrcPluginDoesNotExist);
            };
        let dst_entry =
            if let Some(entry) = self.shared_plugin_pool.plugins.get(&edge.dst_plugin_id) {
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

    pub fn collect_save_state(&mut self) -> AudioGraphSaveState {
        log::trace!("Collecting audio graph save state...");

        let num_plugins = self.shared_plugin_pool.plugins.len();

        let mut plugin_save_states: Vec<PluginSaveState> = Vec::with_capacity(num_plugins);
        let mut edge_save_states: Vec<EdgeSaveState> = Vec::with_capacity(num_plugins * 3);

        let mut node_ref_to_index: FnvHashMap<NodeRef, usize> = FnvHashMap::default();
        node_ref_to_index.reserve(num_plugins);

        for (index, (plugin_id, plugin_entry)) in
            self.shared_plugin_pool.plugins.iter_mut().enumerate()
        {
            if let Some(_) = node_ref_to_index.insert(plugin_id.node_ref, index) {
                // In theory this should never happen.
                panic!("More than one plugin with node ref: {:?}", plugin_id.node_ref);
            }

            // These are the only two "plugins" without a save state.
            if plugin_id.node_ref == self.graph_in_node_id.node_ref
                || plugin_id.node_ref == self.graph_out_node_id.node_ref
            {
                continue;
            }

            plugin_save_states.push(plugin_entry.plugin_host.collect_save_state().unwrap());
        }

        // Iterate again to get all the edges.
        for plugin_id in self.shared_plugin_pool.plugins.keys() {
            for edge in self.abstract_graph.node_edges(plugin_id.node_ref).unwrap() {
                edge_save_states.push(EdgeSaveState {
                    edge_type: DefaultPortType::Audio,
                    src_plugin_i: *node_ref_to_index.get(&edge.src_node).unwrap(),
                    dst_plugin_i: *node_ref_to_index.get(&edge.dst_node).unwrap(),
                    src_channel: self.abstract_graph.port_ident(edge.src_port).unwrap().as_index()
                        as u16,
                    dst_channel: self.abstract_graph.port_ident(edge.dst_port).unwrap().as_index()
                        as u16,
                });
            }
        }

        AudioGraphSaveState { plugins: plugin_save_states, edges: edge_save_states }
    }

    pub fn reset(&mut self) {
        // Try to gracefully remove all existing plugins.
        for plugin_entry in self.shared_plugin_pool.plugins.values_mut() {
            // Don't remove the graph in/out "plugins".
            if plugin_entry.plugin_host.id == self.graph_in_node_id
                || plugin_entry.plugin_host.id == self.graph_out_node_id
            {
                continue;
            }

            plugin_entry.plugin_host.schedule_remove();
        }

        // TODO: Check that the audio thread is still alive.
        let audio_thread_is_alive = true;

        if audio_thread_is_alive {
            let start_time = std::time::Instant::now();

            const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

            // Wait for all plugins to be removed.
            while self.shared_plugin_pool.plugins.len() > 2 && start_time.elapsed() < TIMEOUT {
                std::thread::sleep(std::time::Duration::from_millis(10));

                let _ = self.on_idle(None);
            }

            if self.shared_plugin_pool.plugins.len() > 2 {
                log::error!("Timed out while removing all plugins");
            }
        }

        self.shared_schedule
            .set_new_schedule(Schedule::empty(self.max_frames as usize), &self.coll_handle);

        self.shared_plugin_pool.plugins.clear();
        self.shared_buffer_pool.remove_excess_audio_buffers(0, 0);

        self.abstract_graph = Graph::default();

        // ---  Add the graph input and graph output nodes to the graph  --------------------------

        let mut graph_in_node_id = PluginInstanceID {
            node_ref: audio_graph::NodeRef::new(0),
            format: shared_pool::PluginInstanceType::GraphInput,
            rdn: Shared::clone(&self.graph_in_rdn),
        };
        let mut graph_out_node_id = PluginInstanceID {
            node_ref: audio_graph::NodeRef::new(1),
            format: shared_pool::PluginInstanceType::GraphOutput,
            rdn: Shared::clone(&self.graph_out_rdn),
        };

        graph_in_node_id.node_ref = self.abstract_graph.node(graph_in_node_id.clone());
        graph_out_node_id.node_ref = self.abstract_graph.node(graph_out_node_id.clone());

        self.abstract_graph
            .set_node_ident(graph_in_node_id.node_ref, graph_in_node_id.clone())
            .unwrap();
        self.abstract_graph
            .set_node_ident(graph_out_node_id.node_ref, graph_out_node_id.clone())
            .unwrap();

        let graph_in_out_channel_refs: Vec<audio_graph::PortRef> = (0..self.graph_in_channels)
            .map(|i| {
                self.abstract_graph
                    .port(graph_in_node_id.node_ref, DefaultPortType::Audio, PortID::AudioOut(i))
                    .unwrap()
            })
            .collect();
        let graph_out_in_channel_refs: Vec<audio_graph::PortRef> = (0..self.graph_out_channels)
            .map(|i| {
                self.abstract_graph
                    .port(graph_out_node_id.node_ref, DefaultPortType::Audio, PortID::AudioIn(i))
                    .unwrap()
            })
            .collect();

        let _ = self.shared_plugin_pool.plugins.insert(
            graph_in_node_id.clone(),
            PluginInstanceHostEntry {
                plugin_host: PluginInstanceHost::new(
                    graph_in_node_id.clone(),
                    None,
                    None,
                    HostRequest::new(Shared::clone(&self.host_info)),
                ),
                audio_thread: None,
                audio_in_channel_refs: Vec::new(),
                audio_out_channel_refs: graph_in_out_channel_refs,
            },
        );
        let _ = self.shared_plugin_pool.plugins.insert(
            graph_out_node_id.clone(),
            PluginInstanceHostEntry {
                plugin_host: PluginInstanceHost::new(
                    graph_out_node_id.clone(),
                    None,
                    None,
                    HostRequest::new(Shared::clone(&self.host_info)),
                ),
                audio_thread: None,
                audio_in_channel_refs: graph_out_in_channel_refs,
                audio_out_channel_refs: Vec::new(),
            },
        );

        // ----------------------------------------------------------------------------------------
    }

    pub fn restore_from_save_state(
        &mut self,
        save_state: &AudioGraphSaveState,
        plugin_scanner: &mut PluginScanner,
        fallback_to_other_formats: bool,
    ) -> (Vec<NewPluginRes>, Vec<(PluginInstanceID, PluginEdges)>) {
        log::info!("Restoring audio graph from save state...");

        self.reset();

        let mut plugin_results: Vec<NewPluginRes> = Vec::with_capacity(save_state.plugins.len());

        for plugin_save_state in save_state.plugins.iter() {
            plugin_results.push(self.add_new_plugin_instance(
                plugin_save_state.clone(),
                plugin_scanner,
                plugin_save_state.activation_requested,
                fallback_to_other_formats,
            ));
        }

        for edge_save_state in save_state.edges.iter() {
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
    pub fn compile(&mut self) -> Result<(), GraphCompilerError> {
        match compiler::compile_graph(
            &mut self.shared_plugin_pool,
            &mut self.shared_buffer_pool,
            &mut self.abstract_graph,
            &self.graph_in_node_id,
            &self.graph_out_node_id,
            &mut self.verifier,
            &self.coll_handle,
        ) {
            Ok(schedule) => {
                log::debug!("Successfully compiled new schedule:\n{:?}", &schedule);

                self.shared_schedule.set_new_schedule(schedule, &self.coll_handle);
                Ok(())
            }
            Err(e) => {
                // Replace the current schedule with an emtpy one now that the graph
                // is in an invalid state.
                self.shared_schedule
                    .set_new_schedule(Schedule::empty(self.max_frames as usize), &self.coll_handle);
                Err(e)
            }
        }
    }

    pub fn on_idle(&mut self, event_tx: Option<&mut Sender<DSEngineEvent>>) -> bool {
        let mut plugins_to_remove: SmallVec<[PluginInstanceID; 4]> = SmallVec::new();

        let mut recompile_graph = false;

        // TODO: Optimize by using some kind of hashmap queue that only iterates over the
        // plugins that have a non-zero host request flag, instead of iterating over every
        // plugin every time?
        for plugin in self.shared_plugin_pool.plugins.values_mut() {
            let (res, modified_params) = plugin.plugin_host.on_idle(
                self.sample_rate,
                self.min_frames,
                self.max_frames,
                &self.coll_handle,
            );

            match res {
                OnIdleResult::Ok => {}
                OnIdleResult::PluginDeactivated => {
                    recompile_graph = true;

                    if let Some(event_tx) = event_tx.as_ref() {
                        event_tx
                            .send(DSEngineEvent::Plugin(PluginEvent::Deactivated {
                                plugin_id: plugin.plugin_host.id.clone(),
                                status: Ok(()),
                            }))
                            .unwrap();
                    }
                }
                OnIdleResult::PluginActivated(
                    plugin_audio_thread,
                    new_handle,
                    new_param_values,
                ) => {
                    plugin.audio_thread = Some(SharedPluginHostAudioThread::new(
                        plugin_audio_thread,
                        &self.coll_handle,
                    ));

                    update_plugin_ports(&mut self.abstract_graph, plugin, new_handle.audio_ports());

                    recompile_graph = true;

                    if let Some(event_tx) = event_tx.as_ref() {
                        event_tx
                            .send(DSEngineEvent::Plugin(PluginEvent::Activated {
                                plugin_id: plugin.plugin_host.id.clone(),
                                new_handle,
                                new_param_values,
                            }))
                            .unwrap();
                    }
                }
                OnIdleResult::PluginReadyToRemove => {
                    plugins_to_remove.push(plugin.plugin_host.id.clone());

                    // The user should have already been alerted of the plugin being removed
                    // in a previous `DSEngineEvent::AudioGraphModified` event.
                }
                OnIdleResult::PluginFailedToActivate(e) => {
                    recompile_graph = true;

                    if let Some(event_tx) = event_tx.as_ref() {
                        event_tx
                            .send(DSEngineEvent::Plugin(PluginEvent::Deactivated {
                                plugin_id: plugin.plugin_host.id.clone(),
                                status: Err(e),
                            }))
                            .unwrap();
                    }
                }
            }

            if modified_params.len() > 0 {
                if let Some(event_tx) = event_tx.as_ref() {
                    event_tx
                        .send(DSEngineEvent::Plugin(PluginEvent::ParamsModified {
                            plugin_id: plugin.plugin_host.id.clone(),
                            modified_params: modified_params.to_owned(),
                        }))
                        .unwrap();
                }
            }
        }

        for plugin in plugins_to_remove.iter() {
            let _ = self.shared_plugin_pool.plugins.remove(plugin);
        }

        recompile_graph
    }
}

fn update_plugin_ports(
    abstract_graph: &mut Graph<PluginInstanceID, PortID, DefaultPortType>,
    entry: &mut PluginInstanceHostEntry,
    audio_ports: &PluginAudioPortsExt,
) {
    let num_in_channels = audio_ports.total_in_channels();
    let num_out_channels = audio_ports.total_out_channels();

    if entry.audio_in_channel_refs.len() < num_in_channels {
        let old_len = entry.audio_in_channel_refs.len();
        for i in old_len as u16..num_in_channels as u16 {
            let port_ref = abstract_graph
                .port(entry.plugin_host.id.node_ref, DefaultPortType::Audio, PortID::AudioIn(i))
                .unwrap();

            entry.audio_in_channel_refs.push(port_ref);
        }
    } else if entry.audio_in_channel_refs.len() > num_in_channels {
        let num_to_remove = entry.audio_in_channel_refs.len() - num_in_channels;
        for _ in 0..num_to_remove {
            let port_ref = entry.audio_in_channel_refs.pop().unwrap();
            abstract_graph.delete_port(port_ref).unwrap();
        }
    }

    if entry.audio_out_channel_refs.len() < num_out_channels {
        let old_len = entry.audio_out_channel_refs.len();
        for i in old_len as u16..num_out_channels as u16 {
            let port_ref = abstract_graph
                .port(entry.plugin_host.id.node_ref, DefaultPortType::Audio, PortID::AudioOut(i))
                .unwrap();

            entry.audio_out_channel_refs.push(port_ref);
        }
    } else if entry.audio_out_channel_refs.len() > num_out_channels {
        let num_to_remove = entry.audio_out_channel_refs.len() - num_out_channels;
        for _ in 0..num_to_remove {
            let port_ref = entry.audio_out_channel_refs.pop().unwrap();
            abstract_graph.delete_port(port_ref).unwrap();
        }
    }
}

impl Drop for AudioGraph {
    fn drop(&mut self) {
        self.shared_schedule
            .set_new_schedule(Schedule::empty(self.max_frames as usize), &self.coll_handle);
    }
}

#[derive(Debug)]
pub enum PluginActivationStatus {
    /// This means the plugin successfully activated and returned
    /// its new audio/event port configuration and its new
    /// parameter configuration.
    Activated { new_handle: PluginHandle, new_param_values: FnvHashMap<ParamID, f64> },

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

impl PluginEdges {
    pub fn emtpy() -> Self {
        Self { incoming: SmallVec::new(), outgoing: SmallVec::new() }
    }
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
