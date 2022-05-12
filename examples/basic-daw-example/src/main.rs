use cpal::traits::{DeviceTrait, HostTrait};
use cpal::Stream;
use crossbeam::channel::Receiver;
use eframe::egui;
use rusty_daw_core::SampleRate;
use rusty_daw_engine::{
    DAWEngineEvent, HostInfo, PluginInstanceID, PluginScannerEvent, RustyDAWEngine, ScannedPlugin,
    SharedSchedule,
};
use std::time::Duration;

mod effect_rack_page;
mod scanned_plugins_page;

const MIN_BLOCK_SIZE: usize = 1;
const MAX_BLOCK_SIZE: usize = 512;
const GRAPH_IN_CHANNELS: u16 = 2;
const GRAPH_OUT_CHANNELS: u16 = 2;

fn main() {
    mowl::init().unwrap();

    let (to_audio_thread_tx, mut from_gui_rx) =
        ringbuf::RingBuffer::<UIToAudioThreadMsg>::new(10).split();

    // ---  Initialize cpal stream  -----------------------------------------------

    let cpal_host = cpal::default_host();

    let device = cpal_host.default_output_device().expect("no output device available");

    let config = device.default_output_config().expect("no default output config available");

    let num_out_channels = usize::from(config.channels());
    let sample_rate: SampleRate = config.sample_rate().0.into();

    let mut shared_schedule: Option<SharedSchedule> = None;

    let cpal_stream = device
        .build_output_stream(
            &config.into(),
            move |audio_buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                while let Some(msg) = from_gui_rx.pop() {
                    match msg {
                        UIToAudioThreadMsg::NewSharedSchedule(schedule) => {
                            shared_schedule = Some(schedule);
                        }
                        UIToAudioThreadMsg::DropSharedSchedule => {
                            shared_schedule = None;
                        }
                    }
                }

                if let Some(shared_schedule) = &mut shared_schedule {
                    shared_schedule
                        .process_cpal_interleaved_output_only(num_out_channels, audio_buffer);
                }
            },
            |e| {
                panic!("{}", e);
            },
        )
        .unwrap();

    // ---  Initialize RustyDAW Engine  -------------------------------------------

    let (mut engine, engine_rx, internal_scan_res) = RustyDAWEngine::new(
        Duration::from_secs(3),
        HostInfo::new(String::from("RustyDAW integration test"), String::from("0.1.0"), None, None),
        Vec::new(),
    );

    engine.rescan_plugin_directories();
    engine.activate_engine(
        sample_rate,
        MIN_BLOCK_SIZE,
        MAX_BLOCK_SIZE,
        GRAPH_IN_CHANNELS,
        GRAPH_OUT_CHANNELS,
    );

    // ---  Run the UI  -----------------------------------------------------------

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Basic DAW Example",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());

            Box::new(BasicDawExampleGUI::new(
                engine,
                engine_rx,
                cpal_stream,
                sample_rate,
                to_audio_thread_tx,
            ))
        }),
    );
}

#[derive(Debug)]
enum UIToAudioThreadMsg {
    NewSharedSchedule(SharedSchedule),
    DropSharedSchedule,
}

pub struct ActiveEngineState {
    pub graph_in_node_id: PluginInstanceID,
    pub graph_out_node_id: PluginInstanceID,
}

struct BasicDawExampleGUI {
    engine: RustyDAWEngine,
    engine_rx: Receiver<DAWEngineEvent>,

    to_audio_thread_tx: ringbuf::Producer<UIToAudioThreadMsg>,
    _cpal_stream: Stream,

    sample_rate: SampleRate,

    plugin_list: Vec<ScannedPlugin>,

    failed_plugins_text: Vec<(String, String)>,

    active_engine_state: Option<ActiveEngineState>,

    current_tab: Tab,
}

impl BasicDawExampleGUI {
    fn new(
        engine: RustyDAWEngine,
        engine_rx: Receiver<DAWEngineEvent>,
        cpal_stream: Stream,
        sample_rate: SampleRate,
        to_audio_thread_tx: ringbuf::Producer<UIToAudioThreadMsg>,
    ) -> Self {
        Self {
            engine,
            engine_rx,
            to_audio_thread_tx,
            _cpal_stream: cpal_stream,
            sample_rate,
            plugin_list: Vec::new(),
            failed_plugins_text: Vec::new(),
            active_engine_state: None,
            current_tab: Tab::ScannedPlugins,
        }
    }

    fn poll_updates(&mut self) {
        for msg in self.engine_rx.try_iter() {
            dbg!(&msg);

            match msg {
                // Sent whenever the engine is deactivated.
                //
                // If the result is `Ok(save_state)`, then it means that the engine
                // deactivated gracefully via calling `RustyDAWEngine::deactivate_engine()`,
                // and the latest save state of the audio graph is returned.
                //
                // If the result is `Err(e)`, then it means that the engine deactivated
                // because of a unrecoverable audio graph compiler error.
                //
                // To keep using the audio graph, you must reactivate the engine with
                // `RustyDAWEngine::activate_engine()`, and then restore the audio graph
                // from an existing save state if you wish using
                // `RustyDAWEngine::restore_audio_graph_from_save_state()`.
                DAWEngineEvent::EngineDeactivated(res) => {
                    self.to_audio_thread_tx.push(UIToAudioThreadMsg::DropSharedSchedule).unwrap();

                    self.active_engine_state = None;
                }

                // This message is sent whenever the engine successfully activates.
                DAWEngineEvent::EngineActivated(info) => {
                    self.active_engine_state = Some(ActiveEngineState {
                        graph_in_node_id: info.graph_in_node_id,
                        graph_out_node_id: info.graph_out_node_id,
                    });

                    self.to_audio_thread_tx
                        .push(UIToAudioThreadMsg::NewSharedSchedule(info.shared_schedule))
                        .unwrap();
                }

                // When this message is received, it means that the audio graph is starting
                // the process of restoring from a save state.
                //
                // Reset your UI as if you are loading up a project for the first time, and
                // wait for the `PluginInstancesAdded` and `PluginEdgesChanged` events in
                // order to repopulate the UI.
                //
                // If the audio graph is in an invalid state as a result of restoring from
                // the save state, then the `EngineDeactivatedBecauseGraphIsInvalid` event
                // will be sent instead.
                DAWEngineEvent::AudioGraphCleared => {}

                // When this message is received, it means that the audio graph has finished
                // the process of restoring from a save state.
                DAWEngineEvent::AudioGraphRestoredFromSaveState(info) => {}

                // This message is sent after the user requests the latest save state from
                // calling `RustyDAWEngine::request_latest_save_state()`.
                //
                // Use the latest save state as a backup in case a plugin crashes or a bug
                // in the audio graph compiler causes the audio graph to be in an invalid
                // state, resulting in the audio engine stopping.
                DAWEngineEvent::NewSaveState(audio_graph_save_state) => {}

                // This message is sent whenever new plugins are added to the audio graph.
                DAWEngineEvent::PluginInstancesAdded(instances) => {}

                // This message is sent whenever plugins are removed from the audio graph.
                //
                // Note that the host will always send a `PluginEdgesChanged` event
                // before this event if any of the removed plugins had connected
                // edges. This `PluginEdgesChanged` event will have all edges that
                // were connected to any of the removed plugins removed.
                DAWEngineEvent::PluginInstancesRemoved(instances) => {}

                // This message is sent whenever the edges (connections of ports) of
                // plugins change (including when adding/removing plugins).
                DAWEngineEvent::PluginEdgesChanged(info) => {}

                // Sent whenever a plugin becomes deactivated. When a plugin is deactivated
                // you cannot access any of its methods until it is reactivated.
                DAWEngineEvent::PluginDeactivated {
                    plugin_id,
                    // If this is `Ok(())`, then it means the plugin was gracefully
                    // deactivated from user request.
                    //
                    // If this is `Err(e)`, then it means the plugin became deactivated
                    // because it failed to restart.
                    status,
                } => {}

                // Sent whenever a plugin becomes activated after being deactivated or
                // when the plugin restarts.
                //
                // Make sure your UI updates the port configuration on this plugin.
                DAWEngineEvent::PluginActivated {
                    plugin_id,
                    // If this is `Some(audio_ports)`, then it means that the plugin has
                    // updated its audio port configuration.
                    //
                    // If this is `None`, then it means that the plugin has not changed
                    // its audio port configuration since the last time it was activated.
                    new_audio_ports,
                } => {}

                DAWEngineEvent::PluginScanner(event) => match event {
                    // A new CLAP plugin scan path was added.
                    PluginScannerEvent::ClapScanPathAdded(path) => {}
                    // A CLAP plugin scan path was removed.
                    PluginScannerEvent::ClapScanPathRemoved(path) => {}
                    // A request to rescan all plugin directories has finished. Update
                    // the list of available plugins in your UI.
                    PluginScannerEvent::RescanFinished(mut info) => {
                        self.plugin_list = info.scanned_plugins;

                        self.failed_plugins_text = info
                            .failed_plugins
                            .drain(..)
                            .map(|(path, error)| {
                                (format!("{}", path.to_string_lossy()), format!("{}", error))
                            })
                            .collect();
                    }
                    unkown_event => {
                        dbg!(unkown_event);
                    }
                },
                unkown_event => {
                    dbg!(unkown_event);
                }
            }
        }
    }
}

impl eframe::App for BasicDawExampleGUI {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll_updates();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::EffectRack, "FX Rack");
                ui.selectable_value(&mut self.current_tab, Tab::ScannedPlugins, "Scanned Plugins");

                ui.with_layout(egui::Layout::right_to_left(), |ui| {
                    if let Some(state) = &self.active_engine_state {
                        ui.label(format!("sample rate: {}", self.sample_rate.as_u16()));
                        ui.colored_label(egui::Color32::GREEN, "active");
                        ui.label("engine status:");

                        if ui.button("deactivate").clicked() {
                            self.engine.deactivate_engine();
                        }
                    } else {
                        ui.colored_label(egui::Color32::RED, "inactive");
                        ui.label("engine status:");

                        if ui.button("activate").clicked() {
                            self.engine.activate_engine(
                                self.sample_rate,
                                MIN_BLOCK_SIZE,
                                MAX_BLOCK_SIZE,
                                GRAPH_IN_CHANNELS,
                                GRAPH_OUT_CHANNELS,
                            );
                        }
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
            Tab::EffectRack => effect_rack_page::show(self, ui),
            Tab::ScannedPlugins => scanned_plugins_page::show(self, ui),
        });
    }
}

#[derive(PartialEq)]
enum Tab {
    EffectRack,
    ScannedPlugins,
}
