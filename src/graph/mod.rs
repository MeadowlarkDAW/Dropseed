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
}

impl AudioGraph {
    pub(crate) fn new(
        coll_handle: basedrop::Handle,
        max_block_size: usize,
        host_info: Shared<HostInfo>,
    ) -> Self {
        Self {
            plugin_pool: PluginInstancePool::new(coll_handle.clone()),
            audio_buffer_pool: AudioBufferPool::new(coll_handle, max_block_size),
            host_info,
            graph: Graph::default(),
        }
    }

    pub fn add_new_plugin_instance(
        &mut self,
        key: &ScannedPluginKey,
        plugin_scanner: &mut PluginScanner,
        fallback_to_other_formats: bool,
    ) -> Result<PluginInstanceID, NewPluginInstanceError> {
        match plugin_scanner.new_instance(
            key,
            Shared::clone(&self.host_info),
            fallback_to_other_formats,
        ) {
            Ok((plugin, debug_name, format)) => {
                let instance_id = self.plugin_pool.add_graph_plugin(plugin, key, debug_name, false);

                let node_ref = self.graph.node(instance_id.node_id);

                // If this isn't right then I did something wrong.
                assert_eq!(node_ref, instance_id.node_id);

                Ok(instance_id)
            }
            Err(e) => Err(e),
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
                    src_plugin_i: *node_id_to_index.get(&edge.src_node).unwrap(),
                    dst_plugin_i: *node_id_to_index.get(&edge.dst_node).unwrap(),
                    src_port: *self.graph.port_ident(edge.src_port).unwrap(),
                    dst_port: *self.graph.port_ident(edge.dst_port).unwrap(),
                });
            }
        }

        AudioGraphSaveState { plugins, edges }
    }

    pub fn restore_from_save_state(&mut self, save_state: &AudioGraphSaveState) -> Result<(), ()> {
        self.graph = Graph::default();

        todo!()
    }
}
