use std::fmt::Debug;

use super::ext::audio_ports::PluginAudioPortsExt;
use super::ext::note_ports::PluginNotePortsExt;
use crate::plugin_scanner::ScannedPluginKey;

#[derive(Clone)]
pub struct DSPluginSaveState {
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

    /// The plugin's state/preset as raw bytes.
    ///
    /// If this is `None`, then the plugin will load its default
    /// state/preset.
    pub raw_state: Option<Vec<u8>>,
}

impl DSPluginSaveState {
    pub fn new_with_default_state(key: ScannedPluginKey) -> Self {
        Self {
            key,
            is_active: true,
            backup_audio_ports: None,
            backup_note_ports: None,
            raw_state: None,
        }
    }
}

impl Debug for DSPluginSaveState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("DSPluginSaveState");

        f.field("key", &self.key);
        f.field("is_active", &self.is_active);
        f.field("backup_audio_ports", &self.backup_audio_ports);
        f.field("backup_note_ports", &self.backup_note_ports);

        if let Some(s) = &self.raw_state {
            f.field("raw_state size", &format!("{}", s.len()));
        } else {
            f.field("raw_state", &"None");
        }

        f.finish()
    }
}
