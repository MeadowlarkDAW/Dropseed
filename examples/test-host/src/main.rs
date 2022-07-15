use cpal::traits::{DeviceTrait, HostTrait};
use cpal::Stream;
use crossbeam_channel::Receiver;
use dropseed::plugin::{HostInfo, PluginInstanceID};
use dropseed::{
    ActivateEngineSettings, DSEngineAudioThread, DSEngineEvent, DSEngineHandle, DSEngineRequest,
    EngineDeactivatedInfo, PluginActivationStatus, PluginEvent, PluginScannerEvent, ScannedPlugin,
};
use eframe::egui;
use meadowlark_core_types::SampleRate;

mod effect_rack_page;
mod scanned_plugins_page;

use effect_rack_page::{EffectRackPluginActiveState, EffectRackPluginState, EffectRackState};

const MIN_BLOCK_SIZE: u32 = 1;
const MAX_BLOCK_SIZE: u32 = 512;
const GRAPH_IN_CHANNELS: u16 = 2;
const GRAPH_OUT_CHANNELS: u16 = 2;

fn main() {
    // Prefer to use a logging crate that is wait-free for threads printing
    // out to the log.
    fast_log::init(fast_log::Config::new().console().level(log::LevelFilter::Trace)).unwrap();

    let (to_audio_thread_tx, mut from_gui_rx) =
        ringbuf::RingBuffer::<UIToAudioThreadMsg>::new(10).split();

    // ---  Initialize cpal stream  -----------------------------------------------

    let cpal_host = cpal::default_host();

    let device = cpal_host.default_output_device().expect("no output device available");

    let config = device.default_output_config().expect("no default output config available");

    let num_out_channels = usize::from(config.channels());
    let sample_rate: SampleRate = config.sample_rate().0.into();

    let mut audio_thread: Option<DSEngineAudioThread> = None;

    let cpal_stream = device
        .build_output_stream(
            &config.into(),
            move |audio_buffer: &mut [f32], _: &cpal::OutputCallbackInfo| {
                while let Some(msg) = from_gui_rx.pop() {
                    match msg {
                        UIToAudioThreadMsg::NewEngineAudioThread(new_audio_thread) => {
                            audio_thread = Some(new_audio_thread);
                        }
                        UIToAudioThreadMsg::DropEngineAudioThread => {
                            audio_thread = None;
                        }
                    }
                }

                if let Some(audio_thread) = &mut audio_thread {
                    audio_thread
                        .process_cpal_interleaved_output_only(num_out_channels, audio_buffer);
                }
            },
            |e| {
                panic!("{:?}", e);
            },
        )
        .unwrap();

    // ---  Initialize Dropseed Engine  -------------------------------------------

    let (mut engine_handle, engine_rx) = DSEngineHandle::new(
        HostInfo::new(String::from("Dropseed Example"), String::from("0.1.0"), None, None),
        //vec![Box::new(NoiseGenPluginFactory {})],
        vec![],
    );

    dbg!(&engine_handle.internal_plugins_res);

    engine_handle.send(DSEngineRequest::ActivateEngine(Box::new(ActivateEngineSettings {
        sample_rate,
        min_frames: MIN_BLOCK_SIZE,
        max_frames: MAX_BLOCK_SIZE,
        num_audio_in_channels: GRAPH_IN_CHANNELS,
        num_audio_out_channels: GRAPH_OUT_CHANNELS,
        ..Default::default()
    })));

    engine_handle.send(DSEngineRequest::RescanPluginDirectories);

    // ---  Run the UI  -----------------------------------------------------------

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Dropseed Example",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());

            Box::new(DSExampleGUI::new(
                engine_handle,
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
    NewEngineAudioThread(DSEngineAudioThread),
    DropEngineAudioThread,
}

pub struct EngineState {
    pub graph_in_node_id: PluginInstanceID,
    pub graph_out_node_id: PluginInstanceID,

    pub effect_rack_state: EffectRackState,
}

struct DSExampleGUI {
    engine_handle: DSEngineHandle,
    engine_rx: Receiver<DSEngineEvent>,

    to_audio_thread_tx: ringbuf::Producer<UIToAudioThreadMsg>,
    _cpal_stream: Option<Stream>,

    sample_rate: SampleRate,

    plugin_list: Vec<(ScannedPlugin, String)>,

    failed_plugins_text: Vec<(String, String)>,

    engine_state: Option<EngineState>,

    current_tab: Tab,
}

impl DSExampleGUI {
    fn new(
        engine_handle: DSEngineHandle,
        engine_rx: Receiver<DSEngineEvent>,
        cpal_stream: Stream,
        sample_rate: SampleRate,
        to_audio_thread_tx: ringbuf::Producer<UIToAudioThreadMsg>,
    ) -> Self {
        Self {
            engine_handle,
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
                // The DSEngineAudioThread sent in a previous EngineActivated event is now
                // invalidated. Please drop it and wait for a new EngineActivated event to
                // replace it.
                //
                // To keep using the audio graph, you must reactivate the engine with
                // `DSEngineRequest::ActivateEngine`, and then restore the audio graph
                // from an existing save state if you wish using
                // `DSEngineRequest::RestoreFromSaveState`.
                DSEngineEvent::EngineDeactivated(res) => {
                    self.to_audio_thread_tx
                        .push(UIToAudioThreadMsg::DropEngineAudioThread)
                        .unwrap();

                    match res {
                        // The engine was deactivated gracefully after recieving a
                        // `DSEngineRequest::DeactivateEngine` request.
                        EngineDeactivatedInfo::DeactivatedGracefully { .. } => {
                            println!("Engine deactivated gracefully");
                        }
                        // The engine has crashed.
                        EngineDeactivatedInfo::EngineCrashed { error_msg, .. } => {
                            println!("Engine crashed: {}", error_msg);
                        }
                    }

                    self.engine_state = None;
                }

                // This message is sent whenever the engine successfully activates.
                DSEngineEvent::EngineActivated(info) => {
                    self.engine_state = Some(EngineState {
                        graph_in_node_id: info.graph_in_node_id,
                        graph_out_node_id: info.graph_out_node_id,
                        effect_rack_state: EffectRackState::new(),
                    });

                    self.to_audio_thread_tx
                        .push(UIToAudioThreadMsg::NewEngineAudioThread(info.audio_thread))
                        .unwrap();
                }

                // When this message is received, it means that the audio graph is starting
                // the process of restoring from a save state.
                //
                // Reset your UI as if you are loading up a project for the first time, and
                // wait for the `AudioGraphModified` event to repopulate the UI.
                //
                // If the audio graph is in an invalid state as a result of restoring from
                // the save state, then the `EngineDeactivated` event will be sent instead.
                DSEngineEvent::AudioGraphCleared => {
                    if let Some(engine_state) = &mut self.engine_state {
                        engine_state.effect_rack_state.plugins.clear();
                    }
                }

                // This message is sent whenever the audio graph has been modified.
                //
                // Be sure to update your UI from this new state.
                DSEngineEvent::AudioGraphModified(mut res) => {
                    if let Some(engine_state) = &mut self.engine_state {
                        for plugin_id in res.removed_plugins.drain(..) {
                            engine_state.effect_rack_state.remove_plugin(&plugin_id);
                        }

                        for new_plugin_res in res.new_plugins.drain(..) {
                            let mut found = None;
                            for (p, _) in self.plugin_list.iter() {
                                if p.rdn() == new_plugin_res.plugin_id.rdn().as_str() {
                                    found = Some(p.description.name.clone());
                                    break;
                                }
                            }
                            let plugin_name = found.unwrap();

                            let active_state = match new_plugin_res.status {
                                PluginActivationStatus::Activated {
                                    new_handle,
                                    new_param_values,
                                } => Some(EffectRackPluginActiveState::new(
                                    new_handle,
                                    new_param_values,
                                )),
                                PluginActivationStatus::Inactive => None,
                                PluginActivationStatus::LoadError(e) => {
                                    println!("Plugin failed to load: {}", e);
                                    None
                                }
                                PluginActivationStatus::ActivationError(e) => {
                                    println!("Plugin failed to activate: {}", e);
                                    None
                                }
                            };

                            let effect_rack_plugin = EffectRackPluginState {
                                plugin_name,
                                plugin_id: new_plugin_res.plugin_id,
                                active_state,
                            };

                            engine_state.effect_rack_state.plugins.push(effect_rack_plugin);
                        }

                        for (plugin_id, _) in res.updated_plugin_edges.drain(..) {
                            let effect_rack_plugin =
                                engine_state.effect_rack_state.plugin_mut(&plugin_id).unwrap();

                            if effect_rack_plugin.active_state.is_some() {
                                // TODO
                            }
                        }
                    }
                }

                DSEngineEvent::Plugin(event) => match event {
                    // Sent whenever a plugin becomes activated after being deactivated or
                    // when the plugin restarts.
                    //
                    // Make sure your UI updates the port configuration on this plugin.
                    PluginEvent::Activated { plugin_id, new_handle, new_param_values } => {
                        if let Some(engine_state) = &mut self.engine_state {
                            let effect_rack_plugin =
                                engine_state.effect_rack_state.plugin_mut(&plugin_id).unwrap();

                            effect_rack_plugin.update_handle(new_handle, new_param_values);
                        }
                    }

                    // Sent whenever a plugin becomes deactivated. When a plugin is deactivated
                    // you cannot access any of its methods until it is reactivated.
                    PluginEvent::Deactivated {
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

                            effect_rack_plugin.set_inactive();

                            if let Err(e) = status {
                                println!("Plugin failed to activate: {}", e);
                            }
                        }
                    }

                    PluginEvent::ParamsModified { plugin_id, modified_params } => {
                        if let Some(engine_state) = &mut self.engine_state {
                            let effect_rack_plugin =
                                engine_state.effect_rack_state.plugin_mut(&plugin_id).unwrap();

                            if let Some(active_state) = &mut effect_rack_plugin.active_state {
                                active_state.params_state.parameters_modified(&modified_params);
                            }
                        }
                    }

                    unkown_event => {
                        dbg!(unkown_event);
                    }
                },

                DSEngineEvent::PluginScanner(event) => match event {
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
                        self.plugin_list = info
                            .scanned_plugins
                            .iter()
                            .map(|plugin| {
                                let display_choice =
                                    format!("{} ({})", &plugin.description.name, &plugin.format);

                                (plugin.clone(), display_choice)
                            })
                            .collect();

                        self.failed_plugins_text = info
                            .failed_plugins
                            .drain(..)
                            .map(|(path, error)| (path.to_string_lossy().to_string(), error))
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

impl eframe::App for DSExampleGUI {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_updates();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::EffectRack, "FX Rack");
                ui.selectable_value(&mut self.current_tab, Tab::ScannedPlugins, "Scanned Plugins");

                ui.with_layout(egui::Layout::right_to_left(), |ui| {
                    if self.engine_state.is_some() {
                        ui.label(format!("sample rate: {}", self.sample_rate.as_u16()));
                        ui.colored_label(egui::Color32::GREEN, "active");
                        ui.label("engine status:");

                        if ui.button("deactivate").clicked() {
                            self.engine_handle.send(DSEngineRequest::DeactivateEngine);
                        }
                    } else {
                        ui.colored_label(egui::Color32::RED, "inactive");
                        ui.label("engine status:");

                        if ui.button("activate").clicked() {
                            self.engine_handle.send(DSEngineRequest::ActivateEngine(Box::new(
                                ActivateEngineSettings {
                                    sample_rate: self.sample_rate,
                                    min_frames: MIN_BLOCK_SIZE,
                                    max_frames: MAX_BLOCK_SIZE,
                                    num_audio_in_channels: GRAPH_IN_CHANNELS,
                                    num_audio_out_channels: GRAPH_OUT_CHANNELS,
                                    ..Default::default()
                                },
                            )));
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

    fn on_exit(&mut self, _gl: &eframe::glow::Context) {
        self._cpal_stream = None;
    }
}

#[derive(PartialEq)]
enum Tab {
    EffectRack,
    ScannedPlugins,
}
