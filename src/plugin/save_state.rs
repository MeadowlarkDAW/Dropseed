use crate::{engine::plugin_scanner::ScannedPluginKey, PluginAudioPortsExt};

use super::ext::note_ports::PluginNotePortsExt;

#[derive(Debug, Clone)]
pub struct PluginSaveState {
    pub key: ScannedPluginKey,

    pub activation_requested: bool,

    /// Use this as a backup in case the plugin fails to load. (Most
    /// likey from a user opening another user's project, but the
    /// user doesn't have this plugin installed on their system.)
    pub backup_audio_ports: Option<PluginAudioPortsExt>,

    /// Use this as a backup in case the plugin fails to load. (Most
    /// likey from a user opening another user's project, but the
    /// user doesn't have this plugin installed on their system.)
    pub backup_note_ports: Option<PluginNotePortsExt>,

    // TODO
    pub _preset: (),
}

impl PluginSaveState {
    pub fn new_with_default_preset(key: ScannedPluginKey) -> Self {
        Self {
            key,
            activation_requested: true,
            backup_audio_ports: None,
            backup_note_ports: None,
            _preset: (),
        }
    }
}
