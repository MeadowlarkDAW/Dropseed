use audio_graph::DefaultPortType;
use basedrop::{Collector, Shared};
use crossbeam::channel::{self, Receiver, Sender};
use rusty_daw_core::SampleRate;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use crate::event::{DAWEngineEvent, PluginScannerEvent};
use crate::graph::{
    AudioGraph, AudioGraphSaveState, Edge, PluginActivatedInfo, PluginInstanceID, SharedSchedule,
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
            self.event_tx.send(PluginScannerEvent::ScanPathAdded(path).into()).unwrap();
        }
    }

    #[cfg(feature = "clap-host")]
    pub fn remove_clap_scan_directory<P: Into<PathBuf>>(&mut self, path: P) {
        let path: PathBuf = path.into();
        if self.plugin_scanner.remove_clap_scan_directory(path.clone()) {
            self.event_tx.send(PluginScannerEvent::ScanPathRemoved(path).into()).unwrap();
        }
    }

    pub fn rescan_plugin_directories(&mut self) {
        self.plugin_scanner.rescan_plugin_directories(&mut self.event_tx);
    }

    pub fn activate_engine(
        &mut self,
        sample_rate: SampleRate,
        min_block_frames: usize,
        max_block_frames: usize,
        num_audio_in_channels: u16,
        num_audio_out_channels: u16,
    ) -> Result<(SharedSchedule, PluginInstanceID, PluginInstanceID), ()> {
        if self.audio_graph.is_some() {
            log::warn!("Engine is already activated");
            return Err(());
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

            Ok((
                shared_schedule,
                audio_graph.graph_in_node_id().clone(),
                audio_graph.graph_out_node_id().clone(),
            ))
        } else {
            // If this happens then we did something very wrong.
            panic!("Unexpected error: Empty audio graph failed to compile a schedule.");
        }
    }

    pub fn insert_new_plugin_between_main_ports(
        &mut self,
        save_state: &PluginSaveState,
        src_plugin_id: &PluginInstanceID,
        dst_plugin_id: &PluginInstanceID,
    ) {
        if let Some(audio_graph) = &mut self.audio_graph {
            match audio_graph.insert_new_plugin_between_main_ports(
                save_state,
                &mut self.plugin_scanner,
                true,
                src_plugin_id,
                dst_plugin_id,
            ) {
                Ok(res) => {
                    self.event_tx.send(DAWEngineEvent::PluginInsertedBetween(res)).unwrap();
                }
                Err(e) => {
                    log::error!("Could not insert new plugin instance between: {}", e);
                }
            }
        } else {
            log::warn!("Cannot insert new audio plugin: Engine is deactivated");
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
                Some(Some(res)) => {
                    self.event_tx.send(DAWEngineEvent::PluginRemovedBetween(res)).unwrap();
                }
                Some(None) => {
                    self.event_tx.send(DAWEngineEvent::PluginRemoved(plugin_id.clone())).unwrap();
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

        self.event_tx.send(DAWEngineEvent::EngineDeactivated(save_state)).unwrap();
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

        let restored_info = self.audio_graph.as_mut().unwrap().restore_from_save_state(
            save_state,
            &mut self.plugin_scanner,
            true,
        );

        self.compile_audio_graph();

        if self.audio_graph.is_some() {
            log::info!("Restoring audio graph from save state successful");

            self.event_tx
                .send(DAWEngineEvent::AudioGraphRestoredFromSaveState(restored_info))
                .unwrap();
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
            let restarted_plugins = audio_graph.on_main_thread();

            for (plugin_id, success) in restarted_plugins.iter() {
                if *success {
                    let edges = audio_graph.get_plugin_edges(plugin_id).unwrap();
                    let save_state = audio_graph.get_plugin_save_state(plugin_id).unwrap().clone();

                    let info = PluginActivatedInfo { id: plugin_id.clone(), edges, save_state };

                    self.event_tx.send(DAWEngineEvent::PluginRestarted(info)).unwrap();
                } else {
                    self.event_tx
                        .send(DAWEngineEvent::PluginFailedToRestart(plugin_id.clone()))
                        .unwrap();
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

                    self.event_tx
                        .send(DAWEngineEvent::EngineDeactivatedBecauseGraphIsInvalid(e))
                        .unwrap();

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
