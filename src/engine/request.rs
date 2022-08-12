use dropseed_plugin_api::{DSPluginSaveState, PluginInstanceID};

use crate::graph::{Edge, PortType};

#[derive(Debug, Clone)]
pub struct ModifyGraphRequest {
    /// Any new plugin instances to add.
    pub add_plugin_instances: Vec<DSPluginSaveState>,

    /// Any plugins to remove.
    pub remove_plugin_instances: Vec<PluginInstanceID>,

    /// Any new connections between plugins to add.
    pub connect_new_edges: Vec<EdgeReq>,

    /// Any connections between plugins to remove.
    pub disconnect_edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginIDReq {
    /// Use an existing plugin in the audio graph.
    Existing(PluginInstanceID),
    /// Use one of the new plugins defined in `ModifyGraphRequest::add_plugin_instances`
    /// (the index into that Vec).
    Added(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum EdgeReqPortID {
    /// Use the main port.
    ///
    /// This can be useful if you don't know the layout of the plugin's ports yet
    /// (because the plugin hasn't been added to the graph yet and activated).
    Main,
    /// Use the port with this specific stable ID.
    StableID(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeReq {
    pub edge_type: PortType,

    pub src_plugin_id: PluginIDReq,
    pub dst_plugin_id: PluginIDReq,

    pub src_port_id: EdgeReqPortID,
    pub src_port_channel: u16,

    pub dst_port_id: EdgeReqPortID,
    pub dst_port_channel: u16,

    /// If true, then the engine should log the error if it failed to connect this edge
    /// for any reason.
    ///
    /// If false, then the engine should not log the error if it failed to connect this
    /// edge for any reason. This can be useful in the common case where when adding a
    /// new plugin to the graph, and you don't know the layout of the plugin's ports yet
    /// (because it hasn't been added to the graph yet and activated), yet you still want
    /// to try and connect any main stereo inputs/outputs to the graph.
    pub log_error_on_fail: bool,
}
