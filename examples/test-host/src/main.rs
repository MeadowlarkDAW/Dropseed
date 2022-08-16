use cpal::traits::{DeviceTrait, HostTrait};
use cpal::Stream;
use dropseed::engine::{
    ActivateEngineSettings, ActivatedEngineInfo, DSEngineAudioThread, DSEngineMainThread,
    EngineDeactivatedStatus, OnIdleEvent,
};
use dropseed::plugin_api::HostInfo;
use dropseed::plugin_scanner::ScannedPluginInfo;
use eframe::egui;
use fern::colors::ColoredLevelConfig;
use log::LevelFilter;
use meadowlark_core_types::time::SampleRate;

mod effect_rack_page;
mod scanned_plugins_page;

use effect_rack_page::EffectRackState;

const MIN_FRAMES: u32 = 1;
const MAX_FRAMES: u32 = 512;
const GRAPH_IN_CHANNELS: u16 = 2;
const GRAPH_OUT_CHANNELS: u16 = 2;

fn main() {
    // ---  Set up logging stuff  -------------------------------------------------

    // Prefer to use a logging crate that is wait-free for threads printing
    // out to the log.
    let log_colors = ColoredLevelConfig::default();

    #[cfg(debug_assertions)]
    const MAIN_LOG_LEVEL: LevelFilter = LevelFilter::Debug;
    #[cfg(not(debug_assertions))]
    const MAIN_LOG_LEVEL: LevelFilter = LevelFilter::Info;

    fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%H:%M:%S]"),
                record.target(),
                log_colors.color(record.level()),
                message
            ))
        })
        // Add blanket level filter -
        .level(MAIN_LOG_LEVEL)
        // Output to stdout, files, and other Dispatch configurations
        .chain(
            // TODO: stdout is not realtime-safe. Send messages to a logging thread instead.
            std::io::stdout(),
        )
        //.chain(fern::log_file("output.log")?)
        // Apply globally
        .apply()
        .unwrap();

    // ---  Initialize cpal stream  -----------------------------------------------

    let cpal_host = cpal::default_host();
    let device = cpal_host.default_output_device().expect("no output device available");
    let config = device.default_output_config().expect("no default output config available");

    let num_out_channels = usize::from(config.channels());
    let sample_rate: SampleRate = config.sample_rate().0.into();

    let mut audio_thread: Option<DSEngineAudioThread> = None;

    let (mut to_audio_thread_tx, mut from_gui_rx) =
        ringbuf::RingBuffer::<UIToAudioThreadMsg>::new(10).split();

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
                } else {
                    audio_buffer.fill(0.0);
                }
            },
            |e| {
                panic!("{:?}", e);
            },
        )
        .unwrap();

    // ---  Initialize Dropseed Engine  -------------------------------------------

    let (mut ds_engine, internal_plugins_scan_res) = DSEngineMainThread::new(
        HostInfo::new(
            "Dropseed Test Host".into(),                              // host name
            env!("CARGO_PKG_VERSION").into(),                         // host version
            Some("Meadowlark".into()),                                // vendor
            Some("https://github.com/MeadowlarkDAW/dropseed".into()), // url
        ),
        vec![], // list of internal plugins
    );

    log::info!("{:?}", &internal_plugins_scan_res);

    let (activated_state, ds_engine_audio_thread) = activate_engine(&mut ds_engine, sample_rate);

    to_audio_thread_tx
        .push(UIToAudioThreadMsg::NewEngineAudioThread(ds_engine_audio_thread))
        .unwrap();

    // ---  Run the UI  -----------------------------------------------------------

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Dropseed Test Host",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());

            Box::new(DSTestHostGUI::new(
                ds_engine,
                activated_state,
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

pub struct ActivatedState {
    engine_info: ActivatedEngineInfo,

    scanned_plugin_list: Vec<(ScannedPluginInfo, String)>,
    scanned_failed_list: Vec<(String, String)>,

    effect_rack_state: EffectRackState,
}

struct DSTestHostGUI {
    ds_engine: DSEngineMainThread,

    activated_state: Option<ActivatedState>,

    current_tab: Tab,

    to_audio_thread_tx: ringbuf::Producer<UIToAudioThreadMsg>,
    _cpal_stream: Option<Stream>,

    sample_rate: SampleRate,
}

impl DSTestHostGUI {
    fn new(
        ds_engine: DSEngineMainThread,
        activated_state: ActivatedState,
        cpal_stream: Stream,
        sample_rate: SampleRate,
        to_audio_thread_tx: ringbuf::Producer<UIToAudioThreadMsg>,
    ) -> Self {
        Self {
            ds_engine,
            activated_state: Some(activated_state),
            current_tab: Tab::EffectRack,
            to_audio_thread_tx,
            _cpal_stream: Some(cpal_stream),
            sample_rate,
        }
    }

    fn on_idle(&mut self) {
        if self.activated_state.is_none() {
            return;
        }

        // This must be called periodically (i.e. once every frame).
        //
        // This will return a list of events that have occured.
        let mut events = self.ds_engine.on_idle();
        for event in events.drain(..) {
            match event {
                // The plugin's parameters have been modified via the plugin's custom
                // GUI.
                //
                // Only the parameters which have changed will be returned in this
                // field.
                OnIdleEvent::PluginParamsModified { plugin_id, modified_params } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.on_params_modified(&modified_params);
                    }
                }

                // Sent when the plugin closed its own GUI by its own means. UI should
                // be updated accordingly so that the user could open the UI again.
                OnIdleEvent::PluginGuiClosed { plugin_id } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.on_plugin_gui_closed();
                    }
                }

                // Sent whenever a plugin becomes activated after being deactivated or
                // when the plugin restarts.
                //
                // Make sure your UI updates the port configuration on this plugin, as
                // well as any custom handles.
                OnIdleEvent::PluginActivated { plugin_id, status } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.on_activated(status);
                    }
                }

                // Sent whenever a plugin has been deactivated. When a plugin is
                // deactivated, you cannot access any of its methods until it is
                // reactivated.
                OnIdleEvent::PluginDeactivated { plugin_id, status } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.on_deactivated();
                    }

                    match status {
                        Ok(()) => log::info!("Plugin {:?} was deactivated gracefully", &plugin_id),
                        Err(e) => {
                            log::info!("Plugin {:?} failed to reactivate: {}", &plugin_id, e);
                        }
                    }
                }

                // Sent whenever the engine has been deactivated, whether gracefully or
                // because of a crash.
                OnIdleEvent::EngineDeactivated(status) => {
                    self.activated_state = None;

                    self.to_audio_thread_tx
                        .push(UIToAudioThreadMsg::DropEngineAudioThread)
                        .unwrap();

                    match status {
                        EngineDeactivatedStatus::DeactivatedGracefully => {
                            log::info!("Engine was deactivated gracefully");
                        }
                        EngineDeactivatedStatus::EngineCrashed(e) => {
                            log::error!("Engine crashed: {}", e);
                        }
                    }
                }
            }
        }

        // TODO: Only call this every 3 seconds or so.
        self.ds_engine.collect_garbage();
    }
}

impl eframe::App for DSTestHostGUI {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.on_idle();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::EffectRack, "FX Rack");
                ui.selectable_value(&mut self.current_tab, Tab::ScannedPlugins, "Scanned Plugins");

                ui.with_layout(egui::Layout::right_to_left(), |ui| {
                    if self.activated_state.is_some() {
                        ui.label(format!("sample rate: {}", self.sample_rate.as_u16()));
                        ui.colored_label(egui::Color32::GREEN, "active");
                        ui.label("engine status:");

                        if ui.button("deactivate").clicked() {
                            if self.ds_engine.deactivate_engine() {
                                self.activated_state = None;

                                self.to_audio_thread_tx
                                    .push(UIToAudioThreadMsg::DropEngineAudioThread)
                                    .unwrap();

                                log::info!("Deactivated dropseed engine gracefully");
                            }
                        }
                    } else {
                        ui.colored_label(egui::Color32::RED, "inactive");
                        ui.label("engine status:");

                        if ui.button("activate").clicked() {
                            let (activated_state, ds_engine_audio_thread) =
                                activate_engine(&mut self.ds_engine, self.sample_rate);

                            self.to_audio_thread_tx
                                .push(UIToAudioThreadMsg::NewEngineAudioThread(
                                    ds_engine_audio_thread,
                                ))
                                .unwrap();

                            self.activated_state = Some(activated_state);
                        }
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
            Tab::EffectRack => {
                effect_rack_page::show(&mut self.ds_engine, self.activated_state.as_mut(), ui)
            }
            Tab::ScannedPlugins => scanned_plugins_page::show(self, ui),
        });
    }

    fn on_exit(&mut self, _gl: &eframe::glow::Context) {
        self._cpal_stream = None;
    }
}

fn activate_engine(
    ds_engine: &mut DSEngineMainThread,
    sample_rate: SampleRate,
) -> (ActivatedState, DSEngineAudioThread) {
    let (engine_info, ds_engine_audio_thread) = ds_engine
        .activate_engine(ActivateEngineSettings {
            sample_rate,
            min_frames: MIN_FRAMES,
            max_frames: MAX_FRAMES,
            num_audio_in_channels: GRAPH_IN_CHANNELS,
            num_audio_out_channels: GRAPH_OUT_CHANNELS,
            ..Default::default()
        })
        .unwrap();

    let mut activated_state = ActivatedState {
        engine_info,
        scanned_plugin_list: Vec::new(),
        scanned_failed_list: Vec::new(),
        effect_rack_state: EffectRackState::new(),
    };

    scanned_plugins_page::scan_external_plugins(ds_engine, &mut activated_state);

    (activated_state, ds_engine_audio_thread)
}

#[derive(PartialEq)]
enum Tab {
    EffectRack,
    ScannedPlugins,
}
