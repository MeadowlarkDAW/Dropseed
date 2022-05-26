use eframe::egui;
use rusty_daw_engine::{
    plugin::ext::audio_ports::AudioPortsExtension, Edge, PluginEdges, PluginInstanceID, PortType,
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
    audio_ports_state_ext: AudioPortsExtension,

    audio_in_edges: Vec<Vec<Edge>>,
    audio_out_edges: Vec<Vec<Edge>>,
}

impl AudioPortState {
    pub fn new(audio_ports_state_ext: AudioPortsExtension) -> Self {
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

pub struct EffectRackPluginState {
    pub plugin_name: String,
    pub plugin_id: PluginInstanceID,
    pub audio_ports_state: Option<AudioPortState>,
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
                    egui::ScrollArea::vertical().id_source(&format!("plugin{}hscroll", plugin_i)).show(ui, |ui| {
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
                                            ui.label(&format!("{:?} port {}", edge.src_plugin_id, edge.src_channel));
                                        }
                                    }
                                }
                                PortChannel::AudioOut(channel_i) => {
                                    if let Some(edges) =
                                        audio_ports_state.audio_out_edges.get(channel_i)
                                    {
                                        for edge in edges.iter() {
                                            ui.label(&format!("{:?} port {}", edge.dst_plugin_id, edge.dst_channel));
                                        }
                                    }
                                }
                            }

                            ui.separator();
                        }

                        // TODO: Parameters
                    });
                });
        }
    } else {
        ui.label("Audio engine is deactivated");
    }
}
