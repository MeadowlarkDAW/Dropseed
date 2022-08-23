use audio_graph::{AudioGraphHelper, NodeID, PortID, TypeIdx};
use basedrop::Shared;
use fnv::FnvHashSet;
use meadowlark_core_types::time::SampleRate;
use meadowlark_core_types::time::Seconds;
use smallvec::SmallVec;

mod compiler;

pub mod error;

pub(crate) mod shared_pools;

use dropseed_plugin_api::transport::TempoMap;
use dropseed_plugin_api::{DSPluginSaveState, PluginInstanceID, PluginInstanceType};

use crate::engine::request::{EdgeReq, EdgeReqPortID};
use crate::engine::{NewPluginRes, OnIdleEvent, PluginStatus};
use crate::plugin_host::{OnIdleResult, PluginHostMainThread};
use crate::plugin_scanner::PluginScanner;
use crate::processor_schedule::tasks::{TransportHandle, TransportTask};
use crate::processor_schedule::ProcessorSchedule;
use crate::utils::thread_id::SharedThreadIDs;

use compiler::verifier::Verifier;
use shared_pools::{GraphSharedPools, SharedProcessorSchedule};

use error::{ConnectEdgeError, ConnectEdgeErrorType, GraphCompilerError};

/// A default port type for general purpose applications
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortType {
    /// Audio ports
    Audio = 0,
    Note = 1,
    ParamAutomation = 2,
}

impl PortType {
    pub fn from_type_idx(p: TypeIdx) -> Option<Self> {
        match p.0 {
            0 => Some(PortType::Audio),
            1 => Some(PortType::Note),
            2 => Some(PortType::ParamAutomation),
            _ => None,
        }
    }

    pub fn as_type_idx(&self) -> TypeIdx {
        let t: u32 = self.into();
        TypeIdx(t)
    }
}

impl Default for PortType {
    fn default() -> Self {
        PortType::Audio
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelID {
    pub(crate) stable_id: u32,
    pub(crate) port_type: PortType,
    pub(crate) is_input: bool,
    pub(crate) channel: u16,
}

pub(crate) struct AudioGraph {
    shared_pools: GraphSharedPools,
    verifier: Verifier,

    graph_helper: AudioGraphHelper,
    coll_handle: basedrop::Handle,

    graph_in_channels: u16,
    graph_out_channels: u16,

    graph_in_id: PluginInstanceID,
    graph_out_id: PluginInstanceID,
    graph_in_audio_out_port_refs: Vec<PortID>,
    graph_out_audio_in_port_refs: Vec<PortID>,

    temp_rdn: Shared<String>,

    sample_rate: SampleRate,
    min_frames: u32,
    max_frames: u32,
}

impl AudioGraph {
    pub fn new(
        coll_handle: basedrop::Handle,
        graph_in_channels: u16,
        graph_out_channels: u16,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        note_buffer_size: usize,
        event_buffer_size: usize,
        thread_ids: SharedThreadIDs,
        transport_declick_time: Option<Seconds>,
    ) -> (Self, SharedProcessorSchedule, TransportHandle) {
        //assert!(graph_in_channels > 0);
        assert!(graph_out_channels > 0);

        let mut graph_helper = AudioGraphHelper::new(3);

        let (transport_task, transport_handle) = TransportTask::new(
            None,
            sample_rate,
            max_frames as usize,
            transport_declick_time,
            coll_handle.clone(),
        );

        let (shared_pools, shared_schedule) = GraphSharedPools::new(
            thread_ids,
            max_frames as usize,
            note_buffer_size,
            event_buffer_size,
            transport_task,
            coll_handle.clone(),
        );

        let graph_in_rdn = Shared::new(&coll_handle, String::from("app.meadowlark.graph_in_node"));
        let graph_out_rdn =
            Shared::new(&coll_handle, String::from("app.meadowlark.graph_out_node"));
        let temp_rdn =
            Shared::new(&coll_handle, String::from("app.meadowlark.temporary_plugin_rdn"));

        let graph_in_node_id = graph_helper.add_node(0.0);
        let graph_out_node_id = graph_helper.add_node(0.0);

        let graph_in_id = PluginInstanceID::_new(
            graph_in_node_id.into(),
            0,
            PluginInstanceType::GraphInput,
            graph_in_rdn,
        );
        let graph_out_id = PluginInstanceID::_new(
            graph_out_node_id.into(),
            1,
            PluginInstanceType::GraphOutput,
            graph_out_rdn,
        );

        let mut new_self = Self {
            shared_pools,
            verifier: Verifier::new(),
            graph_helper,
            coll_handle,
            graph_in_channels,
            graph_out_channels,
            graph_in_id,
            graph_out_id,
            graph_in_audio_out_port_refs: Vec::new(),
            graph_out_audio_in_port_refs: Vec::new(),
            temp_rdn,
            sample_rate,
            min_frames,
            max_frames,
        };

        new_self.reset();

        (new_self, shared_schedule, transport_handle)
    }

    pub fn add_new_plugin_instance(
        &mut self,
        save_state: DSPluginSaveState,
        plugin_scanner: &mut PluginScanner,
        fallback_to_other_formats: bool,
    ) -> NewPluginRes {
        let do_activate_plugin = save_state.is_active;

        let node_id = self.graph_helper.add_node(0.0);
        let res = plugin_scanner.create_plugin(save_state, node_id, fallback_to_other_formats);
        let plugin_id = res.plugin_host.id().clone();

        match res.status {
            Ok(()) => {
                log::debug!("Loaded plugin {:?} successfully", &res.plugin_host.id());
            }
            Err(e) => {
                log::error!(
                    "Failed to load plugin {:?} from save state: {}",
                    &res.plugin_host.id(),
                    e
                );
            }
        }

        if self.shared_pools.plugin_hosts.pool.insert(plugin_id.clone(), res.plugin_host).is_some()
        {
            panic!("Something went wrong when allocating a new slot for a plugin");
        }

        let activation_status = if do_activate_plugin {
            self.activate_plugin_instance(&plugin_id).unwrap()
        } else {
            PluginStatus::Inactive
        };

        NewPluginRes { plugin_id, status: activation_status }
    }

    pub fn activate_plugin_instance(&mut self, id: &PluginInstanceID) -> Result<PluginStatus, ()> {
        let plugin_host = if let Some(plugin_host) = self.shared_pools.plugin_hosts.pool.get_mut(id)
        {
            plugin_host
        } else {
            return Err(());
        };

        if let Err(e) = plugin_host.can_activate() {
            return Ok(PluginStatus::ActivationError(e));
        }

        let activation_status = match plugin_host.activate(
            self.sample_rate,
            self.min_frames as u32,
            self.max_frames as u32,
            &self.coll_handle,
        ) {
            Ok(res) => PluginStatus::Activated(res),
            Err(e) => PluginStatus::ActivationError(e),
        };

        if let PluginStatus::Activated { .. } = &activation_status {
            plugin_host.sync_ports_in_graph(&mut self.graph_helper);
        }

        Ok(activation_status)
    }

    /// Remove the given plugins from the graph.
    ///
    /// This will also automatically disconnect all edges that were connected to these
    /// plugins.
    ///
    /// Requests to remove the "graph input/output" nodes with the IDs `AudioGraph::graph_in_node_id()`
    /// and `AudioGraph::graph_out_node_id()` will be ignored.
    pub fn remove_plugin_instances(
        &mut self,
        plugin_ids: &[PluginInstanceID],
        affected_plugins: &mut FnvHashSet<PluginInstanceID>,
    ) -> FnvHashSet<PluginInstanceID> {
        let mut removed_plugins: FnvHashSet<PluginInstanceID> = FnvHashSet::default();

        for id in plugin_ids.iter() {
            if id == &self.graph_in_id || id == &self.graph_out_id {
                log::warn!("Ignored request to remove graph in/out node");
                continue;
            }

            if removed_plugins.insert(id.clone()) {
                if let Ok(edges) = self.get_plugin_edges(&id) {
                    for e in edges.incoming {
                        let _ = affected_plugins.insert(e.src_plugin_id.clone());
                    }
                    for e in edges.outgoing {
                        let _ = affected_plugins.insert(e.dst_plugin_id.clone());
                    }

                    if let Some(plugin_host) = self.shared_pools.plugin_hosts.pool.get_mut(&id) {
                        plugin_host.schedule_remove();
                    }

                    if let Err(e) = self.graph_helper.delete_node(NodeRef::new(id._node_id())) {
                        log::error!("Abstract node failed to delete node: {}", e);
                    }
                } else {
                    let _ = removed_plugins.remove(&id);
                    log::warn!("Ignored request to remove plugin instance {:?}: Plugin is already removed.", id);
                }
            }
        }

        removed_plugins
    }

    pub fn connect_edge(
        &mut self,
        edge: &EdgeReq,
        src_plugin_id: &PluginInstanceID,
        dst_plugin_id: &PluginInstanceID,
    ) -> Result<(), ConnectEdgeError> {
        let src_port_ref = if src_plugin_id == &self.graph_in_id {
            match &edge.src_port_id {
                EdgeReqPortID::Main => match edge.edge_type {
                    PortType::Audio => {
                        if let Some(port_ref) =
                            self.graph_in_audio_out_port_refs.get(usize::from(edge.src_channel))
                        {
                            *port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::ParamAutomation => {
                        return Err(ConnectEdgeError {
                            error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                            edge: edge.clone(),
                        });
                    }
                    PortType::Note => {
                        // TODO: Note in/out ports on graph in/out nodes.
                        return Err(ConnectEdgeError {
                            error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                            edge: edge.clone(),
                        });
                    }
                },
                EdgeReqPortID::StableID(_id) => {
                    // TODO: Stable IDs for ports on graph in/out nodes?
                    return Err(ConnectEdgeError {
                        error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                        edge: edge.clone(),
                    });
                }
            }
        } else if let Some(plugin_host) = self.shared_pools.plugin_hosts.pool.get(src_plugin_id) {
            match &edge.src_port_id {
                EdgeReqPortID::Main => match edge.edge_type {
                    PortType::Audio => {
                        if let Some(port_ref) = plugin_host
                            .port_refs()
                            .main_audio_out_port_refs
                            .get(usize::from(edge.src_channel))
                        {
                            *port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::ParamAutomation => {
                        if let Some(port_ref) = plugin_host.port_refs().automation_out_port_ref {
                            port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::Note => {
                        if let Some(port_ref) = plugin_host.port_refs().main_note_out_port_ref {
                            port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                },
                EdgeReqPortID::StableID(id) => {
                    let src_port_id = ChannelID {
                        port_type: edge.edge_type,
                        stable_id: *id,
                        is_input: false,
                        channel: edge.src_channel,
                    };

                    if let Some(port_ref) = plugin_host.port_refs().channels_refs.get(&src_port_id)
                    {
                        *port_ref
                    } else {
                        return Err(ConnectEdgeError {
                            error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                            edge: edge.clone(),
                        });
                    }
                }
            }
        } else {
            return Err(ConnectEdgeError {
                error_type: ConnectEdgeErrorType::SrcPluginDoesNotExist,
                edge: edge.clone(),
            });
        };

        let dst_port_ref = if dst_plugin_id == &self.graph_out_id {
            match &edge.dst_port_id {
                EdgeReqPortID::Main => match edge.edge_type {
                    PortType::Audio => {
                        if let Some(port_ref) =
                            self.graph_out_audio_in_port_refs.get(usize::from(edge.dst_channel))
                        {
                            *port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::ParamAutomation => {
                        return Err(ConnectEdgeError {
                            error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                            edge: edge.clone(),
                        });
                    }
                    PortType::Note => {
                        // TODO: Note in/out ports on graph in/out nodes.
                        return Err(ConnectEdgeError {
                            error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                            edge: edge.clone(),
                        });
                    }
                },
                EdgeReqPortID::StableID(_id) => {
                    // TODO: Stable IDs for ports on graph in/out nodes?
                    return Err(ConnectEdgeError {
                        error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                        edge: edge.clone(),
                    });
                }
            }
        } else if let Some(plugin_host) = self.shared_pools.plugin_hosts.pool.get(dst_plugin_id) {
            match &edge.dst_port_id {
                EdgeReqPortID::Main => match edge.edge_type {
                    PortType::Audio => {
                        if let Some(port_ref) = plugin_host
                            .port_refs()
                            .main_audio_in_port_refs
                            .get(usize::from(edge.dst_channel))
                        {
                            *port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::ParamAutomation => {
                        if let Some(port_ref) = plugin_host.port_refs().automation_in_port_ref {
                            port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::Note => {
                        if let Some(port_ref) = plugin_host.port_refs().main_note_in_port_ref {
                            port_ref
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                },
                EdgeReqPortID::StableID(id) => {
                    let dst_port_id = ChannelID {
                        port_type: edge.edge_type,
                        stable_id: *id,
                        is_input: true,
                        channel: edge.dst_channel,
                    };

                    if let Some(port_ref) = plugin_host.port_refs().channels_refs.get(&dst_port_id)
                    {
                        *port_ref
                    } else {
                        return Err(ConnectEdgeError {
                            error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                            edge: edge.clone(),
                        });
                    }
                }
            }
        } else {
            return Err(ConnectEdgeError {
                error_type: ConnectEdgeErrorType::DstPluginDoesNotExist,
                edge: edge.clone(),
            });
        };

        if src_plugin_id == dst_plugin_id || src_port_ref == dst_port_ref {
            return Err(ConnectEdgeError {
                error_type: ConnectEdgeErrorType::Cycle,
                edge: edge.clone(),
            });
        }

        match self.graph_helper.connect(src_port_ref, dst_port_ref) {
            Ok(()) => {
                log::trace!("Successfully connected edge: {:?}", &edge);

                Ok(())
            }
            Err(e) => {
                if let audio_graph::Error::Cycle = e {
                    Err(ConnectEdgeError {
                        error_type: ConnectEdgeErrorType::Cycle,
                        edge: edge.clone(),
                    })
                } else {
                    log::error!("Unexpected edge connect error: {}", e);
                    Err(ConnectEdgeError {
                        error_type: ConnectEdgeErrorType::Unkown,
                        edge: edge.clone(),
                    })
                }
            }
        }
    }

    pub fn disconnect_edge(&mut self, edge: &Edge) -> bool {
        let mut found_ports = None;
        if let Ok(edges) = self.graph_helper.node_edges(NodeRef::new(edge.src_plugin_id._node_id()))
        {
            // Find the corresponding edge.
            for e in edges.iter() {
                if e.dst_node.as_usize() != edge.dst_plugin_id._node_id() {
                    continue;
                }
                if e.src_node.as_usize() != edge.src_plugin_id._node_id() {
                    continue;
                }
                if e.port_type != edge.edge_type {
                    continue;
                }

                let src_port = self.graph_helper.port_ident(e.src_port).unwrap();
                let dst_port = self.graph_helper.port_ident(e.dst_port).unwrap();

                if src_port.stable_id != edge.src_stable_id {
                    continue;
                }
                if dst_port.stable_id != edge.dst_stable_id {
                    continue;
                }

                if src_port.is_input {
                    continue;
                }
                if !dst_port.is_input {
                    continue;
                }

                if src_port.channel != edge.src_channel {
                    continue;
                }
                if dst_port.channel != edge.dst_channel {
                    continue;
                }

                found_ports = Some((e.src_port, e.dst_port));
                break;
            }
        }

        if let Some((src_port, dst_port)) = found_ports {
            if let Err(e) = self.graph_helper.disconnect(src_port, dst_port) {
                log::error!("Unexpected error while disconnecting edge {:?}: {}", edge, e);
            } else {
                log::trace!("Successfully disconnected edge: {:?}", edge);
            }
            true
        } else {
            log::warn!("Could not disconnect edge: {:?}: Edge was not found in the graph", edge);
            false
        }
    }

    pub fn get_plugin_edges(&self, id: &PluginInstanceID) -> Result<PluginEdges, ()> {
        if let Ok(edges) = self.graph_helper.node_edges(NodeRef::new(id._node_id())) {
            let mut incoming: SmallVec<[Edge; 8]> = SmallVec::new();
            let mut outgoing: SmallVec<[Edge; 8]> = SmallVec::new();

            for edge in edges.iter() {
                let src_port = self.graph_helper.port_ident(edge.src_port).unwrap();
                let dst_port = self.graph_helper.port_ident(edge.dst_port).unwrap();

                if edge.src_node.as_usize() == id._node_id() {
                    outgoing.push(Edge {
                        edge_type: edge.port_type,
                        src_plugin_id: id.clone(),
                        dst_plugin_id: self.graph_helper.node_ident(edge.dst_node).unwrap().clone(),
                        src_stable_id: src_port.stable_id,
                        src_channel: src_port.channel,
                        dst_stable_id: dst_port.stable_id,
                        dst_channel: dst_port.channel,
                    });
                } else {
                    incoming.push(Edge {
                        edge_type: edge.port_type,
                        src_plugin_id: self.graph_helper.node_ident(edge.src_node).unwrap().clone(),
                        dst_plugin_id: id.clone(),
                        src_stable_id: src_port.stable_id,
                        src_channel: src_port.channel,
                        dst_stable_id: dst_port.stable_id,
                        dst_channel: dst_port.channel,
                    });
                }
            }

            Ok(PluginEdges { incoming, outgoing })
        } else {
            Err(())
        }
    }

    pub fn reset(&mut self) {
        // Try to gracefully remove all existing plugins.
        for plugin_host in self.shared_pools.plugin_hosts.pool.values_mut() {
            plugin_host.schedule_remove();
        }

        // TODO: Check that the audio thread is still alive.
        let audio_thread_is_alive = true;

        if audio_thread_is_alive {
            let start_time = std::time::Instant::now();

            const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

            // Wait for all plugins to be removed.
            while !self.shared_pools.plugin_hosts.pool.is_empty() && start_time.elapsed() < TIMEOUT
            {
                std::thread::sleep(std::time::Duration::from_millis(10));

                let mut _events_out: SmallVec<[OnIdleEvent; 32]> = SmallVec::new();

                let _ = self.on_idle(&mut _events_out);
            }

            if self.shared_pools.plugin_hosts.pool.is_empty() {
                log::error!("Timed out while removing all plugins");
            }
        }

        self.shared_pools.shared_schedule.set_new_schedule(
            ProcessorSchedule::new_empty(
                self.max_frames as usize,
                self.shared_pools.transports.transport.clone(),
            ),
            &self.coll_handle,
        );

        self.shared_pools.plugin_hosts.pool.clear();
        self.shared_pools.buffers.remove_excess_buffers(0, 0, 0, 0);

        self.graph_helper = Graph::default();

        // ---  Add the graph input and graph output nodes to the graph  --------------------------

        let graph_in_node_id = self.graph_helper.node(self.graph_in_id.clone());
        let graph_out_node_id = self.graph_helper.node(self.graph_out_id.clone());

        let graph_in_node_id = PluginInstanceID::_new(
            graph_in_node_id.as_usize(),
            0,
            PluginInstanceType::GraphInput,
            Shared::clone(self.graph_in_id.rdn()),
        );
        let graph_out_node_id = PluginInstanceID::_new(
            graph_out_node_id.as_usize(),
            1,
            PluginInstanceType::GraphOutput,
            Shared::clone(self.graph_out_id.rdn()),
        );

        self.graph_in_id = graph_in_node_id.clone();
        self.graph_out_id = graph_out_node_id.clone();

        self.graph_helper.set_node_ident(graph_in_node_id, graph_in_node_id).unwrap();
        self.graph_helper.set_node_ident(graph_out_node_id, graph_out_node_id).unwrap();

        let mut graph_in_audio_out_port_refs: Vec<audio_graph::PortRef> = Vec::new();
        let mut graph_out_audio_in_port_refs: Vec<audio_graph::PortRef> = Vec::new();

        for i in 0..self.graph_in_channels {
            let port_id =
                ChannelID { port_type: PortType::Audio, stable_id: 0, is_input: false, channel: i };

            let port_ref =
                self.graph_helper.port(graph_in_node_id, PortType::Audio, port_id).unwrap();

            graph_in_audio_out_port_refs.push(port_ref);
        }
        for i in 0..self.graph_out_channels {
            let port_id =
                ChannelID { port_type: PortType::Audio, stable_id: 0, is_input: true, channel: i };

            let port_ref =
                self.graph_helper.port(graph_out_node_id, PortType::Audio, port_id).unwrap();

            graph_out_audio_in_port_refs.push(port_ref);
        }

        self.graph_in_audio_out_port_refs = graph_in_audio_out_port_refs;
        self.graph_out_audio_in_port_refs = graph_out_audio_in_port_refs;
    }

    /// Compile the audio graph into a schedule that is sent to the audio thread.
    ///
    /// If an error is returned then the graph **MUST** be restored with the previous
    /// working save state.
    pub fn compile(&mut self) -> Result<(), GraphCompilerError> {
        match compiler::compile_graph(
            &mut self.shared_pools,
            &mut self.graph_helper,
            &self.graph_in_id,
            &self.graph_out_id,
            &self.graph_in_audio_out_port_refs,
            &self.graph_out_audio_in_port_refs,
            &mut self.verifier,
            &self.coll_handle,
        ) {
            Ok(schedule) => {
                log::debug!("Successfully compiled new schedule:\n{:?}", &schedule);

                self.shared_pools.shared_schedule.set_new_schedule(schedule, &self.coll_handle);
                Ok(())
            }
            Err(e) => {
                // Replace the current schedule with an emtpy one now that the graph
                // is in an invalid state.
                self.shared_pools.shared_schedule.set_new_schedule(
                    ProcessorSchedule::new_empty(
                        self.max_frames as usize,
                        self.shared_pools.transports.transport.clone(),
                    ),
                    &self.coll_handle,
                );
                Err(e)
            }
        }
    }

    pub fn collect_save_states(&mut self) -> Vec<(PluginInstanceID, DSPluginSaveState)> {
        let mut res: Vec<(PluginInstanceID, DSPluginSaveState)> = Vec::new();

        for plugin_host in self.shared_pools.plugin_hosts.pool.values_mut() {
            if plugin_host.is_save_state_dirty() {
                res.push((plugin_host.id().clone(), plugin_host.collect_save_state()));
            }
        }

        res
    }

    pub fn on_idle(&mut self, mut events_out: &mut SmallVec<[OnIdleEvent; 32]>) -> bool {
        let mut plugins_to_remove: SmallVec<[PluginInstanceID; 4]> = SmallVec::new();

        let mut recompile_graph = false;

        // TODO: Optimize by using some kind of hashmap queue that only iterates over the
        // plugins that have non-zero host request flags, instead of iterating over every
        // plugin every time?
        for plugin_host in self.shared_pools.plugin_hosts.pool.values_mut() {
            let (res, modified_params) = plugin_host.on_idle(
                self.sample_rate,
                self.min_frames,
                self.max_frames,
                &self.coll_handle,
                &mut events_out,
            );

            match res {
                OnIdleResult::Ok => {}
                OnIdleResult::PluginDeactivated => {
                    recompile_graph = true;

                    println!("plugin deactivated");

                    events_out.push(OnIdleEvent::PluginDeactivated {
                        plugin_id: plugin_host.id().clone(),
                        status: Ok(()),
                    });
                }
                OnIdleResult::PluginActivated(status) => {
                    plugin_host.sync_ports_in_graph(&mut self.graph_helper);

                    recompile_graph = true;

                    events_out.push(OnIdleEvent::PluginActivated {
                        plugin_id: plugin_host.id().clone(),
                        status,
                    });
                }
                OnIdleResult::PluginReadyToRemove => {
                    plugins_to_remove.push(plugin_host.id().clone());

                    // The user should have already been alerted of the plugin being removed
                    // in a previous `OnIdleEvent::AudioGraphModified` event.
                }
                OnIdleResult::PluginFailedToActivate(e) => {
                    recompile_graph = true;

                    events_out.push(OnIdleEvent::PluginDeactivated {
                        plugin_id: plugin_host.id().clone(),
                        status: Err(e),
                    });
                }
            }

            if !modified_params.is_empty() {
                events_out.push(OnIdleEvent::PluginParamsModified {
                    plugin_id: plugin_host.id().clone(),
                    modified_params: modified_params.to_owned(),
                });
            }
        }

        for plugin in plugins_to_remove.iter() {
            let _ = self.shared_pools.plugin_hosts.pool.remove(plugin);
        }

        recompile_graph
    }

    pub fn update_tempo_map(&mut self, new_tempo_map: Shared<TempoMap>) {
        for plugin_host in self.shared_pools.plugin_hosts.pool.values_mut() {
            plugin_host.update_tempo_map(&new_tempo_map);
        }
    }

    pub fn get_plugin_host(&self, id: &PluginInstanceID) -> Option<&PluginHostMainThread> {
        self.shared_pools.plugin_hosts.pool.get(id)
    }

    pub fn get_plugin_host_mut(
        &mut self,
        id: &PluginInstanceID,
    ) -> Option<&mut PluginHostMainThread> {
        self.shared_pools.plugin_hosts.pool.get_mut(id)
    }

    pub fn graph_in_id(&self) -> &PluginInstanceID {
        &self.graph_in_id
    }

    pub fn graph_out_id(&self) -> &PluginInstanceID {
        &self.graph_out_id
    }
}

impl Drop for AudioGraph {
    fn drop(&mut self) {
        self.shared_pools.shared_schedule.set_new_schedule(
            ProcessorSchedule::new_empty(
                self.max_frames as usize,
                self.shared_pools.transports.transport.clone(),
            ),
            &self.coll_handle,
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginEdges {
    pub incoming: SmallVec<[Edge; 8]>,
    pub outgoing: SmallVec<[Edge; 8]>,
}

impl PluginEdges {
    pub fn emtpy() -> Self {
        Self { incoming: SmallVec::new(), outgoing: SmallVec::new() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub edge_type: PortType,

    pub src_plugin_id: PluginInstanceID,
    pub dst_plugin_id: PluginInstanceID,

    pub src_stable_id: u32,
    pub src_channel: u16,

    pub dst_stable_id: u32,
    pub dst_channel: u16,
}
