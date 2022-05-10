use audio_graph::DefaultPortType;
use basedrop::{Collector, Shared};
use crossbeam::channel::{self, Receiver, Sender};
use rusty_daw_core::SampleRate;
use smallvec::SmallVec;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use crate::event::{DAWEngineEvent, PluginScannerEvent};
use crate::graph::{
    AudioGraph, AudioGraphSaveState, Edge, PluginActivatedInfo, PluginEdges,
    PluginEdgesChangedInfo, PluginInstanceID, SharedSchedule,
};
use crate::plugin::{PluginFactory, PluginSaveState};
use crate::plugin_scanner::PluginScanner;
use crate::{
    garbage_collector::run_garbage_collector_thread, host_request::HostInfo,
    plugin_scanner::ScannedPluginKey,
};

pub struct RustyDAWEngine {
    audio_graph: Option<AudioGraph>,
    plugin_scanner: PluginScanner,
    garbage_coll_handle: Option<std::thread::JoinHandle<()>>,
    garbage_coll_run: Arc<AtomicBool>,
    event_tx: Sender<DAWEngineEvent>,
    coll_handle: basedrop::Handle,
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
        let coll_handle = collector.handle();
        let garbage_coll_run = Arc::new(AtomicBool::new(true));
        let garbage_coll_handle = run_garbage_collector_thread(
            collector,
            garbage_collect_interval,
            Arc::clone(&garbage_coll_run),
        );

        let mut plugin_scanner = PluginScanner::new(coll_handle.clone());

        let host_info = Shared::new(&coll_handle, host_info);

        let (event_tx, event_rx) = channel::unbounded::<DAWEngineEvent>();

        // Scan the user's internal plugins.
        let internal_plugins_res: Vec<Result<ScannedPluginKey, Box<dyn Error>>> =
            internal_plugins.drain(..).map(|p| plugin_scanner.scan_internal_plugin(p)).collect();

        (
            Self {
                audio_graph: None,
                plugin_scanner,
                garbage_coll_handle: Some(garbage_coll_handle),
                garbage_coll_run,
                event_tx,
                coll_handle,
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
        min_block_frames: usize,
        max_block_frames: usize,
        num_audio_in_channels: u16,
        num_audio_out_channels: u16,
    ) {
        if self.audio_graph.is_some() {
            log::warn!("Ignored request to activate RustyDAW engine: Engine is already activated");
            return;
        }

        log::info!("Activating RustyDAW engine...");

        let (mut audio_graph, shared_schedule) = AudioGraph::new(
            self.coll_handle.clone(),
            Shared::clone(&self.host_info),
            num_audio_in_channels,
            num_audio_out_channels,
            sample_rate,
            min_block_frames,
            max_block_frames,
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
                min_block_frames,
                max_block_frames,
                num_audio_in_channels,
                num_audio_out_channels,
            };

            self.event_tx.send(DAWEngineEvent::EngineActivated(info)).unwrap();
        } else {
            // If this happens then we did something very wrong.
            panic!("Unexpected error: Empty audio graph failed to compile a schedule.");
        }
    }

    pub fn add_new_plugin_instance(
        &mut self,
        key: &ScannedPluginKey,
        preset: Option<&()>, // TODO
        activate: bool,
    ) {
        if let Some(audio_graph) = &mut self.audio_graph {
            let res = if let Some(preset) = preset {
                let save_state = PluginSaveState {
                    key: key.clone(),
                    _preset: preset.clone(),
                    activation_requested: activate,
                    audio_in_out_channels: (0, 0),
                };

                audio_graph.add_new_plugin_instance(
                    key,
                    Some(save_state),
                    &mut self.plugin_scanner,
                    activate,
                    true,
                )
            } else {
                audio_graph.add_new_plugin_instance(
                    key,
                    None,
                    &mut self.plugin_scanner,
                    activate,
                    true,
                )
            };

            self.event_tx.send(DAWEngineEvent::PluginInstancesAdded(vec![res])).unwrap();
        } else {
            log::warn!("Cannot insert new audio plugin: Engine is deactivated");
        }
    }

    pub fn insert_new_plugin_between_main_ports(
        &mut self,
        key: &ScannedPluginKey,
        preset: Option<&()>, // TODO
        src_plugin_id: &PluginInstanceID,
        dst_plugin_id: &PluginInstanceID,
        activate: bool,
    ) {
        if let Some(audio_graph) = &mut self.audio_graph {
            let res = if let Some(preset) = preset {
                let save_state = PluginSaveState {
                    key: key.clone(),
                    _preset: preset.clone(),
                    activation_requested: activate,
                    audio_in_out_channels: (0, 0),
                };

                audio_graph.insert_new_plugin_between_main_ports(
                    key,
                    Some(save_state),
                    &mut self.plugin_scanner,
                    true,
                    src_plugin_id,
                    dst_plugin_id,
                    activate,
                )
            } else {
                audio_graph.insert_new_plugin_between_main_ports(
                    key,
                    None,
                    &mut self.plugin_scanner,
                    true,
                    src_plugin_id,
                    dst_plugin_id,
                    activate,
                )
            };

            match res {
                Ok((res, plugins_new_edges)) => {
                    self.event_tx.send(DAWEngineEvent::PluginInstancesAdded(vec![res])).unwrap();
                    self.event_tx
                        .send(DAWEngineEvent::PluginEdgesChanged(plugins_new_edges))
                        .unwrap();
                }
                Err(e) => {
                    log::error!("Could not insert new plugin instance between: {}", e);
                }
            }
        } else {
            log::warn!("Cannot insert new audio plugin: Engine is deactivated");
        }
    }

    pub fn remove_plugin_instances(&mut self, plugin_ids: Vec<PluginInstanceID>) {
        if let Some(audio_graph) = &mut self.audio_graph {
            let (removed_plugins, affected_plugins) =
                audio_graph.remove_plugin_instances(&plugin_ids);

            let mut plugins_new_edges: Vec<(PluginInstanceID, PluginEdges)> = Vec::new();
            for id in plugin_ids.iter() {
                plugins_new_edges.push((
                    id.clone(),
                    PluginEdges { incoming: SmallVec::new(), outgoing: SmallVec::new() },
                ));
            }
            for id in affected_plugins.iter() {
                let edges = audio_graph.get_plugin_edges(&id).unwrap();
                plugins_new_edges.push((id.clone(), edges));
            }

            self.event_tx
                .send(DAWEngineEvent::PluginEdgesChanged(PluginEdgesChangedInfo {
                    plugins_new_edges,
                }))
                .unwrap();
            self.event_tx.send(DAWEngineEvent::PluginInstancesRemoved(removed_plugins)).unwrap();
        } else {
            log::warn!("Ignored request to remove plugin instance: Engine is deactivated");
        }
    }

    pub fn remove_plugin_between(
        &mut self,
        plugin_id: &PluginInstanceID,
        src_plugin_id: &PluginInstanceID,
        dst_plugin_id: &PluginInstanceID,
    ) {
        if let Some(audio_graph) = &mut self.audio_graph {
            match audio_graph.remove_plugin_between(plugin_id, src_plugin_id, dst_plugin_id) {
                Some(plugins_new_edges) => {
                    self.event_tx
                        .send(DAWEngineEvent::PluginEdgesChanged(plugins_new_edges))
                        .unwrap();
                    self.event_tx
                        .send(DAWEngineEvent::PluginInstancesRemoved(vec![plugin_id.clone()]))
                        .unwrap();
                }
                _ => {}
            }
        } else {
            log::warn!("Ignored request to remove plugin instance: Engine is deactivated");
        }
    }

    pub fn deactivate_engine(&mut self) {
        if self.audio_graph.is_none() {
            log::warn!("Ignored request to deactivate engine: Engine is already deactivated");
            return;
        }

        log::info!("Deactivating RustyDAW engine");

        let save_state = self.audio_graph.as_ref().unwrap().collect_save_state();

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

        let (restored_info, plugins_res, plugins_edges) = self
            .audio_graph
            .as_mut()
            .unwrap()
            .restore_from_save_state(save_state, &mut self.plugin_scanner, true);

        self.compile_audio_graph();

        if self.audio_graph.is_some() {
            log::info!("Restoring audio graph from save state successful");

            let save_state = self.audio_graph.as_mut().unwrap().collect_save_state();

            self.event_tx
                .send(DAWEngineEvent::AudioGraphRestoredFromSaveState(restored_info))
                .unwrap();

            self.event_tx.send(DAWEngineEvent::PluginInstancesAdded(plugins_res)).unwrap();
            self.event_tx.send(DAWEngineEvent::PluginEdgesChanged(plugins_edges)).unwrap();

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
    pub fn on_main_thread(&mut self) {
        if let Some(audio_graph) = &mut self.audio_graph {
            let mut restarted_plugins = audio_graph.on_main_thread();

            for (plugin_id, res) in restarted_plugins.drain(..) {
                if let Err(e) = res {
                    self.event_tx
                        .send(DAWEngineEvent::PluginFailedToRestart(plugin_id.clone(), e))
                        .unwrap();
                } else {
                    let edges = audio_graph.get_plugin_edges(&plugin_id).unwrap();
                    let save_state = audio_graph.get_plugin_save_state(&plugin_id).unwrap().clone();

                    let info = PluginActivatedInfo { id: plugin_id.clone(), edges, save_state };

                    self.event_tx.send(DAWEngineEvent::PluginRestarted(info)).unwrap();
                }
            }

            if !restarted_plugins.is_empty() {
                self.compile_audio_graph();
            }
        }
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
        self.garbage_coll_run.store(false, Ordering::Relaxed);
        if let Some(handle) = self.garbage_coll_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Error while stopping garbage collector thread: {:?}", e);
            }
        }
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
    pub min_block_frames: usize,
    pub max_block_frames: usize,
    pub num_audio_in_channels: u16,
    pub num_audio_out_channels: u16,
}
