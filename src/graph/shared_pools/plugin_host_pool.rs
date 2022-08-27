use audio_graph::NodeID;
use dropseed_plugin_api::PluginInstanceID;
use fnv::FnvHashMap;

use crate::plugin_host::PluginHostMainThread;

pub(crate) struct PluginHostPool {
    pool: FnvHashMap<PluginInstanceID, PluginHostMainThread>,
    node_id_to_plugin_id: FnvHashMap<NodeID, PluginInstanceID>,
}

impl PluginHostPool {
    pub fn new() -> Self {
        Self { pool: FnvHashMap::default(), node_id_to_plugin_id: FnvHashMap::default() }
    }

    pub fn insert(
        &mut self,
        id: PluginInstanceID,
        host: PluginHostMainThread,
    ) -> Option<PluginHostMainThread> {
        let old_host = self.pool.insert(id.clone(), host);
        self.node_id_to_plugin_id.insert(id._node_id().into(), id);
        old_host
    }

    pub fn remove(&mut self, id: &PluginInstanceID) -> Option<PluginHostMainThread> {
        self.node_id_to_plugin_id.remove(&id._node_id().into());
        self.pool.remove(id)
    }

    pub fn get(&self, id: &PluginInstanceID) -> Option<&PluginHostMainThread> {
        self.pool.get(id)
    }

    pub fn get_mut(&mut self, id: &PluginInstanceID) -> Option<&mut PluginHostMainThread> {
        self.pool.get_mut(id)
    }

    pub fn get_by_node_id(&self, id: &NodeID) -> Option<&PluginHostMainThread> {
        self.node_id_to_plugin_id.get(id).map(|id| self.pool.get(id).unwrap())
    }

    pub fn num_plugins(&self) -> usize {
        self.pool.len()
    }

    pub fn iter_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut PluginHostMainThread> {
        self.pool.values_mut()
    }

    pub fn clear(&mut self) {
        self.pool.clear();
        self.node_id_to_plugin_id.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }
}
