use audio_graph::{DefaultPortType, Graph, NodeRef, PortRef};
use basedrop::Shared;

pub(crate) mod audio_buffer_pool;
pub(crate) mod plugin_pool;

pub mod schedule;

use audio_buffer_pool::AudioBufferPool;
use plugin_pool::PluginInstancePool;

pub use plugin_pool::PluginInstanceID;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PortID(pub u64);

use crate::{
    host::HostInfo,
    plugin::{PluginAudioThread, PluginFactory, PluginMainThread},
    plugin_scanner::{NewPluginInstanceError, PluginScanner},
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
        rdn: &str,
        plugin_scanner: &mut PluginScanner,
    ) -> Result<PluginInstanceID, NewPluginInstanceError> {
        match plugin_scanner.new_instance(rdn, Shared::clone(&self.host_info)) {
            Ok((plugin, debug_name, plugin_type)) => {
                let instance_id =
                    self.plugin_pool.add_graph_plugin(plugin, plugin_type, debug_name);

                let node_ref = self.graph.node(instance_id.node_id);

                // If this isn't right then I did something wrong.
                assert_eq!(node_ref, instance_id.node_id);

                Ok(instance_id)
            }
            Err(e) => Err(e),
        }
    }
}
