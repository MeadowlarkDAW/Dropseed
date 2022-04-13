use std::error::Error;

use audio_graph::{DefaultPortType, Graph, NodeRef, PortRef};
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
    plugin::{PluginAudioThread, PluginFactory, PluginMainThread, PluginSaveState},
    plugin_scanner::{NewPluginInstanceError, PluginFormat, PluginScanner, ScannedPluginKey},
};

pub struct AudioGraph {
    plugin_pool: PluginInstancePool,
    audio_buffer_pool: AudioBufferPool,
    host_info: Shared<HostInfo>,

    graph: Graph<NodeRef, PortID, DefaultPortType>,
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
        let (plugin, debug_name, format, res) = match plugin_scanner.new_instance(
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
                    src_port: self.graph.port_ident(edge.src_port).unwrap().as_index() as u16,
                    dst_port: self.graph.port_ident(edge.dst_port).unwrap().as_index() as u16,
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

            match edge_save_state.edge_type {
                DefaultPortType::Audio => {
                    let src_plugin_id = &plugin_ids[edge_save_state.src_plugin_i];
                    let dst_plugin_id = &plugin_ids[edge_save_state.dst_plugin_i];

                    let src_port_refs =
                        self.plugin_pool.get_audio_out_port_refs(src_plugin_id).unwrap();
                    let dst_port_refs =
                        self.plugin_pool.get_audio_in_port_refs(dst_plugin_id).unwrap();

                    if usize::from(edge_save_state.src_port) >= src_port_refs.len() {
                        edge_errors.push((
                            i,
                            ConnectEdgeError::SrcPortOutOfBounds(
                                edge_save_state.src_port,
                                src_port_refs.len() as u16,
                            ),
                        ));
                        continue;
                    }
                    if usize::from(edge_save_state.dst_port) >= dst_port_refs.len() {
                        edge_errors.push((
                            i,
                            ConnectEdgeError::DstPortOutOfBounds(
                                edge_save_state.dst_port,
                                dst_port_refs.len() as u16,
                            ),
                        ));
                        continue;
                    }

                    let src_port_ref = src_port_refs[usize::from(edge_save_state.src_port)];
                    let dst_port_ref = dst_port_refs[usize::from(edge_save_state.dst_port)];

                    match self.graph.connect(src_port_ref, dst_port_ref) {
                        Ok(()) => {
                            log::trace!(
                                "Successfully connected edge: {:?}",
                                Edge {
                                    edge_type: DefaultPortType::Audio,
                                    src_plugin_id: src_plugin_id.clone(),
                                    dst_plugin_id: dst_plugin_id.clone(),
                                    src_plugin_port: edge_save_state.src_port,
                                    dst_plugin_port: edge_save_state.dst_port,
                                }
                            )
                        }
                        Err(e) => {
                            if let audio_graph::Error::Cycle = e {
                                edge_errors.push((i, ConnectEdgeError::Cycle));
                            } else {
                                log::error!("Unexpected edge connect error: {}", e);
                                edge_errors.push((i, ConnectEdgeError::Unkown));
                            }
                            continue;
                        }
                    }
                }
                DefaultPortType::Event => {
                    todo!()
                }
            }
        }

        let plugin_errors = if !plugin_errors.is_empty() { Err(plugin_errors) } else { Ok(()) };
        let edge_errors = if !edge_errors.is_empty() { Err(edge_errors) } else { Ok(()) };

        (plugin_ids, plugin_errors, edge_errors)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub edge_type: DefaultPortType,

    pub src_plugin_id: PluginInstanceID,
    pub dst_plugin_id: PluginInstanceID,

    pub src_plugin_port: u16,
    pub dst_plugin_port: u16,
}

#[derive(Debug)]
pub enum ConnectEdgeError {
    SrcPluginDoesNotExist,
    DstPluginDoesNotExist,
    SrcPortOutOfBounds(u16, u16),
    DstPortOutOfBounds(u16, u16),
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
            ConnectEdgeError::SrcPortOutOfBounds(i, max_i) => {
                write!(f, "Could not add edge to graph: Source plugin with {} output ports does not have port with index {}", i, max_i)
            }
            ConnectEdgeError::DstPortOutOfBounds(i, max_i) => {
                write!(f, "Could not add edge to graph: Destination plugin with {} input ports does not have port with index {}", i, max_i)
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
