#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::Stream;
use dropseed::engine::{
    ActivateEngineSettings, ActivatedEngineInfo, DSEngineAudioThread, DSEngineMainThread,
    DefaultTempoMap, EngineDeactivatedStatus, EngineSettings, OnIdleEvent,
};
use dropseed::plugin_api::ext::gui::GuiSize;
use dropseed::plugin_api::transport::LoopState;
use dropseed::plugin_api::HostInfo;
use dropseed::plugin_scanner::ScannedPluginInfo;
use egui_glow::egui_winit::egui;
use egui_glow::egui_winit::winit;
use fern::colors::ColoredLevelConfig;
use glutin::dpi::PhysicalSize;
use glutin::window::WindowId;
use log::LevelFilter;
use std::sync::Arc;
use std::time::Instant;

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
    let sample_rate = config.sample_rate().0;

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

    let (mut ds_engine, first_timer_instant, internal_plugins_scan_res) = DSEngineMainThread::new(
        HostInfo::new(
            "Dropseed Test Host".into(),                              // host name
            env!("CARGO_PKG_VERSION").into(),                         // host version
            Some("Meadowlark".into()),                                // vendor
            Some("https://github.com/MeadowlarkDAW/dropseed".into()), // url
        ),
        EngineSettings::default(),
        vec![], // list of internal plugins
    );

    log::info!("{:?}", &internal_plugins_scan_res);

    let (activated_state, ds_engine_audio_thread) = activate_engine(&mut ds_engine, sample_rate);

    to_audio_thread_tx
        .push(UIToAudioThreadMsg::NewEngineAudioThread(ds_engine_audio_thread))
        .unwrap();

    // --- Initialize UI Stuff  ---------------------------------------------------

    let event_loop = glutin::event_loop::EventLoopBuilder::with_user_event().build();
    let (gl_window, gl) = create_display(&event_loop);
    let gl = Arc::new(gl);

    let egui_glow = egui_glow::EguiGlow::new(&event_loop, Arc::clone(&gl));

    setup_fonts(&egui_glow.egui_ctx);

    let app = DSTestHostGUI::new(
        ds_engine,
        activated_state,
        cpal_stream,
        sample_rate,
        to_audio_thread_tx,
    );

    run_ui_event_loop(event_loop, gl_window, gl, egui_glow, app, first_timer_instant);
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

    sample_rate: u32,
}

impl DSTestHostGUI {
    fn new(
        ds_engine: DSEngineMainThread,
        activated_state: ActivatedState,
        cpal_stream: Stream,
        sample_rate: u32,
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

    fn update(
        &mut self,
        ctx: &egui::Context,
        event_loop: &winit::event_loop::EventLoopWindowTarget<()>,
    ) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::EffectRack, "FX Rack");
                ui.selectable_value(&mut self.current_tab, Tab::ScannedPlugins, "Scanned Plugins");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if self.activated_state.is_some() {
                        ui.label(format!("sample rate: {}", self.sample_rate));
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
            Tab::EffectRack => effect_rack_page::show(
                &mut self.ds_engine,
                self.activated_state.as_mut(),
                ui,
                event_loop,
            ),
            Tab::ScannedPlugins => scanned_plugins_page::show(self, ui),
        });
    }

    fn on_timer(&mut self) -> Instant {
        // This must be called periodically.
        //
        // This will return a list of events that have occured, as well as the next
        // instant that this method should be called again.
        let (mut events, next_timer_instant) = self.ds_engine.on_timer();
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
                        plugin.on_params_modified(&modified_params, &mut self.ds_engine);
                    }
                }

                // The plugin requested the app to resize its gui to the given size.
                //
                // This event will only be sent if using an embedded window for the
                // plugin GUI.
                OnIdleEvent::PluginRequestedToResizeGui { plugin_id, size } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.resize_gui(size, None, true, &mut self.ds_engine);
                    }
                }

                // The plugin requested the app to show its GUI.
                //
                // This event will only be sent if using an embedded window for the
                // plugin GUI.
                OnIdleEvent::PluginRequestedToShowGui { plugin_id } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.show_gui(&mut self.ds_engine);
                    }
                }

                // The plugin requested the app to hide its GUI.
                //
                // Note that hiding the GUI is not the same as destroying the GUI.
                // Hiding only hides the window content, it does not free the GUI's
                // resources.  Yet it may be a good idea to stop painting timers
                // when a plugin GUI is hidden.
                //
                // This event will only be sent if using an embedded window for the
                // plugin GUI.
                OnIdleEvent::PluginRequestedToHideGui { plugin_id } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.hide_gui(&mut self.ds_engine);
                    }
                }

                // Sent when the plugin closed its own GUI by its own means. UI should
                // be updated accordingly so that the user could open the UI again.
                //
                // If `was_destroyed` is `true`, then the app must call
                // `PluginHostMainThread::destroy_gui()` to acknowledge the gui
                // destruction.
                OnIdleEvent::PluginGuiClosed { plugin_id, was_destroyed } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.on_plugin_gui_closed(was_destroyed, &mut self.ds_engine);
                    }
                }

                // Sent when the plugin changed the resize hint information on how
                // to resize its GUI.
                //
                // This event will only be sent if using an embedded window for the
                // plugin GUI.
                OnIdleEvent::PluginChangedGuiResizeHints { plugin_id, resize_hints } => {
                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.resize_hints_changed(resize_hints);
                    }
                }

                // The plugin has updated its list of parameters.
                OnIdleEvent::PluginUpdatedParameterList { plugin_id, status } => {
                    if let Err(e) = status {
                        log::error!("{}", e);
                    }

                    if let Some(plugin) = self
                        .activated_state
                        .as_mut()
                        .unwrap()
                        .effect_rack_state
                        .plugin_mut(&plugin_id)
                    {
                        plugin.on_param_list_updated(&mut self.ds_engine);
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

        next_timer_instant
    }

    fn on_plugin_window_resized(
        &mut self,
        window_id: WindowId,
        new_size: &PhysicalSize<u32>,
        new_scale_factor: Option<f64>,
    ) {
        if self.activated_state.is_none() {
            return;
        }

        if let Some(plugin) = self
            .activated_state
            .as_mut()
            .unwrap()
            .effect_rack_state
            .plugin_mut_from_embedded_window_id(window_id)
        {
            let new_size = GuiSize { width: new_size.width, height: new_size.height };
            plugin.resize_gui(new_size, new_scale_factor, false, &mut self.ds_engine);
        }
    }

    fn on_plugin_window_closed(&mut self, window_id: WindowId) {
        if self.activated_state.is_none() {
            return;
        }

        if let Some(plugin) = self
            .activated_state
            .as_mut()
            .unwrap()
            .effect_rack_state
            .plugin_mut_from_embedded_window_id(window_id)
        {
            plugin.on_plugin_gui_closed(true, &mut self.ds_engine);
        }
    }

    fn on_exit(&mut self) {
        // Make sure that the engine is deactivated or dropped in the main
        // thread before exiting your program.
        if self.ds_engine.is_activated() {
            self.ds_engine.deactivate_engine();
        }

        self._cpal_stream = None;
    }
}

#[derive(PartialEq)]
enum Tab {
    EffectRack,
    ScannedPlugins,
}

fn activate_engine(
    ds_engine: &mut DSEngineMainThread,
    sample_rate: u32,
) -> (ActivatedState, DSEngineAudioThread) {
    let (engine_info, ds_engine_audio_thread) = ds_engine
        .activate_engine(
            0,
            LoopState::Inactive,
            Box::new(DefaultTempoMap::default()),
            ActivateEngineSettings {
                sample_rate,
                min_frames: MIN_FRAMES,
                max_frames: MAX_FRAMES,
                num_audio_in_channels: GRAPH_IN_CHANNELS,
                num_audio_out_channels: GRAPH_OUT_CHANNELS,
                ..Default::default()
            },
        )
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

fn run_ui_event_loop(
    event_loop: glutin::event_loop::EventLoop<()>,
    gl_window: glutin::WindowedContext<glutin::PossiblyCurrent>,
    gl: Arc<egui_glow::glow::Context>,
    mut egui_glow: egui_glow::EguiGlow,
    mut app: DSTestHostGUI,
    first_timer_instant: Instant,
) {
    let main_window_id = gl_window.window().id();

    let mut first_timer_instant = Some(first_timer_instant);
    let mut requested_timer_instant = Instant::now();
    let mut requested_repaint_instant = Instant::now();

    event_loop.run(move |event, event_loop, control_flow| {
        let mut redraw = || {
            let repaint_after = egui_glow.run(gl_window.window(), |egui_ctx| {
                app.update(egui_ctx, event_loop);
            });

            if repaint_after.is_zero() {
                gl_window.window().request_redraw();
                glutin::event_loop::ControlFlow::Poll
            } else if let Some(repaint_after_instant) =
                std::time::Instant::now().checked_add(repaint_after)
            {
                requested_repaint_instant = repaint_after_instant;
                glutin::event_loop::ControlFlow::WaitUntil(repaint_after_instant)
            } else {
                glutin::event_loop::ControlFlow::Wait
            };

            {
                unsafe {
                    use egui_glow::glow::HasContext as _;
                    gl.clear_color(0.0, 0.0, 0.0, 1.0);
                    gl.clear(egui_glow::glow::COLOR_BUFFER_BIT);
                }

                // draw things behind egui here

                egui_glow.paint(gl_window.window());

                // draw things on top of egui here

                gl_window.swap_buffers().unwrap();
            }
        };

        if let Some(first_timer_instant) = first_timer_instant.take() {
            requested_timer_instant = first_timer_instant;
            control_flow.set_wait_until(first_timer_instant);
        }

        match event {
            // Platform-dependent event handlers to workaround a winit bug
            // See: https://github.com/rust-windowing/winit/issues/987
            // See: https://github.com/rust-windowing/winit/issues/1619
            //glutin::event::Event::RedrawEventsCleared if cfg!(windows) => redraw(),
            glutin::event::Event::RedrawRequested(window_id) => {
                if window_id == main_window_id {
                    redraw()
                }
            }

            glutin::event::Event::WindowEvent { window_id, event } => {
                use glutin::event::WindowEvent;

                if window_id == main_window_id {
                    if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
                        *control_flow = glutin::event_loop::ControlFlow::Exit;
                    }

                    if let glutin::event::WindowEvent::Resized(physical_size) = &event {
                        gl_window.resize(*physical_size);
                    } else if let glutin::event::WindowEvent::ScaleFactorChanged {
                        new_inner_size,
                        ..
                    } = &event
                    {
                        gl_window.resize(**new_inner_size);
                    }

                    egui_glow.on_event(&event);

                    gl_window.window().request_redraw();
                } else {
                    if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
                        app.on_plugin_window_closed(window_id);
                    }

                    // TODO: Detect when window is minimized/un-minimized and tell the plugin
                    // to hide/show its GUI accordingly once winit gains that ability.
                    // https://github.com/rust-windowing/winit/issues/2334

                    if let glutin::event::WindowEvent::Resized(physical_size) = &event {
                        app.on_plugin_window_resized(window_id, physical_size, None);
                    } else if let glutin::event::WindowEvent::ScaleFactorChanged {
                        new_inner_size,
                        scale_factor,
                    } = &event
                    {
                        app.on_plugin_window_resized(
                            window_id,
                            new_inner_size,
                            Some(*scale_factor),
                        );
                    }
                }
            }
            glutin::event::Event::LoopDestroyed => {
                app.on_exit();
                egui_glow.destroy();
            }
            glutin::event::Event::NewEvents(glutin::event::StartCause::ResumeTimeReached {
                requested_resume,
                ..
            }) => {
                if requested_resume == requested_timer_instant {
                    let next_timer_instant = app.on_timer();
                    requested_timer_instant = next_timer_instant;
                    control_flow.set_wait_until(next_timer_instant);
                }

                gl_window.window().request_redraw();
            }

            _ => (),
        }
    });
}

fn create_display(
    event_loop: &glutin::event_loop::EventLoop<()>,
) -> (glutin::WindowedContext<glutin::PossiblyCurrent>, egui_glow::glow::Context) {
    let window_builder = glutin::window::WindowBuilder::new()
        .with_resizable(true)
        .with_inner_size(glutin::dpi::LogicalSize { width: 800.0, height: 600.0 })
        .with_title("Dropseed Test Host");

    let gl_window = unsafe {
        glutin::ContextBuilder::new()
            .with_depth_buffer(0)
            .with_srgb(true)
            .with_stencil_buffer(0)
            .with_vsync(true)
            .build_windowed(window_builder, event_loop)
            .unwrap()
            .make_current()
            .unwrap()
    };

    let gl = unsafe {
        egui_glow::glow::Context::from_loader_function(|s| gl_window.get_proc_address(s))
    };

    (gl_window, gl)
}

fn setup_fonts(ctx: &egui::Context) {
    use egui::{FontDefinitions, FontFamily, FontId, FontTweak, TextStyle};

    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "barlow".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/Barlow-Regular.otf"))
            .tweak(FontTweak { scale: 1.01, y_offset_factor: 0.0, y_offset: -4.0 }),
    );

    fonts.font_data.insert(
        "fira_code".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/FiraCode-Regular.otf")),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "barlow".to_owned());

    fonts.families.entry(egui::FontFamily::Monospace).or_default().push("fira_code".to_owned());

    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(28.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(17.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(16.0, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(17.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
    ]
    .into();
    ctx.set_style(style);
}
