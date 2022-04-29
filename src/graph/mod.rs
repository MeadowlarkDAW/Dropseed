use audio_graph::{DefaultPortType, Graph, NodeRef};
use basedrop::Shared;
use fnv::FnvHashMap;
use rusty_daw_core::SampleRate;
use smallvec::SmallVec;
use std::error::Error;

pub(crate) mod audio_buffer_pool;
pub(crate) mod plugin_pool;

mod compiler;
mod save_state;
mod schedule;
mod verifier;

use audio_buffer_pool::AudioBufferPool;
use plugin_pool::PluginInstancePool;
use schedule::Schedule;
use verifier::Verifier;

pub use compiler::GraphCompilerError;
pub use plugin_pool::{PluginActivatedInfo, PluginInstanceID};
pub use save_state::{AudioGraphSaveState, EdgeSaveState};
pub use schedule::SharedSchedule;
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
    host::HostInfo,
    plugin::PluginSaveState,
    plugin_scanner::{NewPluginInstanceError, PluginScanner},
};

use self::compiler::compile_graph;

pub(crate) struct AudioGraph {
    plugin_pool: PluginInstancePool,
    audio_buffer_pool: AudioBufferPool,
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
    min_frames: usize,
    max_frames: usize,
}

impl AudioGraph {
    pub(crate) fn new(
        coll_handle: basedrop::Handle,
        host_info: Shared<HostInfo>,
        graph_in_channels: u16,
        graph_out_channels: u16,
        sample_rate: SampleRate,
        min_frames: usize,
        max_frames: usize,
    ) -> (Self, SharedSchedule) {
        assert!(graph_in_channels > 0);
        assert!(graph_out_channels > 0);

        let failed_plugin_debug_name = Shared::new(&coll_handle, String::from("failed_plugin"));

        let mut abstract_graph = Graph::default();
        let (plugin_pool, graph_in_node_id, graph_out_node_id) = PluginInstancePool::new(
            &mut abstract_graph,
            graph_in_channels,
            graph_out_channels,
            coll_handle.clone(),
            Shared::clone(&host_info),
            sample_rate,
            min_frames,
            max_frames,
        );

        let (shared_schedule, shared_schedule_clone) = SharedSchedule::new(
            Schedule::empty(max_frames, Shared::clone(&host_info)),
            &coll_handle,
        );

        let mut new_self = Self {
            plugin_pool,
            audio_buffer_pool: AudioBufferPool::new(coll_handle.clone(), max_frames),
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

        new_self.connect_edge(&Edge {
            edge_type: DefaultPortType::Audio,
            src_plugin_id: new_self.graph_in_node_id.clone(),
            dst_plugin_id: new_self.graph_out_node_id.clone(),
            src_channel: 0,
            dst_channel: 0,
        }).unwrap();

        /*
        new_self.connect_edge(&Edge {
            edge_type: DefaultPortType::Audio,
            src_plugin_id: new_self.graph_in_node_id.clone(),
            dst_plugin_id: new_self.graph_out_node_id.clone(),
            src_channel: 1,
            dst_channel: 1,
        }).unwrap();
        */

        (
            new_self,
            shared_schedule_clone,
        )
    }

    pub fn graph_in_node_id(&self) -> &PluginInstanceID {
        &self.graph_in_node_id
    }

    pub fn graph_out_node_id(&self) -> &PluginInstanceID {
        &self.graph_out_node_id
    }

    pub fn add_new_plugin_instance(
        &mut self,
        save_state: &PluginSaveState,
        plugin_scanner: &mut PluginScanner,
        fallback_to_other_formats: bool,
    ) -> (PluginInstanceID, Result<(), NewPluginInstanceError>) {
        let (plugin, debug_name, format, res) = match plugin_scanner.create_plugin(
            &save_state.key,
            Shared::clone(&self.host_info),
            fallback_to_other_formats,
        ) {
            Ok((plugin, debug_name, format)) => {
                log::debug!(
                    "Loaded plugin {:?} successfully with format {:?}",
                    &save_state.key,
                    &format
                );

                (Some(plugin), debug_name, format, Ok(()))
            }
            Err(e) => {
                log::error!("Failed to load plugin {:?} from save state: {}", &save_state.key, e);

                (None, Shared::clone(&self.failed_plugin_debug_name), save_state.key.format, Err(e))
            }
        };

        let mut new_save_state = save_state.clone();
        new_save_state.key.format = format;

        let instance_id = self.plugin_pool.add_graph_plugin(
            plugin,
            new_save_state,
            debug_name,
            &mut self.abstract_graph,
            false,
        );

        (instance_id, res)
    }

    /// Remove the given plugins from the graph.
    ///
    /// This will also automatically disconnect all edges that were connected to these
    /// plugins.
    ///
    /// Requests to remove the "graph input/output" nodes with the IDs `AudioGraph::graph_in_node_id()`
    /// and `AudioGraph::graph_out_node_id()` will be ignored.
    pub fn remove_plugin_instances(&mut self, plugin_ids: &[PluginInstanceID]) {
        for id in plugin_ids.iter() {
            if id == &self.graph_in_node_id || id == &self.graph_out_node_id {
                if id == &self.graph_in_node_id {
                    log::warn!("Ignored request to remove the graph in node from the audio graph");
                } else {
                    log::warn!("Ignored request to remove the graph in node from the audio graph");
                }
                continue;
            }

            self.plugin_pool.remove_graph_plugin(id, &mut self.abstract_graph);
        }
    }

    /// Try to connect multiple edges at once. Useful for connecting ports with multiple
    /// channels (i.e. stereo audio ports).
    ///
    /// If any of the given edges fails to connect, then none of the given edges will be connected.
    pub fn try_connect_edges(&mut self, edges: &[Edge]) -> Result<(), (usize, ConnectEdgeError)> {
        let mut res = Ok(());
        for (i, edge) in edges.iter().enumerate() {
            if let Err(e) = self.connect_edge(edge) {
                res = Err((i, e));
                break;
            }
        }

        // If there was an error, disconnect any edges that were connected.
        if let Err((err_i, _)) = &res {
            for i in 0..*err_i {
                self.disconnect_edge(&edges[i]);
            }
        }

        res
    }

    pub fn connect_edge(&mut self, edge: &Edge) -> Result<(), ConnectEdgeError> {
        match edge.edge_type {
            DefaultPortType::Audio => {
                let src_channel_refs =
                    match self.plugin_pool.get_audio_out_channel_refs(&edge.src_plugin_id) {
                        Ok(c) => c,
                        Err(_) => {
                            return Err(ConnectEdgeError::SrcPluginDoesNotExist);
                        }
                    };

                let dst_channel_refs =
                    match self.plugin_pool.get_audio_in_channel_refs(&edge.dst_plugin_id) {
                        Ok(c) => c,
                        Err(_) => {
                            return Err(ConnectEdgeError::DstPluginDoesNotExist);
                        }
                    };

                if usize::from(edge.src_channel) >= src_channel_refs.len() {
                    return Err(ConnectEdgeError::SrcChannelOutOfBounds(
                        edge.src_channel,
                        src_channel_refs.len() as u16,
                    ));
                }
                if usize::from(edge.dst_channel) >= dst_channel_refs.len() {
                    return Err(ConnectEdgeError::DstChannelOutOfBounds(
                        edge.dst_channel,
                        dst_channel_refs.len() as u16,
                    ));
                }

                let src_channel_ref = src_channel_refs[usize::from(edge.src_channel)];
                let dst_channel_ref = dst_channel_refs[usize::from(edge.dst_channel)];

                match self.abstract_graph.connect(src_channel_ref, dst_channel_ref) {
                    Ok(()) => {
                        log::debug!(
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

    pub fn disconnect_edge(&mut self, edge: &Edge) {
        let mut found_ports = None;
        if let Ok(edges) = self.abstract_graph.node_edges(edge.src_plugin_id.node_id) {
            // Find the corresponding edge.
            for e in edges.iter() {
                if e.dst_node != edge.dst_plugin_id.node_id {
                    continue;
                }
                if e.src_node != edge.src_plugin_id.node_id {
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
                log::debug!("Successfully disconnected edge: {:?}", edge);
            }
        } else {
            log::warn!("Could not disconnect edge: {:?}: Edge was not found in the graph", edge);
        }
    }

    pub fn get_plugin_edges(&self, id: &PluginInstanceID) -> Result<PluginEdges, ()> {
        if let Ok(edges) = self.abstract_graph.node_edges(id.node_id) {
            let mut incoming: SmallVec<[Edge; 8]> = SmallVec::new();
            let mut outgoing: SmallVec<[Edge; 8]> = SmallVec::new();

            for edge in edges.iter() {
                let src_channel =
                    self.abstract_graph.port_ident(edge.src_port).unwrap().as_index() as u16;
                let dst_channel =
                    self.abstract_graph.port_ident(edge.dst_port).unwrap().as_index() as u16;

                if edge.src_node == id.node_id {
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
    ) -> AudioGraphRestoredInfo {
        log::info!("Restoring audio graph from save state...");

        self.abstract_graph = Graph::default();
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
        self.plugin_pool = plugin_pool;
        self.graph_in_node_id = graph_in_id;
        self.graph_out_node_id = graph_out_id;

        let mut plugin_ids: Vec<PluginInstanceID> =
            Vec::with_capacity(save_state.plugins.len() + 2);
        let mut plugin_errors: Vec<(usize, NewPluginInstanceError)> = Vec::new();
        let mut edge_errors: Vec<(usize, ConnectEdgeError)> = Vec::new();

        plugin_ids.push(self.graph_in_node_id.clone());
        plugin_ids.push(self.graph_out_node_id.clone());

        for (i, plugin_save_state) in save_state.plugins.iter().enumerate() {
            let (id, res) = self.add_new_plugin_instance(
                &plugin_save_state,
                plugin_scanner,
                fallback_to_other_formats,
            );

            plugin_ids.push(id);

            if let Err(e) = res {
                plugin_errors.push((i, e));
            }
        }

        for (i, edge_save_state) in save_state.edges.iter().enumerate() {
            if edge_save_state.src_plugin_i >= plugin_ids.len() {
                edge_errors.push((i, ConnectEdgeError::SrcPluginDoesNotExist));
                continue;
            }
            if edge_save_state.dst_plugin_i >= plugin_ids.len() {
                edge_errors.push((i, ConnectEdgeError::DstPluginDoesNotExist));
                continue;
            }

            let edge = Edge {
                edge_type: edge_save_state.edge_type,
                src_plugin_id: plugin_ids[edge_save_state.src_plugin_i].clone(),
                dst_plugin_id: plugin_ids[edge_save_state.dst_plugin_i].clone(),
                src_channel: edge_save_state.src_channel,
                dst_channel: edge_save_state.dst_channel,
            };

            if let Err(e) = self.connect_edge(&edge) {
                edge_errors.push((i, e));
            }
        }

        let mut plugin_instances: Vec<(PluginInstanceID, PluginEdges, PluginSaveState)> =
            Vec::with_capacity(self.plugin_pool.num_plugins());
        for plugin_id in self.plugin_pool.iter_plugin_ids() {
            if plugin_id.node_id == self.graph_in_node_id.node_id
                || plugin_id.node_id == self.graph_out_node_id.node_id
            {
                continue;
            }

            let edges = self.get_plugin_edges(plugin_id).unwrap();
            let save_state =
                self.plugin_pool.get_graph_plugin_save_state(plugin_id.node_id).unwrap().clone();

            plugin_instances.push((plugin_id.clone(), edges, save_state));
        }

        let new_save_state = self.collect_save_state();

        AudioGraphRestoredInfo {
            requested_save_state: save_state.clone(),
            save_state: new_save_state,
            plugin_instances,
            plugin_errors,
            edge_errors,
        }
    }

    /// Compile the audio graph into a schedule that is sent to the audio thread.
    ///
    /// If an error is returned then the graph **MUST** be restored with the previous
    /// working save state.
    pub(crate) fn compile(&mut self) -> Result<(), GraphCompilerError> {
        match compile_graph(
            &mut self.plugin_pool,
            &mut self.audio_buffer_pool,
            &mut self.abstract_graph,
            &self.graph_in_node_id,
            &self.graph_out_node_id,
            &mut self.verifier,
        ) {
            Ok(schedule) => {
                log::debug!("Successfully compiled new schedule: {:?}", &schedule);

                self.shared_schedule.set_new_schedule(schedule, &self.coll_handle);
                Ok(())
            }
            Err(e) => {
                // Replace the current schedule with an emtpy one now that the graph
                // is in an invalid state.
                self.shared_schedule.set_new_schedule(
                    Schedule::empty(self.max_frames, Shared::clone(&self.host_info)),
                    &self.coll_handle,
                );
                Err(e)
            }
        }
    }

    pub(crate) fn on_main_thread(&mut self) -> SmallVec<[(PluginInstanceID, bool); 4]> {
        self.plugin_pool.on_main_thread(&mut self.abstract_graph)
    }
}

#[derive(Debug)]
pub struct AudioGraphRestoredInfo {
    /// The save state the audio graph attempted to restore from.
    pub requested_save_state: AudioGraphSaveState,

    /// The new save state of this audio graph.
    pub save_state: AudioGraphSaveState,

    /// All of the plugin instances in the audio graph, along with their edges
    /// and save state.
    pub plugin_instances: Vec<(PluginInstanceID, PluginEdges, PluginSaveState)>,

    /// All of the plugins which failed to load correctly
    ///
    /// (index into `requested_save_state.plugins`, error)
    pub plugin_errors: Vec<(usize, NewPluginInstanceError)>,

    /// All of the edges which failed to connect
    ///
    /// (index into `requested_save_state.edges`, error)
    pub edge_errors: Vec<(usize, ConnectEdgeError)>,
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
