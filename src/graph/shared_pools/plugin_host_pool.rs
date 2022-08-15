use dropseed_plugin_api::PluginInstanceID;
use fnv::FnvHashMap;

use crate::plugin_host::PluginHostMainThread;

pub(crate) struct PluginHostPool {
    pub pool: FnvHashMap<PluginInstanceID, PluginHostMainThread>,
}

impl PluginHostPool {
    pub fn new() -> Self {
        Self { pool: FnvHashMap::default() }
    }
}
