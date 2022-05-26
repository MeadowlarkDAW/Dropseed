use cpal::traits::{DeviceTrait, HostTrait};
use cpal::Stream;
use crossbeam::channel::Receiver;
use eframe::egui;
use rusty_daw_core::SampleRate;
use rusty_daw_engine::{
    DAWEngineEvent, Edge, EdgeReq, HostInfo, ModifyGraphRequest, PluginActivationStatus,
    PluginEdges, PluginIDReq, PluginInstanceID, PluginScannerEvent, PortType, RustyDAWEngine,
    ScannedPlugin, SharedSchedule,
};
use std::time::Duration;

mod effect_rack_page;
mod scanned_plugins_page;

use effect_rack_page::{AudioPortState, EffectRackPluginState, EffectRackState};

const MIN_BLOCK_SIZE: u32 = 1;
const MAX_BLOCK_SIZE: u32 = 512;
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

    engine.activate_engine(
        sample_rate,
        MIN_BLOCK_SIZE,
        MAX_BLOCK_SIZE,
        GRAPH_IN_CHANNELS,
        GRAPH_OUT_CHANNELS,
    );
    engine.rescan_plugin_directories();

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

pub struct EngineState {
    pub graph_in_node_id: PluginInstanceID,
    pub graph_out_node_id: PluginInstanceID,

    pub effect_rack_state: EffectRackState,
}

struct BasicDawExampleGUI {
    engine: RustyDAWEngine,
    engine_rx: Receiver<DAWEngineEvent>,

    to_audio_thread_tx: ringbuf::Producer<UIToAudioThreadMsg>,
    _cpal_stream: Option<Stream>,

    sample_rate: SampleRate,

    plugin_list: Vec<ScannedPlugin>,

    failed_plugins_text: Vec<(String, String)>,

    engine_state: Option<EngineState>,

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
            _cpal_stream: Some(cpal_stream),
            sample_rate,
            plugin_list: Vec::new(),
            failed_plugins_text: Vec::new(),
            engine_state: None,
            current_tab: Tab::EffectRack,
        }
    }

    fn poll_updates(&mut self) {
        for msg in self.engine_rx.try_iter() {
            //dbg!(&msg);

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
                    if let Err(e) = res {
                        println!("Engine crashed: {}", e);
                    }

                    self.to_audio_thread_tx.push(UIToAudioThreadMsg::DropSharedSchedule).unwrap();

                    self.engine_state = None;
                }

                // This message is sent whenever the engine successfully activates.
                DAWEngineEvent::EngineActivated(info) => {
                    self.engine_state = Some(EngineState {
                        graph_in_node_id: info.graph_in_node_id,
                        graph_out_node_id: info.graph_out_node_id,
                        effect_rack_state: EffectRackState::new(),
                    });

                    self.to_audio_thread_tx
                        .push(UIToAudioThreadMsg::NewSharedSchedule(info.shared_schedule))
                        .unwrap();
                }

                // When this message is received, it means that the audio graph is starting
                // the process of restoring from a save state.
                //
                // Reset your UI as if you are loading up a project for the first time, and
                // wait for the `AudioGraphModified` event to repopulate the UI.
                //
                // If the audio graph is in an invalid state as a result of restoring from
                // the save state, then the `EngineDeactivated(Err(e))` event
                // will be sent instead.
                DAWEngineEvent::AudioGraphCleared => {
                    if let Some(engine_state) = &mut self.engine_state {
                        engine_state.effect_rack_state.plugins.clear();
                    }
                }

                // This message is sent whenever the audio graph has been modified.
                //
                // Be sure to update your UI from this new state.
                DAWEngineEvent::AudioGraphModified(mut res) => {
                    if let Some(engine_state) = &mut self.engine_state {
                        for plugin_id in res.removed_plugins.drain(..) {
                            engine_state.effect_rack_state.remove_plugin(&plugin_id);
                        }

                        for new_plugin_res in res.new_plugins.drain(..) {
                            let mut found = None;
                            for p in self.plugin_list.iter() {
                                if p.rdn() == new_plugin_res.plugin_id.rdn() {
                                    found = Some(p.description.name.clone());
                                    break;
                                }
                            }
                            let plugin_name = found.unwrap();

                            let effect_rack_plugin = match new_plugin_res.status {
                                PluginActivationStatus::Activated { audio_ports } => {
                                    EffectRackPluginState {
                                        plugin_name,
                                        plugin_id: new_plugin_res.plugin_id,
                                        audio_ports_state: Some(AudioPortState::new(audio_ports)),
                                        selected_port: Default::default(),
                                        active: true,
                                    }
                                }
                                PluginActivationStatus::Inactive => EffectRackPluginState {
                                    plugin_name,
                                    plugin_id: new_plugin_res.plugin_id,
                                    audio_ports_state: None,
                                    selected_port: Default::default(),
                                    active: false,
                                },
                                PluginActivationStatus::LoadError(e) => {
                                    println!("Plugin failed to load: {}", e);

                                    EffectRackPluginState {
                                        plugin_name,
                                        plugin_id: new_plugin_res.plugin_id,
                                        audio_ports_state: None,
                                        selected_port: Default::default(),
                                        active: false,
                                    }
                                }
                                PluginActivationStatus::ActivationError(e) => {
                                    println!("Plugin failed to activate: {}", e);

                                    EffectRackPluginState {
                                        plugin_name,
                                        plugin_id: new_plugin_res.plugin_id,
                                        audio_ports_state: None,
                                        selected_port: Default::default(),
                                        active: false,
                                    }
                                }
                            };

                            engine_state.effect_rack_state.plugins.push(effect_rack_plugin);
                        }

                        for (plugin_id, plugin_edges) in res.updated_plugin_edges.drain(..) {
                            let effect_rack_plugin =
                                engine_state.effect_rack_state.plugin_mut(&plugin_id).unwrap();

                            if let Some(audio_ports_state) =
                                &mut effect_rack_plugin.audio_ports_state
                            {
                                audio_ports_state.sync_with_new_edges(&plugin_edges);
                            }
                        }
                    }
                }

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
                } => {
                    if let Some(engine_state) = &mut self.engine_state {
                        let effect_rack_plugin =
                            engine_state.effect_rack_state.plugin_mut(&plugin_id).unwrap();

                        effect_rack_plugin.active = false;

                        if let Err(e) = status {
                            println!("Plugin failed to activate: {}", e);
                        }
                    }
                }

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
                } => {
                    if let Some(engine_state) = &mut self.engine_state {
                        let effect_rack_plugin =
                            engine_state.effect_rack_state.plugin_mut(&plugin_id).unwrap();

                        effect_rack_plugin.active = true;
                        effect_rack_plugin.audio_ports_state =
                            Some(AudioPortState::new(new_audio_ports));
                    }
                }

                DAWEngineEvent::PluginScanner(event) => match event {
                    // A new CLAP plugin scan path was added.
                    PluginScannerEvent::ClapScanPathAdded(path) => {
                        println!("Added clap scan path: {:?}", path);
                    }
                    // A CLAP plugin scan path was removed.
                    PluginScannerEvent::ClapScanPathRemoved(path) => {
                        println!("Removed clap scan path: {:?}", path);
                    }
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

                        if let Some(engine_state) = &mut self.engine_state {
                            let mut index = None;
                            for (i, p) in self.plugin_list.iter().enumerate() {
                                if p.rdn() == "com.moist-plugins-gmbh-egui.gain-gui" {
                                    index = Some(i);
                                    break;
                                }
                            }

                            if index.is_none() {
                                continue;
                            }

                            let req = ModifyGraphRequest {
                                add_plugin_instances: vec![(
                                    self.plugin_list[index.unwrap()].key.clone(),
                                    None,
                                )],
                                remove_plugin_instances: vec![],
                                connect_new_edges: vec![
                                    EdgeReq {
                                        edge_type: PortType::Audio,
                                        src_plugin_id: PluginIDReq::Existing(
                                            engine_state.graph_in_node_id.clone(),
                                        ),
                                        dst_plugin_id: PluginIDReq::Added(0),
                                        src_channel: 0,
                                        dst_channel: 0,
                                    },
                                    EdgeReq {
                                        edge_type: PortType::Audio,
                                        src_plugin_id: PluginIDReq::Existing(
                                            engine_state.graph_in_node_id.clone(),
                                        ),
                                        dst_plugin_id: PluginIDReq::Added(0),
                                        src_channel: 1,
                                        dst_channel: 1,
                                    },
                                    EdgeReq {
                                        edge_type: PortType::Audio,
                                        src_plugin_id: PluginIDReq::Added(0),
                                        dst_plugin_id: PluginIDReq::Existing(
                                            engine_state.graph_out_node_id.clone(),
                                        ),
                                        src_channel: 0,
                                        dst_channel: 0,
                                    },
                                    EdgeReq {
                                        edge_type: PortType::Audio,
                                        src_plugin_id: PluginIDReq::Added(0),
                                        dst_plugin_id: PluginIDReq::Existing(
                                            engine_state.graph_out_node_id.clone(),
                                        ),
                                        src_channel: 1,
                                        dst_channel: 1,
                                    },
                                ],
                                disconnect_edges: vec![],
                                /*
                                disconnect_edges: vec![
                                    Edge {
                                        edge_type: PortType::Audio,
                                        src_plugin_id: engine_state.graph_in_node_id.clone(),
                                        dst_plugin_id: engine_state.graph_out_node_id.clone(),
                                        src_channel: 0,
                                        dst_channel: 0,
                                    },
                                    Edge {
                                        edge_type: PortType::Audio,
                                        src_plugin_id: engine_state.graph_in_node_id.clone(),
                                        dst_plugin_id: engine_state.graph_out_node_id.clone(),
                                        src_channel: 1,
                                        dst_channel: 1,
                                    },
                                ],
                                */
                            };

                            self.engine.modify_graph(req);
                        }
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
                    if let Some(state) = &self.engine_state {
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

        self.engine.on_idle();
    }

    fn on_exit(&mut self, _gl: &eframe::glow::Context) {
        self._cpal_stream = None;
    }
}

#[derive(PartialEq)]
enum Tab {
    EffectRack,
    ScannedPlugins,
}
