use eframe::egui;
use rusty_daw_engine::{
    plugin::ext::audio_ports::AudioPortsExtension, PluginEdges, PluginInstanceID,
};

use super::BasicDawExampleGUI;

pub struct EffectRackPluginState {
    pub plugin_name: String,
    pub plugin_id: PluginInstanceID,
    pub audio_ports: Option<AudioPortsExtension>,
    pub edges: PluginEdges,
    pub active: bool,
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
    if let Some(engine_state) = &app.engine_state {
        for plugin in engine_state.effect_rack_state.plugins.iter() {
            ui.label(&plugin.plugin_name);
        }
    } else {
        ui.label("Audio engine is deactivated");
    }
}
