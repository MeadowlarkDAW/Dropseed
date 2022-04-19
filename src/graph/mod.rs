use std::error::Error;

use audio_graph::{DefaultPortType, Graph, NodeRef};
use basedrop::Shared;
use fnv::FnvHashMap;

pub(crate) mod audio_buffer_pool;
pub(crate) mod plugin_pool;

mod save_state;

pub mod schedule;

use audio_buffer_pool::AudioBufferPool;
use plugin_pool::PluginInstancePool;

pub use plugin_pool::PluginInstanceID;
pub use save_state::{AudioGraphSaveState, EdgeSaveState};
use smallvec::SmallVec;

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

pub struct AudioGraph {
    plugin_pool: PluginInstancePool,
    audio_buffer_pool: AudioBufferPool,
    host_info: Shared<HostInfo>,

    graph: Graph<PluginInstanceID, PortID, DefaultPortType>,
    coll_handle: basedrop::Handle,

    failed_plugin_debug_name: Shared<String>,
}

impl AudioGraph {
    pub(crate) fn new(
        coll_handle: basedrop::Handle,
        max_block_size: usize,
        host_info: Shared<HostInfo>,
    ) -> Self {
        let failed_plugin_debug_name = Shared::new(&coll_handle, String::from("failed_plugin"));

        Self {
            plugin_pool: PluginInstancePool::new(coll_handle.clone(), Shared::clone(&host_info)),
            audio_buffer_pool: AudioBufferPool::new(coll_handle.clone(), max_block_size),
            host_info,
            graph: Graph::default(),
            coll_handle,
            failed_plugin_debug_name,
        }
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
                log::trace!(
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

        let instance_id = self.plugin_pool.add_graph_plugin(
            plugin,
            save_state.clone(),
            debug_name,
            &mut self.graph,
            format,
            false,
        );

        (instance_id, res)
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

                match self.graph.connect(src_channel_ref, dst_channel_ref) {
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

    pub fn remove_edge(&mut self, edge: &Edge) {
        let mut found_ports = None;
        if let Ok(edges) = self.graph.node_edges(edge.src_plugin_id.node_id) {
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
                if self.graph.port_ident(e.src_port).unwrap().as_index()
                    != usize::from(edge.src_channel)
                {
                    continue;
                }
                if self.graph.port_ident(e.dst_port).unwrap().as_index()
                    != usize::from(edge.dst_channel)
                {
                    continue;
                }

                found_ports = Some((e.src_port, e.dst_port));
                break;
            }
        }

        if let Some((src_port, dst_port)) = found_ports {
            if let Err(e) = self.graph.disconnect(src_port, dst_port) {
                log::error!("Unexpected error while disconnecting edge {:?}: {}", edge, e);
            } else {
                log::trace!("Successfully disconnected edge: {:?}", edge);
            }
        } else {
            log::warn!("Could not disconnect edge: {:?}: Edge was not found in the graph", edge);
        }
    }

    pub fn get_plugin_edges(&self, id: &PluginInstanceID) -> Result<PluginEdges, ()> {
        if let Ok(edges) = self.graph.node_edges(id.node_id) {
            let mut incoming: SmallVec<[Edge; 8]> = SmallVec::new();
            let mut outgoing: SmallVec<[Edge; 8]> = SmallVec::new();

            for edge in edges.iter() {
                let src_channel = self.graph.port_ident(edge.src_port).unwrap().as_index() as u16;
                let dst_channel = self.graph.port_ident(edge.dst_port).unwrap().as_index() as u16;

                if edge.src_node == id.node_id {
                    outgoing.push(Edge {
                        edge_type: edge.port_type,
                        src_plugin_id: id.clone(),
                        dst_plugin_id: self.graph.node_ident(edge.dst_node).unwrap().clone(),
                        src_channel,
                        dst_channel,
                    });
                } else {
                    incoming.push(Edge {
                        edge_type: edge.port_type,
                        src_plugin_id: self.graph.node_ident(edge.src_node).unwrap().clone(),
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

    pub fn collect_save_state(&self) -> AudioGraphSaveState {
        let mut plugins: Vec<PluginSaveState> = Vec::with_capacity(self.plugin_pool.num_plugins());
        let mut edges: Vec<EdgeSaveState> = Vec::with_capacity(self.plugin_pool.num_plugins() * 3);

        let mut node_id_to_index: FnvHashMap<NodeRef, usize> = FnvHashMap::default();
        node_id_to_index.reserve(self.plugin_pool.num_plugins());

        for (index, node_id) in self.plugin_pool.iter_plugin_ids().enumerate() {
            if let Some(_) = node_id_to_index.insert(node_id, index) {
                // In theory this should never happen.
                panic!("More than one plugin with node id: {:?}", node_id);
            }

            let save_state = self.plugin_pool.get_graph_plugin_save_state(node_id).unwrap().clone();
            plugins.push(save_state);
        }

        // Iterate again to get all the edges.
        for node_id in self.plugin_pool.iter_plugin_ids() {
            for edge in self.graph.node_edges(node_id).unwrap() {
                edges.push(EdgeSaveState {
                    edge_type: DefaultPortType::Audio,
                    src_plugin_i: *node_id_to_index.get(&edge.src_node).unwrap(),
                    dst_plugin_i: *node_id_to_index.get(&edge.dst_node).unwrap(),
                    src_channel: self.graph.port_ident(edge.src_port).unwrap().as_index() as u16,
                    dst_channel: self.graph.port_ident(edge.dst_port).unwrap().as_index() as u16,
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
    ) -> (
        Vec<PluginInstanceID>,
        Result<(), Vec<(usize, NewPluginInstanceError)>>,
        Result<(), Vec<(usize, ConnectEdgeError)>>,
    ) {
        self.graph = Graph::default();
        self.plugin_pool =
            PluginInstancePool::new(self.coll_handle.clone(), Shared::clone(&self.host_info));

        let mut plugin_ids: Vec<PluginInstanceID> = Vec::with_capacity(save_state.plugins.len());
        let mut plugin_errors: Vec<(usize, NewPluginInstanceError)> = Vec::new();
        let mut edge_errors: Vec<(usize, ConnectEdgeError)> = Vec::new();

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

        let plugin_errors = if !plugin_errors.is_empty() { Err(plugin_errors) } else { Ok(()) };
        let edge_errors = if !edge_errors.is_empty() { Err(edge_errors) } else { Ok(()) };

        (plugin_ids, plugin_errors, edge_errors)
    }
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
