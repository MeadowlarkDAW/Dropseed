use audio_graph::{error::AddEdgeError, AudioGraphHelper, EdgeID, NodeID, PortID, TypeIdx};
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
    pub const NUM_TYPES: usize = 3;

    pub const AUDIO_TYPE_IDX: TypeIdx = TypeIdx(PortType::Audio as u32 as usize);
    pub const NOTE_TYPE_IDX: TypeIdx = TypeIdx(PortType::Note as u32 as usize);
    pub const PARAM_AUTOMATION_TYPE_IDX: TypeIdx =
        TypeIdx(PortType::ParamAutomation as u32 as usize);

    pub const AUDIO_IDX: usize = 0;
    pub const NOTE_IDX: usize = 1;
    pub const PARAM_AUTOMATION_IDX: usize = 2;

    pub fn from_type_idx(p: TypeIdx) -> Option<Self> {
        match p.0 {
            0 => Some(PortType::Audio),
            1 => Some(PortType::Note),
            2 => Some(PortType::ParamAutomation),
            _ => None,
        }
    }

    pub fn as_type_idx(&self) -> TypeIdx {
        TypeIdx(*self as u32 as usize)
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
    graph_in_audio_out_port_ids: Vec<PortID>,
    graph_out_audio_in_port_ids: Vec<PortID>,

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

        let mut graph_helper = AudioGraphHelper::new(PortType::NUM_TYPES);

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
            graph_in_audio_out_port_ids: Vec::new(),
            graph_out_audio_in_port_ids: Vec::new(),
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
            &mut self.graph_helper,
            &self.coll_handle,
        ) {
            Ok(res) => PluginStatus::Activated(res),
            Err(e) => PluginStatus::ActivationError(e),
        };

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
    ) -> (FnvHashSet<PluginInstanceID>, Vec<EdgeID>) {
        let mut removed_plugins: FnvHashSet<PluginInstanceID> = FnvHashSet::default();

        let mut removed_edges: Vec<EdgeID> = Vec::new();

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

                    removed_edges
                        .append(&mut self.graph_helper.remove_node(id._node_id().into()).unwrap());
                } else {
                    let _ = removed_plugins.remove(&id);
                    log::warn!("Ignored request to remove plugin instance {:?}: Plugin is already removed.", id);
                }
            }
        }

        (removed_plugins, removed_edges)
    }

    pub fn connect_edge(
        &mut self,
        edge: &EdgeReq,
        src_plugin_id: &PluginInstanceID,
        dst_plugin_id: &PluginInstanceID,
        check_for_cycles: bool,
    ) -> Result<EdgeID, ConnectEdgeError> {
        let src_port_id = if src_plugin_id == &self.graph_in_id {
            match &edge.src_port_id {
                EdgeReqPortID::Main => match edge.edge_type {
                    PortType::Audio => {
                        if let Some(port_id) =
                            self.graph_in_audio_out_port_ids.get(usize::from(edge.src_channel))
                        {
                            *port_id
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
                        if let Some(port_id) = plugin_host
                            .port_ids()
                            .main_audio_out_port_ids
                            .get(usize::from(edge.src_channel))
                        {
                            *port_id
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::ParamAutomation => {
                        if let Some(port_id) = plugin_host.port_ids().automation_out_port_id {
                            port_id
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::Note => {
                        if let Some(port_id) = plugin_host.port_ids().main_note_out_port_id {
                            port_id
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::SrcPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                },
                EdgeReqPortID::StableID(id) => {
                    let src_channel_id = ChannelID {
                        port_type: edge.edge_type,
                        stable_id: *id,
                        is_input: false,
                        channel: edge.src_channel,
                    };

                    if let Some(port_id) =
                        plugin_host.port_ids().channel_id_to_port_id.get(&src_channel_id)
                    {
                        *port_id
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

        let dst_port_id = if dst_plugin_id == &self.graph_out_id {
            match &edge.dst_port_id {
                EdgeReqPortID::Main => match edge.edge_type {
                    PortType::Audio => {
                        if let Some(port_id) =
                            self.graph_out_audio_in_port_ids.get(usize::from(edge.dst_channel))
                        {
                            *port_id
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
                        if let Some(port_id) = plugin_host
                            .port_ids()
                            .main_audio_in_port_ids
                            .get(usize::from(edge.dst_channel))
                        {
                            *port_id
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::ParamAutomation => {
                        if let Some(port_id) = plugin_host.port_ids().automation_in_port_id {
                            port_id
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                    PortType::Note => {
                        if let Some(port_id) = plugin_host.port_ids().main_note_in_port_id {
                            port_id
                        } else {
                            return Err(ConnectEdgeError {
                                error_type: ConnectEdgeErrorType::DstPortDoesNotExist,
                                edge: edge.clone(),
                            });
                        }
                    }
                },
                EdgeReqPortID::StableID(id) => {
                    let dst_channel_id = ChannelID {
                        port_type: edge.edge_type,
                        stable_id: *id,
                        is_input: true,
                        channel: edge.dst_channel,
                    };

                    if let Some(port_id) =
                        plugin_host.port_ids().channel_id_to_port_id.get(&dst_channel_id)
                    {
                        *port_id
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

        match self.graph_helper.add_edge(
            src_plugin_id._node_id().into(),
            src_port_id,
            dst_plugin_id._node_id().into(),
            dst_port_id,
            check_for_cycles,
        ) {
            Ok(edge_id) => Ok(edge_id),
            Err(AddEdgeError::CycleDetected) => Err(ConnectEdgeError {
                error_type: ConnectEdgeErrorType::Cycle,
                edge: edge.clone(),
            }),
            Err(AddEdgeError::EdgeAlreadyExists(_)) => Err(ConnectEdgeError {
                error_type: ConnectEdgeErrorType::EdgeAlreadyExists,
                edge: edge.clone(),
            }),
            Err(e) => {
                log::error!("Unexpected error while connecting edge: {}", e);

                Err(ConnectEdgeError {
                    error_type: ConnectEdgeErrorType::Unkown,
                    edge: edge.clone(),
                })
            }
        }
    }

    pub fn disconnect_edge(&mut self, edge_id: EdgeID) -> bool {
        if self.graph_helper.remove_edge(edge_id).is_ok() {
            log::trace!("Successfully disconnected edge: {:?}", edge_id);
            true
        } else {
            log::warn!("Could not disconnect edge: {:?}: Edge was not found in the graph", edge_id);
            false
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

        self.graph_helper = AudioGraphHelper::new(PortType::NUM_TYPES);

        // ---  Add the graph input and graph output nodes to the graph  --------------------------

        let graph_in_node_id = self.graph_helper.add_node(0.0);
        let graph_out_node_id = self.graph_helper.add_node(0.0);

        self.graph_in_id = PluginInstanceID::_new(
            graph_in_node_id.into(),
            0,
            PluginInstanceType::GraphInput,
            Shared::clone(self.graph_in_id.rdn()),
        );
        self.graph_out_id = PluginInstanceID::_new(
            graph_out_node_id.into(),
            1,
            PluginInstanceType::GraphOutput,
            Shared::clone(self.graph_out_id.rdn()),
        );

        self.graph_in_audio_out_port_ids.clear();
        self.graph_out_audio_in_port_ids.clear();

        for i in 0..self.graph_in_channels {
            let channel_id =
                ChannelID { port_type: PortType::Audio, stable_id: 0, is_input: false, channel: i };

            self.graph_helper
                .add_port(graph_in_node_id, PortID(i as u32), PortType::Audio.as_type_idx(), false)
                .unwrap();

            self.graph_in_audio_out_port_ids.push(PortID(i as u32));
        }
        for i in 0..self.graph_out_channels {
            let channel_id =
                ChannelID { port_type: PortType::Audio, stable_id: 0, is_input: true, channel: i };

            self.graph_helper
                .add_port(graph_out_node_id, PortID(i as u32), PortType::Audio.as_type_idx(), true)
                .unwrap();

            self.graph_out_audio_in_port_ids.push(PortID(i as u32));
        }
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
            &self.graph_in_audio_out_port_ids,
            &self.graph_out_audio_in_port_ids,
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
