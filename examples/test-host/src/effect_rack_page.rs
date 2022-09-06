use dropseed::graph::PortType;
use dropseed::plugin_api::ext::audio_ports::PluginAudioPortsExt;
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
use eframe::egui;
use fnv::FnvHashMap;

use crate::ActivatedState;

pub struct ParamState {
    id: ParamID,

    display_name: String,

    value: f64,

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

    has_gui: bool,
    is_gui_open: bool,
    activated: bool,

    params: FnvHashMap<ParamID, ParamState>,

    audio_ports_ext: Option<PluginAudioPortsExt>,
    note_ports_ext: Option<PluginNotePortsExt>,

    internal_handle: Option<Box<dyn std::any::Any + Send + 'static>>,
}

impl EffectRackPluginState {
    pub fn new(
        plugin_id: PluginInstanceID,
        plugin_name: String,
        plugin_status: PluginStatus,
    ) -> Self {
        let mut new_self = Self {
            plugin_id,
            plugin_name,
            has_gui: false,
            is_gui_open: false,
            activated: false,
            audio_ports_ext: None,
            note_ports_ext: None,
            params: FnvHashMap::default(),
            internal_handle: None,
        };

        if let PluginStatus::Activated(status) = plugin_status {
            new_self.on_activated(status);
        }

        new_self
    }

    pub fn on_activated(&mut self, mut status: PluginActivatedStatus) {
        self.activated = true;

        self.audio_ports_ext = status.new_audio_ports_ext.take();
        self.note_ports_ext = status.new_note_ports_ext.take();

        self.params.clear();

        for (info, value) in status.new_parameters.drain(..) {
            let _ = self.params.insert(
                info.stable_id,
                ParamState {
                    id: info.stable_id,
                    display_name: info.display_name.clone(),
                    value,
                    min_value: info.min_value,
                    max_value: info.max_value,
                    is_stepped: info.flags.contains(ParamInfoFlags::IS_STEPPED),
                    is_read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
                    is_hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
                    is_gesturing: false,
                },
            );
        }

        self.internal_handle = status.internal_handle.take();

        self.has_gui = status.has_gui;
    }

    pub fn on_deactivated(&mut self) {
        self.activated = false;
        self.has_gui = false;
        self.params.clear();
    }

    pub fn on_params_modified(&mut self, modified_params: &[ParamModifiedInfo]) {
        for m_p in modified_params.iter() {
            let param = self.params.get_mut(&m_p.param_id).unwrap();

            if let Some(new_value) = m_p.new_value {
                param.value = new_value;
            }

            param.is_gesturing = m_p.is_gesturing;
        }
    }

    pub fn on_plugin_gui_closed(&mut self) {
        self.is_gui_open = false;
    }

    pub fn show_gui(&mut self, ds_engine: &mut DSEngineMainThread) {
        if self.has_gui {
            if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                match plugin_host.show_gui() {
                    Ok(()) => self.is_gui_open = true,
                    Err(e) => {
                        log::error!("Failed to open GUI for plugin {:?}: {}", &self.plugin_id, e);
                        self.is_gui_open = false;
                    }
                }
            }
        }
    }

    pub fn close_gui(&mut self, ds_engine: &mut DSEngineMainThread) {
        if self.is_gui_open {
            if let Some(plugin_host) = ds_engine.plugin_host_mut(&self.plugin_id) {
                plugin_host.close_gui();
            }

            self.on_plugin_gui_closed();
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

    /*
    pub fn plugin(&self, plugin_id: &PluginInstanceID) -> Option<&EffectRackPluginState> {
        let mut found = None;
        for p in self.plugins.iter() {
            if &p.plugin_id == plugin_id {
                found = Some(p);
                break;
            }
        }
        found
    }
    */

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

        let new_plugin_state = EffectRackPluginState::new(
            new_plugin_res.plugin_id,
            plugin_name,
            new_plugin_res.status,
        );

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
            if show_effect_rack_plugin(ui, plugin_i, plugin, ds_engine) {
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
                    if ui.small_button("x").clicked() {
                        remove = true;
                    }

                    if plugin.has_gui {
                        if plugin.is_gui_open {
                            if ui.small_button("close ui").clicked() {
                                plugin.show_gui(ds_engine);
                            }
                        } else if ui.small_button("ui").clicked() {
                            plugin.close_gui(ds_engine);
                        }
                    }

                    // TODO: Let the user activate/deactive the plugin in this GUI.

                    if plugin.activated {
                        ui.colored_label(egui::Color32::GREEN, "activated");
                    } else {
                        ui.colored_label(egui::Color32::RED, "deactivated");
                        return;
                    }

                    ui.label(&plugin.plugin_name);
                    ui.label(&format!("id: {:?}", plugin.plugin_id));

                    ui.separator();

                    // TODO: plugin ports

                    for param in plugin.params.values_mut() {
                        if param.is_hidden {
                            continue;
                        }

                        if param.is_read_only {
                            ui.horizontal(|ui| {
                                ui.label(&format!("{}: {:.8}", &param.display_name, param.value));
                            });

                            continue;
                        }

                        ui.horizontal(|ui| {
                            if param.is_stepped {
                                let mut value: i64 = param.value.round() as i64;
                                let min_value: i64 = param.min_value.round() as i64;
                                let max_value: i64 = param.max_value.round() as i64;

                                if ui
                                    .add(
                                        egui::Slider::new(&mut value, min_value..=max_value)
                                            .text(&param.display_name),
                                    )
                                    .changed()
                                {
                                    match ds_engine
                                        .plugin_host_mut(&plugin.plugin_id)
                                        .as_mut()
                                        .unwrap()
                                        .set_param_value(param.id, value as f64)
                                    {
                                        Ok(v) => param.value = v,
                                        Err(e) => log::error!("{}", e),
                                    }
                                }
                            } else if ui
                                .add(
                                    egui::Slider::new(
                                        &mut param.value,
                                        param.min_value..=param.max_value,
                                    )
                                    .text(&param.display_name),
                                )
                                .changed()
                            {
                                match ds_engine
                                    .plugin_host_mut(&plugin.plugin_id)
                                    .as_mut()
                                    .unwrap()
                                    .set_param_value(param.id, param.value)
                                {
                                    Ok(v) => param.value = v,
                                    Err(e) => log::error!("{}", e),
                                }
                            }

                            if param.is_gesturing {
                                ui.colored_label(egui::Color32::GREEN, "Gesturing");
                            }
                        });
                    }
                },
            );
        });

    remove
}
