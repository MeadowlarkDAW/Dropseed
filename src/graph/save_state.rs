use crate::plugin::PluginSaveState;

use super::PortType;

#[derive(Debug, Clone, Copy)]
pub struct EdgeSaveState {
    pub edge_type: PortType,
    pub src_plugin_i: usize,
    pub dst_plugin_i: usize,
    pub src_port_stable_id: u32,
    pub src_port_channel: u16,
    pub dst_port_stable_id: u32,
    pub dst_port_channel: u16,
}

#[derive(Debug, Clone, Default)]
pub struct AudioGraphSaveState {
    pub plugins: Vec<PluginSaveState>,
    pub edges: Vec<EdgeSaveState>,
}
