use basedrop::{Collector, Shared, SharedCell};
use fnv::FnvHashSet;
use meadowlark_core_types::time::{SampleRate, Seconds};
use smallvec::SmallVec;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use thread_priority::ThreadPriority;

use dropseed_plugin_api::ext::audio_ports::PluginAudioPortsExt;
use dropseed_plugin_api::ext::note_ports::PluginNotePortsExt;
use dropseed_plugin_api::ext::params::ParamInfo;
use dropseed_plugin_api::plugin_scanner::ScannedPluginKey;
use dropseed_plugin_api::transport::TempoMap;
use dropseed_plugin_api::{DSPluginSaveState, HostInfo, PluginFactory, PluginInstanceID};

use crate::engine::audio_thread::DSEngineAudioThread;
use crate::graph::{AudioGraph, PluginEdges, PortType};
use crate::plugin_host::error::ActivatePluginError;
use crate::plugin_host::{ParamModifiedInfo, PluginHostMainThread};
use crate::plugin_scanner::{PluginScanner, RescanPluginDirectoriesRes};
use crate::processor_schedule::TransportHandle;
use crate::utils::thread_id::SharedThreadIDs;

use super::error::{EngineCrashError, NewPluginInstanceError};
use super::request::{EdgeReq, EdgeReqPortID, ModifyGraphRequest, PluginIDReq};

pub struct DSEngineMainThread<CH: FnMut(EngineCrashError)> {
    audio_graph: Option<AudioGraph>,
    host_info: Shared<HostInfo>,
    plugin_scanner: PluginScanner,
    thread_ids: SharedThreadIDs,
    collector: Collector,
    run_process_thread: Option<Arc<AtomicBool>>,
    process_thread_handle: Option<JoinHandle<()>>,
    tempo_map_shared: Option<Shared<SharedCell<(Shared<TempoMap>, u64)>>>,

    crash_handler: CH,
}

impl<CH: FnMut(EngineCrashError)> DSEngineMainThread<CH> {
    /// Construct a new Dropseed engine.
    ///
    /// * `host_info` - The information about this host.
    /// * `internal_plugins` - A list of plugin factories for internal plugins.
    /// * `crash_handler` - Called when the engine crashes. When this closure is
    /// called, the engine will deactivated and must be re-activated to keep
    /// using.
    pub fn new(
        host_info: HostInfo,
        mut internal_plugins: Vec<Box<dyn PluginFactory>>,
        crash_handler: CH,
    ) -> (Self, Vec<Result<ScannedPluginKey, String>>) {
        // Set up and run garbage collector wich collects and safely drops garbage from
        // the audio thread.
        let collector = Collector::new();

        let host_info = Shared::new(&collector.handle(), host_info);

        let thread_ids =
            SharedThreadIDs::new(Some(thread::current().id()), None, &collector.handle());

        let mut plugin_scanner =
            PluginScanner::new(collector.handle(), Shared::clone(&host_info), thread_ids.clone());

        // Scan the user's internal plugins.
        let internal_plugins_res: Vec<Result<ScannedPluginKey, String>> =
            internal_plugins.drain(..).map(|p| plugin_scanner.scan_internal_plugin(p)).collect();

        (
            Self {
                audio_graph: None,
                host_info,
                plugin_scanner,
                thread_ids,
                collector,
                run_process_thread: None,
                process_thread_handle: None,
                tempo_map_shared: None,
                crash_handler,
            },
            internal_plugins_res,
        )
    }

    /// Retrieve the info about this host
    pub fn host_info(&self) -> &HostInfo {
        &*self.host_info
    }

    // TODO: multiple transports
    /// Replace the old tempo map with this new one
    pub fn update_tempo_map(&mut self, new_tempo_map: TempoMap) {
        if let Some(tempo_map_shared) = &self.tempo_map_shared {
            let tempo_map_version = tempo_map_shared.get().1;

            let new_tempo_map_shared = Shared::new(&self.collector.handle(), new_tempo_map);

            tempo_map_shared.set(Shared::new(
                &self.collector.handle(),
                (Shared::clone(&new_tempo_map_shared), tempo_map_version + 1),
            ));

            if let Some(audio_graph) = &mut self.audio_graph {
                audio_graph.update_tempo_map(new_tempo_map_shared);
            }
        }
    }

    /// Get an immutable reference to the host for a particular plugin.
    ///
    /// This will return `None` if a plugin with the given ID does not exist/
    /// has been removed.
    pub fn get_plugin_host(&self, id: &PluginInstanceID) -> Option<&PluginHostMainThread> {
        self.audio_graph.as_ref().and_then(|a| a.get_plugin_host(&id))
    }

    /// Get a mutable reference to the host for a particular plugin.
    ///
    /// This will return `None` if a plugin with the given ID does not exist/
    /// has been removed.
    pub fn get_plugin_host_mut(
        &mut self,
        id: &PluginInstanceID,
    ) -> Option<&mut PluginHostMainThread> {
        self.audio_graph.as_mut().and_then(|a| a.get_plugin_host_mut(&id))
    }

    /// This must be called periodically (i.e. once every frame).
    ///
    /// This will return a list of events that have occured as a result of
    pub fn on_idle(&mut self) -> SmallVec<[OnIdleEvent; 32]> {
        let mut events_out: SmallVec<[OnIdleEvent; 32]> = SmallVec::new();

        if let Some(audio_graph) = &mut self.audio_graph {
            let recompile = audio_graph.on_idle(&mut events_out);

            if recompile {
                self.compile_audio_graph();
            }
        }

        self.collector.collect();

        events_out
    }

    #[cfg(feature = "clap-host")]
    pub fn add_clap_scan_directory<P: Into<PathBuf>>(&mut self, path: P) -> bool {
        self.plugin_scanner.add_clap_scan_directory(path.into())
    }

    #[cfg(feature = "clap-host")]
    pub fn remove_clap_scan_directory<P: Into<PathBuf>>(&mut self, path: P) -> bool {
        self.plugin_scanner.remove_clap_scan_directory(path.into())
    }

    pub fn rescan_plugin_directories(&mut self) -> RescanPluginDirectoriesRes {
        self.plugin_scanner.rescan_plugin_directories()
    }

    pub fn activate_engine(
        &mut self,
        settings: &ActivateEngineSettings,
    ) -> Option<EngineActivatedInfo> {
        if self.audio_graph.is_some() {
            log::warn!("Ignored request to activate RustyDAW engine: Engine is already activated");
            return None;
        }

        log::info!("Activating RustyDAW engine...");

        let num_audio_in_channels = settings.num_audio_in_channels;
        let num_audio_out_channels = settings.num_audio_out_channels;
        let min_frames = settings.min_frames;
        let max_frames = settings.max_frames;
        let sample_rate = settings.sample_rate;
        let note_buffer_size = settings.note_buffer_size;
        let event_buffer_size = settings.event_buffer_size;
        let transport_declick_time = settings.transport_declick_time;

        let (mut audio_graph, shared_schedule, transport_handle) = AudioGraph::new(
            self.collector.handle(),
            num_audio_in_channels,
            num_audio_out_channels,
            sample_rate,
            min_frames,
            max_frames,
            note_buffer_size,
            event_buffer_size,
            self.thread_ids.clone(),
            transport_declick_time,
        );

        let graph_in_node_id = audio_graph.graph_in_id().clone();
        let graph_out_node_id = audio_graph.graph_out_id().clone();

        // TODO: Remove this once compiler is fixed.
        audio_graph
            .connect_edge(
                &EdgeReq {
                    edge_type: PortType::Audio,
                    src_plugin_id: PluginIDReq::Added(0),
                    dst_plugin_id: PluginIDReq::Added(0),
                    src_port_id: EdgeReqPortID::Main,
                    src_port_channel: 0,
                    dst_port_id: EdgeReqPortID::Main,
                    dst_port_channel: 0,
                    log_error_on_fail: true,
                },
                &graph_in_node_id,
                &graph_out_node_id,
            )
            .unwrap();
        audio_graph
            .connect_edge(
                &EdgeReq {
                    edge_type: PortType::Audio,
                    src_plugin_id: PluginIDReq::Added(0),
                    dst_plugin_id: PluginIDReq::Added(0),
                    src_port_id: EdgeReqPortID::Main,
                    src_port_channel: 1,
                    dst_port_id: EdgeReqPortID::Main,
                    dst_port_channel: 1,
                    log_error_on_fail: true,
                },
                &graph_in_node_id,
                &graph_out_node_id,
            )
            .unwrap();

        self.audio_graph = Some(audio_graph);

        self.compile_audio_graph();

        if let Some(audio_graph) = &self.audio_graph {
            log::info!("Successfully activated RustyDAW engine");

            let (audio_thread, mut process_thread) = DSEngineAudioThread::new(
                num_audio_in_channels as usize,
                num_audio_out_channels as usize,
                &self.collector.handle(),
                shared_schedule,
                sample_rate,
            );

            let run_process_thread = Arc::new(AtomicBool::new(true));

            let run_process_thread_clone = Arc::clone(&run_process_thread);

            if let Some(old_run_process_thread) = self.run_process_thread.take() {
                // Just to be sure.
                old_run_process_thread.store(false, Ordering::Relaxed);
            }
            self.run_process_thread = Some(run_process_thread);

            let process_thread_handle =
                thread_priority::spawn(ThreadPriority::Max, move |priority_res| {
                    if let Err(e) = priority_res {
                        log::error!("Failed to set process thread priority to max: {:?}", e);
                    } else {
                        log::info!("Successfully set process thread priority to max");
                    }

                    process_thread.run(run_process_thread_clone);
                });

            self.process_thread_handle = Some(process_thread_handle);

            self.tempo_map_shared = Some(transport_handle.tempo_map_shared());
            let tempo_map = (*self.tempo_map_shared.as_ref().unwrap().get().0).clone();

            let info = EngineActivatedInfo {
                audio_thread,
                graph_in_node_id: audio_graph.graph_in_id().clone(),
                graph_out_node_id: audio_graph.graph_out_id().clone(),
                sample_rate,
                min_frames,
                max_frames,
                transport_handle,
                num_audio_in_channels,
                num_audio_out_channels,
                tempo_map,
            };

            Some(info)
        } else {
            // If this happens then we did something very wrong.
            panic!("Unexpected error: Empty audio graph failed to compile a schedule.");
        }
    }

    pub fn modify_graph(&mut self, mut req: ModifyGraphRequest) -> Option<ModifyGraphRes> {
        if let Some(audio_graph) = &mut self.audio_graph {
            let mut affected_plugins: FnvHashSet<PluginInstanceID> = FnvHashSet::default();

            for edge in req.disconnect_edges.iter() {
                if audio_graph.disconnect_edge(edge) {
                    let _ = affected_plugins.insert(edge.src_plugin_id.clone());
                    let _ = affected_plugins.insert(edge.dst_plugin_id.clone());
                }
            }

            let mut removed_plugins = audio_graph
                .remove_plugin_instances(&req.remove_plugin_instances, &mut affected_plugins);

            let new_plugins_res: Vec<NewPluginRes> = req
                .add_plugin_instances
                .drain(..)
                .map(|save_state| {
                    audio_graph.add_new_plugin_instance(save_state, &mut self.plugin_scanner, true)
                })
                .collect();

            let new_plugin_ids: Vec<PluginInstanceID> = new_plugins_res
                .iter()
                .map(|res| {
                    let _ = affected_plugins.insert(res.plugin_id.clone());
                    res.plugin_id.clone()
                })
                .collect();

            for edge in req.connect_new_edges.iter() {
                let src_plugin_id = match &edge.src_plugin_id {
                    PluginIDReq::Added(index) => {
                        if let Some(new_plugin_id) = new_plugin_ids.get(*index) {
                            new_plugin_id
                        } else {
                            log::error!(
                                "Could not connect edge {:?}: Source plugin index out of bounds",
                                edge
                            );
                            continue;
                        }
                    }
                    PluginIDReq::Existing(id) => id,
                };

                let dst_plugin_id = match &edge.dst_plugin_id {
                    PluginIDReq::Added(index) => {
                        if let Some(new_plugin_id) = new_plugin_ids.get(*index) {
                            new_plugin_id
                        } else {
                            log::error!(
                                "Could not connect edge {:?}: Destination plugin index out of bounds",
                                edge
                            );
                            continue;
                        }
                    }
                    PluginIDReq::Existing(id) => id,
                };

                if let Err(e) = audio_graph.connect_edge(edge, src_plugin_id, dst_plugin_id) {
                    if edge.log_error_on_fail {
                        log::error!("Could not connect edge: {}", e);
                    } else {
                        #[cfg(debug_assertions)]
                        log::debug!("Could not connect edge: {}", e);
                    }
                } else {
                    // These will always be true.
                    if let PluginIDReq::Existing(id) = &edge.src_plugin_id {
                        let _ = affected_plugins.insert(id.clone());
                    }
                    if let PluginIDReq::Existing(id) = &edge.dst_plugin_id {
                        let _ = affected_plugins.insert(id.clone());
                    }
                }
            }

            // Don't include the graph in/out "plugins" in the result.
            let _ = affected_plugins.remove(audio_graph.graph_in_id());
            let _ = affected_plugins.remove(audio_graph.graph_out_id());

            let updated_plugin_edges: Vec<(PluginInstanceID, PluginEdges)> = affected_plugins
                .iter()
                .filter(|plugin_id| !removed_plugins.contains(plugin_id))
                .map(|plugin_id| {
                    (plugin_id.clone(), audio_graph.get_plugin_edges(plugin_id).unwrap())
                })
                .collect();

            let removed_plugins = removed_plugins.drain().collect();

            let res = ModifyGraphRes {
                new_plugins: new_plugins_res,
                removed_plugins,
                updated_plugin_edges,
            };

            // TODO: Compile audio graph in a separate thread?
            self.compile_audio_graph();

            Some(res)
        } else {
            log::warn!("Cannot modify audio graph: Engine is deactivated");
            None
        }
    }

    pub fn deactivate_engine(&mut self) -> bool {
        if self.audio_graph.is_none() {
            log::warn!("Ignored request to deactivate engine: Engine is already deactivated");
            return false;
        }

        log::info!("Deactivating RustyDAW engine");

        self.audio_graph = None;

        if let Some(run_process_thread) = self.run_process_thread.take() {
            run_process_thread.store(false, Ordering::Relaxed);
        }
        self.process_thread_handle = None;

        self.tempo_map_shared = None;

        true
    }

    pub fn collect_latest_save_states(&mut self) -> Vec<(PluginInstanceID, DSPluginSaveState)> {
        if self.audio_graph.is_none() {
            log::warn!("Ignored request for the latest save states: Engine is deactivated");
            return Vec::new();
        }

        log::trace!("Got request for latest plugin save states");

        self.audio_graph.as_mut().unwrap().collect_save_states()
    }

    fn compile_audio_graph(&mut self) {
        if let Some(mut audio_graph) = self.audio_graph.take() {
            match audio_graph.compile() {
                Ok(_) => {
                    self.audio_graph = Some(audio_graph);
                }
                Err(e) => {
                    log::error!("{}", e);

                    if let Some(run_process_thread) = self.run_process_thread.take() {
                        run_process_thread.store(false, Ordering::Relaxed);
                    }
                    self.process_thread_handle = None;

                    // Audio graph is in an invalid state. Drop it and have the user restore
                    // from the last working save state.
                    let _ = audio_graph;

                    (self.crash_handler)(EngineCrashError::CompilerError(e));
                }
            }
        }
    }
}

impl<CH: FnMut(EngineCrashError)> Drop for DSEngineMainThread<CH> {
    fn drop(&mut self) {
        if let Some(run_process_thread) = self.run_process_thread.take() {
            run_process_thread.store(false, Ordering::Relaxed);

            if let Some(process_thread_handle) = self.process_thread_handle.take() {
                if let Err(e) = process_thread_handle.join() {
                    log::error!("Failed to join process thread handle: {:?}", e);
                }
            }
        }

        // Make sure all of the stuff in the audio thread gets dropped properly.
        let _ = self.audio_graph;

        self.collector.collect();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ActivateEngineSettings {
    pub sample_rate: SampleRate,
    pub min_frames: u32,
    pub max_frames: u32,
    pub num_audio_in_channels: u16,
    pub num_audio_out_channels: u16,
    pub note_buffer_size: usize,
    pub event_buffer_size: usize,

    /// The time window for the transport's declick buffers.
    ///
    /// Set this to `None` to have no transport declicking.
    ///
    /// By default this is set to `None`.
    pub transport_declick_time: Option<Seconds>,
}

impl Default for ActivateEngineSettings {
    fn default() -> Self {
        Self {
            sample_rate: SampleRate::default(),
            min_frames: 1,
            max_frames: 512,
            num_audio_in_channels: 2,
            num_audio_out_channels: 2,
            note_buffer_size: 256,
            event_buffer_size: 256,
            transport_declick_time: None,
        }
    }
}

pub struct EngineActivatedInfo {
    /// The realtime-safe channel for the audio thread to interface with
    /// the engine.
    ///
    /// Send this to the audio thread to be run.
    ///
    /// When a `OnIdleEvent::EngineDeactivated` event is recieved, send
    /// a signal to the audio thread to drop this.
    pub audio_thread: DSEngineAudioThread,

    /// The ID for the input to the audio graph. Use this to connect any
    /// plugins to system inputs.
    pub graph_in_node_id: PluginInstanceID,

    /// The ID for the output to the audio graph. Use this to connect any
    /// plugins to system outputs.
    pub graph_out_node_id: PluginInstanceID,

    pub transport_handle: TransportHandle,
    pub tempo_map: TempoMap,

    pub sample_rate: SampleRate,
    pub min_frames: u32,
    pub max_frames: u32,
    pub num_audio_in_channels: u16,
    pub num_audio_out_channels: u16,
}

impl std::fmt::Debug for EngineActivatedInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("EngineActivatedInfo");

        f.field("graph_in_node_id", &self.graph_in_node_id);
        f.field("graph_out_node_id", &self.graph_out_node_id);
        f.field("sample_rate", &self.sample_rate);
        f.field("min_frames", &self.min_frames);
        f.field("max_frames", &self.max_frames);
        f.field("num_audio_in_channels", &self.num_audio_in_channels);
        f.field("num_audio_out_channels", &self.num_audio_out_channels);

        f.finish()
    }
}

#[derive(Debug)]
/// Sent whenever the engine is deactivated.
///
/// The DSEngineAudioThread sent in a previous EngineActivated event is now
/// invalidated. Please drop it and wait for a new EngineActivated event to
/// replace it.
///
/// To keep using the audio graph, you must reactivate the engine with
/// `DSEngineRequest::ActivateEngine`, and then restore the audio graph
/// from an existing save state if you wish using
/// `DSEngineRequest::RestoreFromSaveState`.
pub enum EngineDeactivatedInfo {
    /// The engine was deactivated gracefully after recieving a
    /// `DSEngineRequest::DeactivateEngine` request.
    DeactivatedGracefully,
    /// The engine has crashed.
    EngineCrashed { error_msg: String },
}

#[derive(Debug)]
pub struct PluginActivatedRes {
    pub new_parameters: Vec<(ParamInfo, f64)>,
    pub new_audio_ports_ext: Option<PluginAudioPortsExt>,
    pub new_note_ports_ext: Option<PluginNotePortsExt>,
    /// If this is an internal plugin with a custom defined handle,
    /// then this will be the new custom handle.
    pub internal_handle: Option<Box<dyn std::any::Any + Send + 'static>>,
}

#[derive(Debug)]
pub enum NewPluginStatus {
    /// This means the plugin successfully activated and returned
    /// its new configurations.
    Activated(PluginActivatedRes),

    /// This means that the plugin loaded but did not activate yet. This
    /// can happen when the user loads a project with a deactivated
    /// plugin.
    Inactive,

    /// There was an error loading the plugin.
    LoadError(NewPluginInstanceError),

    /// There was an error activating the plugin.
    ActivationError(ActivatePluginError),
}

#[derive(Debug)]
pub struct NewPluginRes {
    pub plugin_id: PluginInstanceID,

    pub status: NewPluginStatus,
    pub supports_gui: bool, // TODO: probably doesn't belong here
}

#[derive(Debug)]
pub struct ModifyGraphRes {
    /// Any new plugins that were added to the graph.
    pub new_plugins: Vec<NewPluginRes>,

    /// Any plugins that were removed from the graph.
    pub removed_plugins: Vec<PluginInstanceID>,

    ///
    pub updated_plugin_edges: Vec<(PluginInstanceID, PluginEdges)>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum OnIdleEvent {
    /// Sent whenever the engine is deactivated.
    ///
    /// The DSEngineAudioThread sent in a previous EngineActivated event is now
    /// invalidated. Please drop it and wait for a new EngineActivated event to
    /// replace it.
    ///
    /// To keep using the audio graph, you must reactivate the engine with
    /// `DSEngineRequest::ActivateEngine` and repopulate the graph.
    EngineDeactivated(EngineDeactivatedInfo),

    /// Sent whenever a plugin becomes activated after being deactivated or
    /// when the plugin restarts.
    ///
    /// Make sure your UI updates the port configuration on this plugin.
    PluginActivated { plugin_id: PluginInstanceID, result: PluginActivatedRes },

    /// Sent whenever a plugin becomes deactivated. When a plugin is deactivated
    /// you cannot access any of its methods until it is reactivated.
    PluginDeactivated {
        plugin_id: PluginInstanceID,
        /// If this is `Ok(())`, then it means the plugin was gracefully
        /// deactivated from user request.
        ///
        /// If this is `Err(e)`, then it means the plugin became deactivated
        /// because it failed to restart.
        status: Result<(), ActivatePluginError>,
    },

    PluginParamsModified {
        plugin_id: PluginInstanceID,
        modified_params: SmallVec<[ParamModifiedInfo; 4]>,
    },

    /// Sent when the plugin closed its own GUI by its own means. UI should be updated accordingly
    /// so that the user could open the UI again.
    PluginGuiClosed { plugin_id: PluginInstanceID },
}
