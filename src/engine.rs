use audio_graph::DefaultPortType;
use basedrop::{Collector, Shared};
use crossbeam::channel::{self, Receiver, Sender};
use fnv::FnvHashSet;
use rusty_daw_core::SampleRate;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use crate::event::{DAWEngineEvent, PluginScannerEvent};
use crate::graph::plugin_host::OnIdleResult;
use crate::graph::{
    AudioGraph, AudioGraphSaveState, Edge, NewPluginRes, PluginActivationStatus, PluginEdges,
    PluginInstanceID, SharedSchedule,
};
use crate::plugin::{PluginFactory, PluginSaveState};
use crate::plugin_scanner::PluginScanner;
use crate::{host_request::HostInfo, plugin_scanner::ScannedPluginKey};

pub struct RustyDAWEngine {
    audio_graph: Option<AudioGraph>,
    plugin_scanner: PluginScanner,
    //garbage_coll_handle: Option<std::thread::JoinHandle<()>>,
    //garbage_coll_run: Arc<AtomicBool>,
    event_tx: Sender<DAWEngineEvent>,
    collector: basedrop::Collector,
    host_info: Shared<HostInfo>,
}

impl RustyDAWEngine {
    pub fn new(
        garbage_collect_interval: Duration,
        host_info: HostInfo,
        mut internal_plugins: Vec<Box<dyn PluginFactory>>,
    ) -> (Self, Receiver<DAWEngineEvent>, Vec<Result<ScannedPluginKey, Box<dyn Error>>>) {
        // Set up and run garbage collector wich collects and safely drops garbage from
        // the audio thread.
        let collector = Collector::new();

        /*
        let coll_handle = collector.handle();
        let garbage_coll_run = Arc::new(AtomicBool::new(true));
        let garbage_coll_handle = run_garbage_collector_thread(
            collector,
            garbage_collect_interval,
            Arc::clone(&garbage_coll_run),
        );
        */

        let host_info = Shared::new(&collector.handle(), host_info);

        let mut plugin_scanner = PluginScanner::new(collector.handle(), Shared::clone(&host_info));

        let (event_tx, event_rx) = channel::unbounded::<DAWEngineEvent>();

        // Scan the user's internal plugins.
        let internal_plugins_res: Vec<Result<ScannedPluginKey, Box<dyn Error>>> =
            internal_plugins.drain(..).map(|p| plugin_scanner.scan_internal_plugin(p)).collect();

        (
            Self {
                audio_graph: None,
                plugin_scanner,
                //garbage_coll_handle: Some(garbage_coll_handle),
                //garbage_coll_run,
                event_tx,
                collector,
                //coll_handle,
                host_info,
            },
            event_rx,
            internal_plugins_res,
        )
    }

    pub fn get_graph_input_node_key(&self) {}

    #[cfg(feature = "clap-host")]
    pub fn add_clap_scan_directory<P: Into<PathBuf>>(&mut self, path: P) {
        let path: PathBuf = path.into();
        if self.plugin_scanner.add_clap_scan_directory(path.clone()) {
            self.event_tx.send(PluginScannerEvent::ClapScanPathAdded(path).into()).unwrap();
        }
    }

    #[cfg(feature = "clap-host")]
    pub fn remove_clap_scan_directory<P: Into<PathBuf>>(&mut self, path: P) {
        let path: PathBuf = path.into();
        if self.plugin_scanner.remove_clap_scan_directory(path.clone()) {
            self.event_tx.send(PluginScannerEvent::ClapScanPathRemoved(path).into()).unwrap();
        }
    }

    pub fn rescan_plugin_directories(&mut self) {
        let res = self.plugin_scanner.rescan_plugin_directories();
        self.event_tx.send(PluginScannerEvent::RescanFinished(res).into()).unwrap();
    }

    pub fn activate_engine(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        num_audio_in_channels: u16,
        num_audio_out_channels: u16,
    ) {
        if self.audio_graph.is_some() {
            log::warn!("Ignored request to activate RustyDAW engine: Engine is already activated");
            return;
        }

        log::info!("Activating RustyDAW engine...");

        let (mut audio_graph, shared_schedule) = AudioGraph::new(
            self.collector.handle(),
            Shared::clone(&self.host_info),
            num_audio_in_channels,
            num_audio_out_channels,
            sample_rate,
            min_frames,
            max_frames,
        );

        // TODO: Remove this once compiler is fixed.
        audio_graph
            .connect_edge(&Edge {
                edge_type: DefaultPortType::Audio,
                src_plugin_id: audio_graph.graph_in_node_id().clone(),
                dst_plugin_id: audio_graph.graph_out_node_id().clone(),
                src_channel: 0,
                dst_channel: 0,
            })
            .unwrap();
        audio_graph
            .connect_edge(&Edge {
                edge_type: DefaultPortType::Audio,
                src_plugin_id: audio_graph.graph_in_node_id().clone(),
                dst_plugin_id: audio_graph.graph_out_node_id().clone(),
                src_channel: 1,
                dst_channel: 1,
            })
            .unwrap();

        self.audio_graph = Some(audio_graph);

        self.compile_audio_graph();

        if let Some(audio_graph) = &self.audio_graph {
            log::info!("Successfully activated RustyDAW engine");

            let info = EngineActivatedInfo {
                shared_schedule,
                graph_in_node_id: audio_graph.graph_in_node_id().clone(),
                graph_out_node_id: audio_graph.graph_out_node_id().clone(),
                sample_rate,
                min_frames,
                max_frames,
                num_audio_in_channels,
                num_audio_out_channels,
            };

            self.event_tx.send(DAWEngineEvent::EngineActivated(info)).unwrap();
        } else {
            // If this happens then we did something very wrong.
            panic!("Unexpected error: Empty audio graph failed to compile a schedule.");
        }
    }

    pub fn modify_graph(&mut self, mut req: ModifyGraphRequest) {
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
                .map(|(key, preset)| {
                    if let Some(preset) = preset {
                        let save_state = PluginSaveState {
                            key: key.clone(),
                            _preset: preset.clone(),
                            activation_requested: true,
                            audio_in_out_channels: (0, 0),
                        };

                        audio_graph.add_new_plugin_instance(
                            save_state,
                            &mut self.plugin_scanner,
                            true,
                            true,
                        )
                    } else {
                        let save_state = PluginSaveState {
                            key: key.clone(),
                            _preset: (), // TODO: Get default preset.
                            activation_requested: true,
                            audio_in_out_channels: (0, 0),
                        };

                        audio_graph.add_new_plugin_instance(
                            save_state,
                            &mut self.plugin_scanner,
                            true,
                            true,
                        )
                    }
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
                    PluginIDReq::Existing(id) => id.clone(),
                    PluginIDReq::Added(index) => {
                        if let Some(new_plugin_id) = new_plugin_ids.get(*index) {
                            new_plugin_id.clone()
                        } else {
                            log::error!(
                                "Could not connect edge {:?}: Source plugin index out of bounds",
                                edge
                            );
                            continue;
                        }
                    }
                };

                let dst_plugin_id = match &edge.dst_plugin_id {
                    PluginIDReq::Existing(id) => id.clone(),
                    PluginIDReq::Added(index) => {
                        if let Some(new_plugin_id) = new_plugin_ids.get(*index) {
                            new_plugin_id.clone()
                        } else {
                            log::error!("Could not connect edge {:?}: Destination plugin index out of bounds", edge);
                            continue;
                        }
                    }
                };

                let new_edge = Edge {
                    edge_type: edge.edge_type,
                    src_plugin_id,
                    dst_plugin_id,
                    src_channel: edge.src_channel,
                    dst_channel: edge.dst_channel,
                };

                if let Err(e) = audio_graph.connect_edge(&new_edge) {
                    log::error!("Could not connect edge {:?}: {}", edge, e);
                } else {
                    let _ = affected_plugins.insert(new_edge.src_plugin_id.clone());
                    let _ = affected_plugins.insert(new_edge.dst_plugin_id.clone());
                }
            }

            // Don't include the graph in/out "plugins" in the result.
            let _ = affected_plugins.remove(audio_graph.graph_in_node_id());
            let _ = affected_plugins.remove(audio_graph.graph_out_node_id());

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

            self.event_tx.send(DAWEngineEvent::AudioGraphModified(res)).unwrap();
        } else {
            log::warn!("Cannot modify audio graph: Engine is deactivated");
        }
    }

    pub fn deactivate_engine(&mut self) {
        if self.audio_graph.is_none() {
            log::warn!("Ignored request to deactivate engine: Engine is already deactivated");
            return;
        }

        log::info!("Deactivating RustyDAW engine");

        let save_state = self.audio_graph.as_mut().unwrap().collect_save_state();

        self.audio_graph = None;

        self.event_tx.send(DAWEngineEvent::EngineDeactivated(Ok(save_state))).unwrap();
    }

    pub fn restore_audio_graph_from_save_state(&mut self, save_state: &AudioGraphSaveState) {
        if self.audio_graph.is_none() {
            log::warn!(
                "Ignored request to restore audio graph from save state: Engine is deactivated"
            );
            return;
        }

        log::info!("Restoring audio graph from save state...");

        log::debug!("Save state: {:?}", save_state);

        self.event_tx.send(DAWEngineEvent::AudioGraphCleared).unwrap();

        let (plugins_res, plugins_edges) = self
            .audio_graph
            .as_mut()
            .unwrap()
            .restore_from_save_state(save_state, &mut self.plugin_scanner, true);

        self.compile_audio_graph();

        if self.audio_graph.is_some() {
            log::info!("Restoring audio graph from save state successful");

            let save_state = self.audio_graph.as_mut().unwrap().collect_save_state();

            let res = ModifyGraphRes {
                new_plugins: plugins_res,
                removed_plugins: Vec::new(),
                updated_plugin_edges: plugins_edges,
            };

            self.event_tx.send(DAWEngineEvent::AudioGraphModified(res)).unwrap();

            self.event_tx.send(DAWEngineEvent::NewSaveState(save_state)).unwrap();
        }
    }

    pub fn request_latest_save_state(&mut self) {
        if self.audio_graph.is_none() {
            log::warn!(
                "Ignored request for the latest audio graph save state: Engine is deactivated"
            );
            return;
        }

        log::trace!("Got request for latest audio graph save state");

        // TODO: Collect save state in a separate thread?
        let save_state = self.audio_graph.as_mut().unwrap().collect_save_state();

        self.event_tx.send(DAWEngineEvent::NewSaveState(save_state)).unwrap();
    }

    /// Call this method periodically (every other frame or so). This is needed to properly
    /// handle the state of plugins.
    pub fn on_idle(&mut self) {
        if let Some(audio_graph) = &mut self.audio_graph {
            let (mut changed_plugins, recompile) = audio_graph.on_idle();

            for msg in changed_plugins.drain(..) {
                self.event_tx.send(msg).unwrap();
            }

            if recompile {
                self.compile_audio_graph();
            }
        }

        self.collector.collect();
    }

    fn compile_audio_graph(&mut self) {
        if let Some(mut audio_graph) = self.audio_graph.take() {
            match audio_graph.compile() {
                Ok(_) => {
                    self.audio_graph = Some(audio_graph);
                }
                Err(e) => {
                    log::error!("{}", e);

                    self.event_tx.send(DAWEngineEvent::EngineDeactivated(Err(e))).unwrap();

                    // Audio graph is in an invalid state. Drop it and have the user restore
                    // from the last working save state.
                    let _ = audio_graph;
                }
            }
        }
    }
}

impl Drop for RustyDAWEngine {
    fn drop(&mut self) {
        // Make sure all of the stuff in the audio thread gets dropped properly.
        let _ = self.audio_graph;

        self.collector.collect();

        /*
        self.garbage_coll_run.store(false, Ordering::Relaxed);
        if let Some(handle) = self.garbage_coll_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Error while stopping garbage collector thread: {:?}", e);
            }
        }
        */
    }
}

#[derive(Debug)]
pub struct EngineActivatedInfo {
    /// The realtime-safe shared audio graph schedule.
    ///
    /// Send this to the audio thread to be run.
    ///
    /// This will automatically sync with any changes in the audio
    /// graph engine, so no further schedules need to be sent to
    /// the audio thread (until the engine is deactivated).
    ///
    /// When a `DAWEngineEvent::EngineDeactivated` event is recieved, send
    /// a signal to the audio thread to drop this schedule.
    pub shared_schedule: SharedSchedule,

    /// The ID for the input to the audio graph. Use this to connect any
    /// plugins to system inputs.
    pub graph_in_node_id: PluginInstanceID,

    /// The ID for the output to the audio graph. Use this to connect any
    /// plugins to system outputs.
    pub graph_out_node_id: PluginInstanceID,

    pub sample_rate: SampleRate,
    pub min_frames: u32,
    pub max_frames: u32,
    pub num_audio_in_channels: u16,
    pub num_audio_out_channels: u16,
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
pub struct EdgeReq {
    pub edge_type: DefaultPortType,

    pub src_plugin_id: PluginIDReq,
    pub dst_plugin_id: PluginIDReq,

    pub src_channel: u16,
    pub dst_channel: u16,
}

#[derive(Debug, Clone)]
pub struct ModifyGraphRequest {
    /// Any new plugin instances to add.
    ///
    /// `(plugin key, plugin preset (None for default preset))`
    pub add_plugin_instances: Vec<(ScannedPluginKey, Option<()>)>,

    /// Any plugins to remove.
    pub remove_plugin_instances: Vec<PluginInstanceID>,

    /// Any new connections between plugins to add.
    pub connect_new_edges: Vec<EdgeReq>,

    /// Any connections between plugins to remove.
    pub disconnect_edges: Vec<Edge>,
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
