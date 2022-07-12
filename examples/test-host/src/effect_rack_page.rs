use dropseed::{
    plugin::PluginSaveState, DSEngineHandle, EdgeReq, EdgeReqPortID, ModifyGraphRequest, ParamID,
    ParamInfoFlags, ParamModifiedInfo, PluginHandle, PluginIDReq, PluginInstanceID, PortType,
};
use eframe::egui;
use fnv::FnvHashMap;

use super::DSExampleGUI;

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

pub struct ParamsState {
    params: FnvHashMap<ParamID, ParamState>,
}

impl ParamsState {
    pub fn new(handle: &PluginHandle, param_values: FnvHashMap<ParamID, f64>) -> Self {
        let mut new_self = Self { params: FnvHashMap::default() };

        new_self.update_handle(handle, param_values);

        new_self
    }

    pub fn update_handle(
        &mut self,
        new_handle: &PluginHandle,
        param_values: FnvHashMap<ParamID, f64>,
    ) {
        self.params.clear();

        for info in new_handle.params.params.values() {
            let _ = self.params.insert(
                info.stable_id,
                ParamState {
                    id: info.stable_id,
                    display_name: info.display_name.clone(),
                    value: *param_values.get(&info.stable_id).unwrap(),
                    min_value: info.min_value,
                    max_value: info.max_value,
                    is_stepped: info.flags.contains(ParamInfoFlags::IS_STEPPED),
                    is_read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
                    is_hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
                    is_gesturing: false,
                },
            );
        }
    }

    pub fn parameters_modified(&mut self, modified_params: &[ParamModifiedInfo]) {
        for m_p in modified_params.iter() {
            let param = self.params.get_mut(&m_p.param_id).unwrap();

            if let Some(new_value) = m_p.new_value {
                param.value = new_value;
            }

            param.is_gesturing = m_p.is_gesturing;
        }
    }
}

pub struct EffectRackPluginActiveState {
    pub handle: PluginHandle,
    pub params_state: ParamsState,
}

impl EffectRackPluginActiveState {
    pub fn new(handle: PluginHandle, param_values: FnvHashMap<ParamID, f64>) -> Self {
        let params_state = ParamsState::new(&handle, param_values);

        Self { handle, params_state }
    }
}

pub struct EffectRackPluginState {
    pub plugin_name: String,
    pub plugin_id: PluginInstanceID,

    pub active_state: Option<EffectRackPluginActiveState>,
}

impl EffectRackPluginState {
    pub fn set_inactive(&mut self) {
        self.active_state = None;
    }

    pub fn update_handle(
        &mut self,
        new_handle: PluginHandle,
        param_values: FnvHashMap<ParamID, f64>,
    ) {
        self.active_state = Some(EffectRackPluginActiveState::new(new_handle, param_values));
    }
}

pub struct EffectRackState {
    pub selected_to_add_plugin_i: Option<usize>,

    pub plugins: Vec<EffectRackPluginState>,
}

impl EffectRackState {
    pub fn new() -> Self {
        Self { selected_to_add_plugin_i: None, plugins: Vec::new() }
    }

    pub fn plugin(&self, id: &PluginInstanceID) -> Option<&EffectRackPluginState> {
        let mut found = None;
        for p in self.plugins.iter() {
            if &p.plugin_id == id {
                found = Some(p);
                break;
            }
        }
        found
    }

    pub fn plugin_mut(&mut self, id: &PluginInstanceID) -> Option<&mut EffectRackPluginState> {
        let mut found = None;
        for p in self.plugins.iter_mut() {
            if &p.plugin_id == id {
                found = Some(p);
                break;
            }
        }
        found
    }

    pub fn remove_plugin(&mut self, id: &PluginInstanceID) {
        let mut found = None;
        for (i, p) in self.plugins.iter().enumerate() {
            if &p.plugin_id == id {
                found = Some(i);
                break;
            }
        }
        if let Some(i) = found {
            let _ = self.plugins.remove(i);
        }
    }
}

pub(crate) fn show(app: &mut DSExampleGUI, ui: &mut egui::Ui) {
    if let Some(engine_state) = &mut app.engine_state {
        ui.horizontal(|ui| {
            let selected_text =
                if let Some(plugin_i) = engine_state.effect_rack_state.selected_to_add_plugin_i {
                    &app.plugin_list[plugin_i].1
                } else {
                    "<select a plugin>"
                };

            egui::ComboBox::from_id_source("plugin_to_add").selected_text(selected_text).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(
                        &mut engine_state.effect_rack_state.selected_to_add_plugin_i,
                        None,
                        "<select a plugin>",
                    );

                    for (plugin_i, plugin) in app.plugin_list.iter().enumerate() {
                        ui.selectable_value(
                            &mut engine_state.effect_rack_state.selected_to_add_plugin_i,
                            Some(plugin_i),
                            &plugin.1,
                        );
                    }
                },
            );

            if ui.button("Add Plugin").clicked() {
                if let Some(plugin_i) = engine_state.effect_rack_state.selected_to_add_plugin_i {
                    let key = app.plugin_list[plugin_i].0.key.clone();

                    let request = ModifyGraphRequest {
                        add_plugin_instances: vec![PluginSaveState::new_with_default_preset(key)],
                        remove_plugin_instances: vec![],
                        connect_new_edges: vec![
                            EdgeReq {
                                edge_type: PortType::Audio,
                                src_plugin_id: PluginIDReq::Existing(
                                    engine_state.graph_in_node_id.clone(),
                                ),
                                dst_plugin_id: PluginIDReq::Added(0),
                                src_port_id: EdgeReqPortID::Main,
                                src_port_channel: 0,
                                dst_port_id: EdgeReqPortID::Main,
                                dst_port_channel: 0,
                                log_error_on_fail: true,
                            },
                            EdgeReq {
                                edge_type: PortType::Audio,
                                src_plugin_id: PluginIDReq::Existing(
                                    engine_state.graph_in_node_id.clone(),
                                ),
                                dst_plugin_id: PluginIDReq::Added(0),
                                src_port_id: EdgeReqPortID::Main,
                                src_port_channel: 1,
                                dst_port_id: EdgeReqPortID::Main,
                                dst_port_channel: 1,
                                log_error_on_fail: true,
                            },
                            EdgeReq {
                                edge_type: PortType::Audio,
                                src_plugin_id: PluginIDReq::Added(0),
                                dst_plugin_id: PluginIDReq::Existing(
                                    engine_state.graph_out_node_id.clone(),
                                ),
                                src_port_id: EdgeReqPortID::Main,
                                src_port_channel: 0,
                                dst_port_id: EdgeReqPortID::Main,
                                dst_port_channel: 0,
                                log_error_on_fail: true,
                            },
                            EdgeReq {
                                edge_type: PortType::Audio,
                                src_plugin_id: PluginIDReq::Added(0),
                                dst_plugin_id: PluginIDReq::Existing(
                                    engine_state.graph_out_node_id.clone(),
                                ),
                                src_port_id: EdgeReqPortID::Main,
                                src_port_channel: 1,
                                dst_port_id: EdgeReqPortID::Main,
                                dst_port_channel: 1,
                                log_error_on_fail: true,
                            },
                        ],
                        disconnect_edges: vec![],
                    };

                    app.engine_handle.send(request.into());
                }
            }
        });

        for (plugin_i, plugin) in engine_state.effect_rack_state.plugins.iter_mut().enumerate() {
            show_effect_rack_plugin(ui, plugin_i, plugin, &mut app.engine_handle);
        }
    } else {
        ui.label("Audio engine is deactivated");
    }
}

pub(crate) fn show_effect_rack_plugin(
    ui: &mut egui::Ui,
    plugin_i: usize,
    plugin: &mut EffectRackPluginState,
    engine_handle: &mut DSEngineHandle,
) {
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
                        let request = ModifyGraphRequest {
                            add_plugin_instances: vec![],
                            remove_plugin_instances: vec![plugin.plugin_id.clone()],
                            connect_new_edges: vec![],
                            disconnect_edges: vec![],
                        };

                        engine_handle.send(request.into());
                    }

                    // TODO: Let the user activate/deactive the plugin in this GUI.

                    if plugin.active_state.is_some() {
                        ui.colored_label(egui::Color32::GREEN, "activated");
                    } else {
                        ui.colored_label(egui::Color32::RED, "deactivated");
                    }

                    ui.label(&plugin.plugin_name);
                    ui.label(&format!("id: {:?}", plugin.plugin_id));

                    ui.separator();

                    if let Some(active_state) = &mut plugin.active_state {
                        // TODO: plugin ports

                        let mut values_to_set: Vec<(ParamID, f64)> = Vec::new();

                        for param in active_state.params_state.params.values_mut() {
                            if param.is_hidden {
                                continue;
                            }

                            if param.is_read_only {
                                ui.horizontal(|ui| {
                                    ui.label(&format!(
                                        "{}: {:.8}",
                                        &param.display_name, param.value
                                    ));
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
                                        values_to_set.push((param.id, value as f64));
                                        param.value = value as f64;
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
                                    values_to_set.push((param.id, param.value))
                                }

                                if param.is_gesturing {
                                    ui.colored_label(egui::Color32::GREEN, "Gesturing");
                                }
                            });
                        }

                        for (param_id, value) in values_to_set.drain(..) {
                            active_state.handle.params.set_value(param_id, value);
                        }
                    }
                },
            );
        });
}
