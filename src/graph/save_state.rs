use crate::plugin::PluginSaveState;

use super::PortID;

#[derive(Debug, Clone, Copy)]
pub struct EdgeSaveState {
    pub src_plugin_i: usize,
    pub dst_plugin_i: usize,
    pub src_port: PortID,
    pub dst_port: PortID,
}

#[derive(Debug, Clone)]
pub struct AudioGraphSaveState {
    pub plugins: Vec<PluginSaveState>,
    pub edges: Vec<EdgeSaveState>,
}

impl Default for AudioGraphSaveState {
    fn default() -> Self {
        AudioGraphSaveState { plugins: Vec::new(), edges: Vec::new() }
    }
}
