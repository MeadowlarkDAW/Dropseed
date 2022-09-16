use dropseed::engine::NewPluginRes;
use dropseed::graph::PortType;
use dropseed::plugin_api::ext::audio_ports::PluginAudioPortsExt;
use dropseed::plugin_api::ext::gui::{GuiResizeHints, GuiSize};
use dropseed::plugin_api::ext::note_ports::PluginNotePortsExt;
use dropseed::plugin_api::ext::params::ParamInfoFlags;
use dropseed::plugin_api::{DSPluginSaveState, ParamID, PluginInstanceID};
use dropseed::plugin_host::ParamModifiedInfo;
use dropseed::{
    engine::{
        modify_request::{ConnectEdgeReq, EdgeReqPortID, ModifyGraphRequest, PluginIDReq},
        ActivatedEngineInfo, DSEngineMainThread, PluginActivatedStatus, PluginStatus,
    },
    plugin_api::plugin_scanner::ScannedPluginKey,
};
use egui_glow::egui_winit::egui;
use egui_glow::egui_winit::winit;
use fnv::FnvHashMap;
use glutin::window::WindowId;
use raw_window_handle::HasRawWindowHandle;

use crate::ActivatedState;

pub struct EffectRackParamState {
    id: ParamID,

    display_name: String,
    display_value: String,

    value: f64,
    mod_amount: f64,

    min_value: f64,
    max_value: f64,

    is_stepped: bool,
    is_read_only: bool,
    is_hidden: bool,

    is_gesturing: bool,
}

pub struct EffectRackPluginState {
    plugin_id: PluginInstanceID,
    plugin_name: String,

    supports_floating_gui: bool,
    supports_embedded_gui: bool,
    gui_resizable: bool,
    gui_active: bool,
    gui_visible: bool,
    gui_resize_hints: Option<GuiResizeHints>,

    embedded_window: Option<winit::window::Window>,

    activated: bool,
    bypassed: bool,

    param_states: Vec<EffectRackParamState>,
    param_id_to_index: FnvHashMap<ParamID, usize>,

    audio_ports_ext: Option<PluginAudioPortsExt>,
    note_ports_ext: Option<PluginNotePortsExt>,

    internal_handle: Option<Box<dyn std::any::Any + Send + 'static>>,
}

impl EffectRackPluginState {
    pub fn new(
        new_plugin_res: NewPluginRes,
        plugin_name: String,
        ds_engine: &mut DSEngineMainThread,
    ) -> Self {
        let mut new_self = Self {
            plugin_id: new_plugin_res.plugin_id,
            plugin_name,
            supports_floating_gui: new_plugin_res.supports_floating_gui,
            supports_embedded_gui: new_plugin_res.supports_embedded_gui,
            gui_resizable: false,
            gui_active: false,
            gui_visible: false,
            gui_resize_hints: None,
            embedded_window: None,
            activated: false,
            bypassed: false,
            audio_ports_ext: None,
            note_ports_ext: None,
            param_states: Vec::new(),
            param_id_to_index: FnvHashMap::default(),
            internal_handle: None,
        };

        new_self.on_param_list_updated(ds_engine);

        if let PluginStatus::Activated(status) = new_plugin_res.status {
            new_self.on_activated(status);
        }

        new_self
    }

    pub fn on_param_list_updated(&mut self, ds_engine: &mut DSEngineMainThread) {
        self.param_states.clear();
        self.param_id_to_index.clear();
        if let Some(plugin_host) = ds_engine.plugin_host(&self.plugin_id) {
            for (i, param_id) in plugin_host.param_list().iter().enumerate() {
                let param_state = plugin_host.param_state(*param_id).unwrap();

                let mut display_value = String::new();
                if let Err(e) = plugin_host.param_value_to_text(
                    *param_id,
                    param_state.value,
                    &mut display_value,
                ) {
                    log::error!("{}", e);
                    display_value = "error".into();
                }

                self.param_states.push(EffectRackParamState {
                    id: *param_id,
                    display_name: param_state.info.display_name.clone(),
                    display_value,
                    value: param_state.value,
                    mod_amount: param_state.mod_amount,
                    min_value: param_state.info.min_value,
                    max_value: param_state.info.max_value,
                    is_stepped: param_state.info.flags.contains(ParamInfoFlags::IS_STEPPED),
                    is_read_only: param_state.info.flags.contains(ParamInfoFlags::IS_READONLY),
                    is_hidden: param_state.info.flags.contains(ParamInfoFlags::IS_HIDDEN),
                    is_gesturing: param_state.is_gesturing,
                });
                self.param_id_to_index.insert(*param_id, i);
            }
        }
    }

    pub fn on_activated(&mut self, mut status: PluginActivatedStatus) {
        self.activated = true;
        self.internal_handle = status.internal_handle.take();
    }

    pub fn on_deactivated(&mut self) {
        self.activated = false;
        self.internal_handle = None;
    }

    pub fn on_params_modified(
        &mut self,
        modified_params: &[ParamModifiedInfo],
        ds_engine: &mut DSEngineMainThread,
    ) {
        if let Some(plugin_host) = ds_engine.plugin_host(&self.plugin_id) {
            for m_p in modified_params.iter() {
                if let Some(i) = self.param_id_to_index.get(&m_p.param_id) {
                    let param_state = &mut self.param_states[*i];

                    if let Some(new_value) = m_p.new_value {
                        param_state.value = new_value;

                        param_state.display_value.clear();
                        if let Err(e) = plugin_host.param_value_to_text(
                            param_state.id,
                            new_value,
                            &mut param_state.display_value,
                        ) {
                            log::error!("{}", e);
                            param_state.display_value = "error".into();
                        }
                    }

                    param_state.is_gesturing = m_p.is_gesturing;
                }
            }
        }
    }

    pub fn create_floating_gui(&mut self, ds_engine: &mut DSEngineMainThread) {
        if self.supports_floating_gui && !self.gui_active {
            if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                match plugin_host.create_new_floating_gui(None) {
                    Ok(()) => {
                        self.gui_active = true;
                        self.gui_visible = false;
                        self.show_gui(ds_engine);
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to create floating GUI for plugin {:?}: {}",
                            &self.plugin_id,
                            e
                        );
                    }
                }
            }
        }
    }

    pub fn create_embedded_gui(
        &mut self,
        ds_engine: &mut DSEngineMainThread,
        event_loop: &winit::event_loop::EventLoopWindowTarget<()>,
    ) {
        if self.supports_embedded_gui && !self.gui_active {
            if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                let new_window = match winit::window::WindowBuilder::new()
                    .with_title(&self.plugin_name)
                    .build(event_loop)
                {
                    Ok(w) => w,
                    Err(e) => {
                        log::error!("Failed to create window for embedded plugin GUI: {}", e);
                        return;
                    }
                };

                match plugin_host.create_new_embedded_gui(
                    None,
                    None,
                    new_window.raw_window_handle(),
                ) {
                    Ok(info) => {
                        self.gui_active = true;
                        self.gui_visible = false;

                        new_window.set_resizable(info.resizable);

                        new_window.set_inner_size(winit::dpi::PhysicalSize::new(
                            info.size.width,
                            info.size.height,
                        ));

                        self.embedded_window = Some(new_window);

                        self.show_gui(ds_engine);
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to create embedded GUI for plugin {:?}: {}",
                            &self.plugin_id,
                            e
                        );
                    }
                }
            }
        }
    }

    pub fn resize_gui(
        &mut self,
        size: GuiSize,
        scale_factor: Option<f64>,
        initated_by_plugin: bool,
        ds_engine: &mut DSEngineMainThread,
    ) {
        if self.gui_active {
            if initated_by_plugin {
                if let Some(embedded_window) = &mut self.embedded_window {
                    embedded_window
                        .set_inner_size(winit::dpi::PhysicalSize::new(size.width, size.height));
                }
            } else if self.gui_resizable {
                if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                    if let Some(scale_factor) = scale_factor {
                        plugin_host.set_gui_scale(scale_factor);
                    }

                    if let Some(working_size) = plugin_host.adjust_gui_size(size) {
                        match plugin_host.set_gui_size(working_size) {
                            Ok(()) => {
                                if working_size != size {
                                    self.embedded_window.as_mut().unwrap().set_inner_size(
                                        winit::dpi::PhysicalSize::new(
                                            working_size.width,
                                            working_size.height,
                                        ),
                                    );
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to set size of plugin GUI to {:?} on plugin {:?}: {}",
                                    working_size,
                                    &self.plugin_id,
                                    e
                                );
                            }
                        }
                    } else {
                        log::error!(
                            "Failed to set size of plugin GUI to {:?} on plugin {:?}",
                            size,
                            &self.plugin_id
                        );
                    }
                }
            }
        }
    }

    pub fn on_plugin_gui_closed(
        &mut self,
        was_destroyed: bool,
        ds_engine: &mut DSEngineMainThread,
    ) {
        self.gui_visible = false;
        if was_destroyed {
            self.destroy_gui(ds_engine)
        }
    }

    pub fn show_gui(&mut self, ds_engine: &mut DSEngineMainThread) {
        if self.gui_active && !self.gui_visible {
            if let Some(window) = &mut self.embedded_window {
                window.set_visible(true);
            }

            if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                match plugin_host.show_gui() {
                    Ok(()) => self.gui_visible = true,
                    Err(e) => {
                        log::error!("Failed to show GUI for plugin {:?}: {}", &self.plugin_id, e);
                    }
                }
            }
        }
    }

    pub fn hide_gui(&mut self, ds_engine: &mut DSEngineMainThread) {
        if self.gui_active && self.gui_visible {
            if let Some(window) = &mut self.embedded_window {
                window.set_visible(false);
            }

            if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                match plugin_host.hide_gui() {
                    Ok(()) => self.gui_visible = false,
                    Err(e) => {
                        log::error!("Failed to hide GUI for plugin {:?}: {}", &self.plugin_id, e);
                    }
                }
            }
        }
    }

    pub fn destroy_gui(&mut self, ds_engine: &mut DSEngineMainThread) {
        if self.gui_active {
            if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                plugin_host.destroy_gui();
            }
        }

        self.embedded_window = None;
        self.gui_active = false;
        self.gui_visible = false;
    }

    pub fn resize_hints_changed(&mut self, resize_hints: Option<GuiResizeHints>) {
        self.gui_resize_hints = resize_hints;
    }

    pub fn set_bypassed(&mut self, bypassed: bool, ds_engine: &mut DSEngineMainThread) {
        if self.bypassed != bypassed {
            self.bypassed = bypassed;
            ds_engine.plugin_host_mut(&self.plugin_id).unwrap().set_bypassed(bypassed);
        }
    }
}

pub struct EffectRackState {
    selected_plugin_to_add_i: Option<usize>,

    plugins: Vec<EffectRackPluginState>,
}

impl EffectRackState {
    pub fn new() -> Self {
        Self { selected_plugin_to_add_i: None, plugins: Vec::new() }
    }

    pub fn plugin_mut(
        &mut self,
        plugin_id: &PluginInstanceID,
    ) -> Option<&mut EffectRackPluginState> {
        let mut found = None;
        for p in self.plugins.iter_mut() {
            if &p.plugin_id == plugin_id {
                found = Some(p);
                break;
            }
        }
        found
    }

    pub fn plugin_mut_from_embedded_window_id(
        &mut self,
        window_id: WindowId,
    ) -> Option<&mut EffectRackPluginState> {
        let mut found = None;
        for p in self.plugins.iter_mut() {
            if let Some(window) = &p.embedded_window {
                if window.id() == window_id {
                    found = Some(p);
                    break;
                }
            }
        }
        found
    }

    pub fn add_plugin(
        &mut self,
        plugin_key: ScannedPluginKey,
        plugin_name: String,
        engine_info: &ActivatedEngineInfo,
        ds_engine: &mut DSEngineMainThread,
    ) {
        let request = ModifyGraphRequest {
            add_plugin_instances: vec![DSPluginSaveState::new_with_default_state(plugin_key)],
            remove_plugin_instances: vec![],
            connect_new_edges: vec![
                ConnectEdgeReq {
                    edge_type: PortType::Audio,
                    src_plugin_id: PluginIDReq::Existing(engine_info.graph_in_id.clone()),
                    dst_plugin_id: PluginIDReq::Added(0),
                    src_port_id: EdgeReqPortID::Main,
                    src_port_channel: 0,
                    dst_port_id: EdgeReqPortID::Main,
                    dst_port_channel: 0,
                    log_error_on_fail: true,
                    check_for_cycles: true,
                },
                ConnectEdgeReq {
                    edge_type: PortType::Audio,
                    src_plugin_id: PluginIDReq::Existing(engine_info.graph_in_id.clone()),
                    dst_plugin_id: PluginIDReq::Added(0),
                    src_port_id: EdgeReqPortID::Main,
                    src_port_channel: 1,
                    dst_port_id: EdgeReqPortID::Main,
                    dst_port_channel: 1,
                    log_error_on_fail: true,
                    check_for_cycles: true,
                },
                ConnectEdgeReq {
                    edge_type: PortType::Audio,
                    src_plugin_id: PluginIDReq::Added(0),
                    dst_plugin_id: PluginIDReq::Existing(engine_info.graph_out_id.clone()),
                    src_port_id: EdgeReqPortID::Main,
                    src_port_channel: 0,
                    dst_port_id: EdgeReqPortID::Main,
                    dst_port_channel: 0,
                    log_error_on_fail: true,
                    check_for_cycles: true,
                },
                ConnectEdgeReq {
                    edge_type: PortType::Audio,
                    src_plugin_id: PluginIDReq::Added(0),
                    dst_plugin_id: PluginIDReq::Existing(engine_info.graph_out_id.clone()),
                    src_port_id: EdgeReqPortID::Main,
                    src_port_channel: 1,
                    dst_port_id: EdgeReqPortID::Main,
                    dst_port_channel: 1,
                    log_error_on_fail: true,
                    check_for_cycles: true,
                },
            ],
            disconnect_edges: vec![],
        };

        let mut result = ds_engine.modify_graph(request).unwrap();
        let new_plugin_res = result.new_plugins.remove(0);

        let new_plugin_state = EffectRackPluginState::new(new_plugin_res, plugin_name, ds_engine);

        self.plugins.push(new_plugin_state);
    }

    pub fn remove_plugin(
        &mut self,
        plugin_id: &PluginInstanceID,
        ds_engine: &mut DSEngineMainThread,
    ) {
        let mut found = None;
        for (i, p) in self.plugins.iter().enumerate() {
            if &p.plugin_id == plugin_id {
                found = Some(i);
                break;
            }
        }
        if let Some(i) = found {
            let result = ds_engine.modify_graph(ModifyGraphRequest {
                add_plugin_instances: vec![],
                remove_plugin_instances: vec![plugin_id.clone()],
                connect_new_edges: vec![],
                disconnect_edges: vec![],
            });

            log::debug!("{:?}", &result);

            let _ = self.plugins.remove(i);
        }
    }
}

pub(crate) fn show(
    ds_engine: &mut DSEngineMainThread,
    activated_state: Option<&mut ActivatedState>,
    ui: &mut egui::Ui,
    event_loop: &winit::event_loop::EventLoopWindowTarget<()>,
) {
    if let Some(activated_state) = activated_state {
        let ActivatedState { effect_rack_state, scanned_plugin_list, engine_info, .. } =
            activated_state;

        ui.horizontal(|ui| {
            let selected_text = if let Some(plugin_i) = effect_rack_state.selected_plugin_to_add_i {
                &scanned_plugin_list[plugin_i].1
            } else {
                "<select a plugin>"
            };

            egui::ComboBox::from_id_source("plugin_to_add").selected_text(selected_text).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(
                        &mut effect_rack_state.selected_plugin_to_add_i,
                        None,
                        "<select a plugin>",
                    );

                    for (plugin_i, plugin) in scanned_plugin_list.iter().enumerate() {
                        ui.selectable_value(
                            &mut effect_rack_state.selected_plugin_to_add_i,
                            Some(plugin_i),
                            &plugin.1,
                        );
                    }
                },
            );

            if ui.button("Add Plugin").clicked() {
                if let Some(plugin_i) = effect_rack_state.selected_plugin_to_add_i {
                    let plugin_key = scanned_plugin_list[plugin_i].0.key.clone();
                    let plugin_name = scanned_plugin_list[plugin_i].0.description.name.clone();

                    effect_rack_state.add_plugin(plugin_key, plugin_name, engine_info, ds_engine);
                }
            }
        });

        let mut plugins_to_remove: Vec<PluginInstanceID> = Vec::new();
        for (plugin_i, plugin) in effect_rack_state.plugins.iter_mut().enumerate() {
            if show_effect_rack_plugin(ui, plugin_i, plugin, ds_engine, event_loop) {
                plugins_to_remove.push(plugin.plugin_id.clone());
            }
        }

        for plugin_id in plugins_to_remove.iter() {
            effect_rack_state.remove_plugin(plugin_id, ds_engine)
        }
    } else {
        ui.label("Audio engine is deactivated");
    }
}

fn show_effect_rack_plugin(
    ui: &mut egui::Ui,
    plugin_i: usize,
    plugin: &mut EffectRackPluginState,
    ds_engine: &mut DSEngineMainThread,
    event_loop: &winit::event_loop::EventLoopWindowTarget<()>,
) -> bool {
    let mut remove = false;

    egui::Frame::default()
        .inner_margin(egui::style::Margin::same(10.0))
        .outer_margin(egui::style::Margin::same(5.0))
        .fill(egui::Color32::from_gray(15))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(100)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().id_source(&format!("plugin{}hscroll", plugin_i)).show(
                ui,
                |ui| {
                    if ui.button("remove").clicked() {
                        remove = true;
                    }

                    ui.label(&plugin.plugin_name);
                    ui.label(&format!("id: {:?}", plugin.plugin_id));

                    ui.separator();

                    if plugin.bypassed {
                        if ui.button("unbypass").clicked() {
                            plugin.set_bypassed(false, ds_engine);
                        }
                    } else {
                        if ui.button("bypass").clicked() {
                            plugin.set_bypassed(true, ds_engine);
                        }
                    }

                    ui.separator();

                    if plugin.supports_floating_gui || plugin.supports_embedded_gui {
                        if plugin.gui_active {
                            if plugin.gui_visible {
                                if ui.button("hide gui").clicked() {
                                    plugin.hide_gui(ds_engine);
                                }
                            } else {
                                if ui.button("show gui").clicked() {
                                    plugin.show_gui(ds_engine);
                                }
                            }

                            if ui.button("destroy gui").clicked() {
                                plugin.destroy_gui(ds_engine);
                            }
                        } else {
                            if plugin.supports_floating_gui {
                                if ui.button("create floating gui").clicked() {
                                    plugin.create_floating_gui(ds_engine);
                                }
                            }
                            if plugin.supports_embedded_gui {
                                if ui.button("create embedded gui").clicked() {
                                    plugin.create_embedded_gui(ds_engine, event_loop);
                                }
                            }
                        }
                    }

                    ui.separator();

                    // TODO: Let the user activate/deactive the plugin in this GUI.

                    if plugin.activated {
                        ui.colored_label(egui::Color32::GREEN, "activated");
                    } else {
                        ui.colored_label(egui::Color32::RED, "deactivated");
                        return;
                    }

                    ui.separator();

                    // TODO: plugin ports

                    for param_state in plugin.param_states.iter_mut() {
                        if param_state.is_hidden {
                            continue;
                        }

                        if param_state.is_read_only {
                            ui.horizontal(|ui| {
                                ui.label(&format!(
                                    "{}: {}",
                                    &param_state.display_name, param_state.display_value
                                ));
                            });

                            continue;
                        }

                        ui.horizontal(|ui| {
                            ui.label(&param_state.display_name);

                            let mut update_value_to: Option<f64> = None;
                            if param_state.is_stepped {
                                let mut value: i64 = param_state.value.round() as i64;
                                let min_value: i64 = param_state.min_value.round() as i64;
                                let max_value: i64 = param_state.max_value.round() as i64;

                                if ui
                                    .add(
                                        egui::Slider::new(&mut value, min_value..=max_value)
                                            .show_value(false),
                                    )
                                    .changed()
                                {
                                    update_value_to = Some(value as f64);
                                }
                            } else if ui
                                .add(
                                    egui::Slider::new(
                                        &mut param_state.value,
                                        param_state.min_value..=param_state.max_value,
                                    )
                                    .show_value(false),
                                )
                                .changed()
                            {
                                update_value_to = Some(param_state.value);
                            }

                            if let Some(new_value) = update_value_to {
                                match ds_engine
                                    .plugin_host_mut(&plugin.plugin_id)
                                    .as_mut()
                                    .unwrap()
                                    .set_param_value(param_state.id, new_value as f64)
                                {
                                    Ok(v) => {
                                        param_state.value = v;

                                        let plugin_host =
                                            ds_engine.plugin_host(&plugin.plugin_id).unwrap();
                                        param_state.display_value.clear();
                                        if let Err(e) = plugin_host.param_value_to_text(
                                            param_state.id,
                                            v,
                                            &mut param_state.display_value,
                                        ) {
                                            log::error!("{}", e);
                                            param_state.display_value = "error".into();
                                        }
                                    }
                                    Err(e) => log::error!("{}", e),
                                }
                            }

                            ui.label(&param_state.display_value);

                            if param_state.is_gesturing {
                                ui.colored_label(egui::Color32::GREEN, "Gesturing");
                            }
                        });
                    }
                },
            );
        });

    remove
}
