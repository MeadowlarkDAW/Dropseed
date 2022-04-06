use crate::{graph::PluginInstanceID, plugin::PluginDescriptor, plugin_scanner::ScannedPluginKey};

#[derive(Debug, Clone)]
pub enum AudioEngineEvent {
    PluginScanned { key: ScannedPluginKey, descriptor: PluginDescriptor },

    PluginInstanceCreated(PluginInstanceID),
    PluginInstanceRemoved(PluginInstanceID),
    PluginActivated(PluginInstanceID),
    PluginDeactivated(PluginInstanceID),
}

pub struct ActionsQueue {
    pub(crate) actions: Vec<AudioEngineEvent>,
}
