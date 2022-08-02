use std::fmt::Debug;

use super::ext::audio_ports::PluginAudioPortsExt;
use super::ext::note_ports::PluginNotePortsExt;
use crate::plugin_scanner::ScannedPluginKey;

#[derive(Clone)]
pub struct PluginPreset {
    /// The version of this plugin that saved this preset.
    pub version: Option<String>,

    /// The preset as raw bytes (use serde and bincode).
    pub bytes: Vec<u8>,
}

impl Debug for PluginPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("PluginPreset");

        f.field("version", &self.version);

        f.field("preset_size", &self.bytes.len());

        f.finish()
    }
}

#[derive(Debug, Clone)]
pub struct PluginSaveState {
    pub key: ScannedPluginKey,

    /// If this is `false` when receiving a save state, then it means that
    /// the plugin was deactivated at the time of collecting the save
    /// state/saving the project.
    ///
    /// If this is `false` when loading a new plugin, then the plugin will
    /// not be activated automatically.
    pub is_active: bool,

    /// Use this as a backup in case the plugin fails to load. (Most
    /// likey from a user opening another user's project, but the
    /// former user doesn't have this plugin installed on their system.)
    pub backup_audio_ports: Option<PluginAudioPortsExt>,

    /// Use this as a backup in case the plugin fails to load. (Most
    /// likey from a user opening another user's project, but the
    /// former user doesn't have this plugin installed on their system.)
    pub backup_note_ports: Option<PluginNotePortsExt>,

    /// The plugin's preset.
    ///
    /// If this is `None`, then the plugin will load its default preset.
    pub preset: Option<PluginPreset>,
}

impl PluginSaveState {
    pub fn new_with_default_preset(key: ScannedPluginKey) -> Self {
        Self {
            key,
            is_active: true,
            backup_audio_ports: None,
            backup_note_ports: None,
            preset: None,
        }
    }
}
