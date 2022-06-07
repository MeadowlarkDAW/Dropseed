use eframe::egui;
use fnv::FnvHashMap;
use rusty_daw_engine::{
    plugin::ext::audio_ports::PluginAudioPortsExt, Edge, ParamID, ParamInfoFlags,
    ParamModifiedInfo, PluginEdges, PluginInstanceID, PluginParamsExt, PortType,
};

use super::BasicDawExampleGUI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortChannel {
    AudioIn(usize),
    AudioOut(usize),
}

impl Default for PortChannel {
    fn default() -> Self {
        PortChannel::AudioIn(0)
    }
}

pub struct AudioPortState {
    audio_ports_state_ext: PluginAudioPortsExt,

    audio_in_edges: Vec<Vec<Edge>>,
    audio_out_edges: Vec<Vec<Edge>>,
}

impl AudioPortState {
    pub fn new(audio_ports_state_ext: PluginAudioPortsExt) -> Self {
        let audio_in_edges: Vec<Vec<Edge>> =
            (0..audio_ports_state_ext.total_in_channels()).map(|_| Vec::new()).collect();
        let audio_out_edges: Vec<Vec<Edge>> =
            (0..audio_ports_state_ext.total_out_channels()).map(|_| Vec::new()).collect();

        Self { audio_ports_state_ext, audio_in_edges, audio_out_edges }
    }

    pub fn sync_with_new_edges(&mut self, edges: &PluginEdges) {
        for edges in self.audio_in_edges.iter_mut() {
            edges.clear();
        }
        for edges in self.audio_out_edges.iter_mut() {
            edges.clear();
        }

        for edge in edges.incoming.iter() {
            match edge.edge_type {
                PortType::Audio => {
                    self.audio_in_edges[usize::from(edge.dst_channel)].push(edge.clone());
                }
                PortType::Event => {
                    // TODO
                }
            }
        }
        for edge in edges.outgoing.iter() {
            match edge.edge_type {
                PortType::Audio => {
                    self.audio_out_edges[usize::from(edge.src_channel)].push(edge.clone());
                }
                PortType::Event => {
                    // TODO
                }
            }
        }
    }
}

pub struct ParamState {
    id: ParamID,

    display_name: String,

    value: f64,

    min_value: f64,
    max_value: f64,
    default_value: f64,

    is_stepped: bool,
    is_read_only: bool,
    is_hidden: bool,

    is_gesturing: bool,
}

pub struct ParamsState {
    params_ext: PluginParamsExt,

    params: FnvHashMap<ParamID, ParamState>,
}

impl ParamsState {
    pub fn new(params_ext: PluginParamsExt, param_values: FnvHashMap<ParamID, f64>) -> Self {
        let mut params: FnvHashMap<ParamID, ParamState> = FnvHashMap::default();

        for info in params_ext.params.values() {
            let _ = params.insert(
                info.stable_id,
                ParamState {
                    id: info.stable_id,
                    display_name: info.display_name.clone(),
                    value: *param_values.get(&info.stable_id).unwrap(),
                    min_value: info.min_value,
                    max_value: info.max_value,
                    default_value: info.default_value,
                    is_stepped: info.flags.contains(ParamInfoFlags::IS_STEPPED),
                    is_read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
                    is_hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
                    is_gesturing: false,
                },
            );
        }

        Self { params_ext, params }
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

pub struct EffectRackPluginState {
    pub plugin_name: String,
    pub plugin_id: PluginInstanceID,
    pub audio_ports_state: Option<AudioPortState>,
    pub params_state: Option<ParamsState>,

    pub active: bool,
    pub selected_port: PortChannel,
}

pub struct EffectRackState {
    pub plugins: Vec<EffectRackPluginState>,
}

impl EffectRackState {
    pub fn new() -> Self {
        Self { plugins: Vec::new() }
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

pub(crate) fn show(app: &mut BasicDawExampleGUI, ui: &mut egui::Ui) {
    if let Some(engine_state) = &mut app.engine_state {
        // TODO: Let the user add/remove plugins in this GUI.

        for (plugin_i, plugin) in engine_state.effect_rack_state.plugins.iter_mut().enumerate() {
            egui::Frame::default()
                .inner_margin(egui::style::Margin::same(10.0))
                .outer_margin(egui::style::Margin::same(5.0))
                .fill(egui::Color32::from_gray(15))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(100)))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_source(&format!("plugin{}hscroll", plugin_i))
                        .show(ui, |ui| {
                            // TODO: Let the user activate/deactive the plugin in this GUI.

                            if plugin.active {
                                ui.colored_label(egui::Color32::GREEN, "activated");
                            } else {
                                ui.colored_label(egui::Color32::RED, "deactivated");
                            }

                            ui.label(&plugin.plugin_name);
                            ui.label(&format!("id: {:?}", plugin.plugin_id));

                            ui.separator();

                            if let Some(audio_ports_state) = &plugin.audio_ports_state {
                                ui.label("audio in");
                                let mut channel_i = 0;
                                for (port_i, port) in audio_ports_state
                                    .audio_ports_state_ext
                                    .inputs
                                    .iter()
                                    .enumerate()
                                {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            port.display_name
                                                .as_ref()
                                                .unwrap_or(&format!("{}", port_i)),
                                        );

                                        for _ in 0..port.channels {
                                            ui.selectable_value(
                                                &mut plugin.selected_port,
                                                PortChannel::AudioIn(channel_i),
                                                &format!("{}", channel_i),
                                            );

                                            channel_i += 1;
                                        }
                                    });
                                }

                                ui.separator();

                                ui.label("audio out");
                                let mut channel_i = 0;
                                for (port_i, port) in audio_ports_state
                                    .audio_ports_state_ext
                                    .outputs
                                    .iter()
                                    .enumerate()
                                {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            port.display_name
                                                .as_ref()
                                                .unwrap_or(&format!("{}", port_i)),
                                        );

                                        for _ in 0..port.channels {
                                            ui.selectable_value(
                                                &mut plugin.selected_port,
                                                PortChannel::AudioOut(channel_i),
                                                &format!("{}", channel_i),
                                            );

                                            channel_i += 1;
                                        }
                                    });
                                }

                                ui.separator();

                                // TODO: Let the user add/remove connections in this GUI.

                                ui.label("connections on port");
                                match plugin.selected_port {
                                    PortChannel::AudioIn(channel_i) => {
                                        if let Some(edges) =
                                            audio_ports_state.audio_in_edges.get(channel_i)
                                        {
                                            for edge in edges.iter() {
                                                ui.label(&format!(
                                                    "{:?} port {}",
                                                    edge.src_plugin_id, edge.src_channel
                                                ));
                                            }
                                        }
                                    }
                                    PortChannel::AudioOut(channel_i) => {
                                        if let Some(edges) =
                                            audio_ports_state.audio_out_edges.get(channel_i)
                                        {
                                            for edge in edges.iter() {
                                                ui.label(&format!(
                                                    "{:?} port {}",
                                                    edge.dst_plugin_id, edge.dst_channel
                                                ));
                                            }
                                        }
                                    }
                                }

                                ui.separator();
                            }

                            if let Some(params_state) = &mut plugin.params_state {
                                let mut values_to_set: Vec<(ParamID, f64)> = Vec::new();

                                for param in params_state.params.values_mut() {
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
                                                    egui::Slider::new(
                                                        &mut value,
                                                        min_value..=max_value,
                                                    )
                                                    .text(&param.display_name),
                                                )
                                                .changed()
                                            {
                                                values_to_set.push((param.id, value as f64));
                                                param.value = value as f64;
                                            }
                                        } else {
                                            if ui
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
                                        }

                                        if param.is_gesturing {
                                            ui.colored_label(egui::Color32::GREEN, "Gesturing");
                                        }
                                    });
                                }

                                for (param_id, value) in values_to_set.drain(..) {
                                    params_state.params_ext.set_value(param_id, value);
                                }
                            }
                        });
                });
        }
    } else {
        ui.label("Audio engine is deactivated");
    }
}
