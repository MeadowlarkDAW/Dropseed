use crate::plugin_scanner::ScannedPluginKey;

#[derive(Debug, Clone)]
pub struct PluginSaveState {
    pub key: ScannedPluginKey,

    pub activation_requested: bool,

    /// In case the plugin fails to load, use this as a backup method for
    /// retrieving the number of audio channels. If the plugin does load
    /// successfully then this will be overwritten once loaded.
    pub audio_in_out_channels: (u16, u16),

    // TODO
    pub _preset: (),
}
