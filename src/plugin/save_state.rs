use crate::plugin_scanner::ScannedPluginKey;

#[derive(Debug, Clone)]
pub struct PluginSaveState {
    pub key: ScannedPluginKey,

    pub activated: bool,

    // TODO
    pub _preset: (),
}
