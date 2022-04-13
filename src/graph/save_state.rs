use crate::plugin::PluginSaveState;
use audio_graph::DefaultPortType;

#[derive(Debug, Clone, Copy)]
pub struct EdgeSaveState {
    pub edge_type: DefaultPortType,
    pub src_plugin_i: usize,
    pub dst_plugin_i: usize,
    pub src_port: u16,
    pub dst_port: u16,
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
