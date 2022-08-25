use audio_graph::NodeID;
use fnv::FnvHashMap;

use crate::plugin_host::PluginHostMainThread;

pub(crate) struct PluginHostPool {
    pub pool: FnvHashMap<NodeID, PluginHostMainThread>,
}

impl PluginHostPool {
    pub fn new() -> Self {
        Self { pool: FnvHashMap::default() }
    }
}
