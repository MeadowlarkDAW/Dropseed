use dropseed_plugin_api::{PluginInstanceID, PluginPreset};
use std::path::PathBuf;

use dropseed_plugin_api::transport::TempoMap;

use crate::engine::main_thread::{ActivateEngineSettings, ModifyGraphRequest};

#[derive(Debug, Clone)]
/// A request to the engine.
///
/// Note that the engine may decide to ignore invalid requests.
pub enum DSEngineRequest {
    /// Modify the audio graph.
    ModifyGraph(ModifyGraphRequest),

    /// Activate the engine.
    ActivateEngine(Box<ActivateEngineSettings>),

    /// Deactivate the engine.
    ///
    /// The engine cannot be used until it is reactivated.
    DeactivateEngine,

    /// Request to get the save state of all plugins that have changed since
    /// the last request.
    ///
    /// Only the state of the plugins which have changed their save state since
    /// the last request will be returned.
    RequestLatestSaveStates,

    #[cfg(feature = "clap-host")]
    /// Add a directory to the list of directories to scan for CLAP plugins.
    AddClapScanDirectory(PathBuf),

    #[cfg(feature = "clap-host")]
    /// Remove a directory from the list of directories to scan for CLAP plugins.
    RemoveClapScanDirectory(PathBuf),

    /// Rescan all plugin directories.
    RescanPluginDirectories,

    UpdateTempoMap(Box<TempoMap>),

    /// A request to a specific Plugin instance
    Plugin(PluginInstanceID, PluginRequest),
}

impl From<ModifyGraphRequest> for DSEngineRequest {
    fn from(m: ModifyGraphRequest) -> Self {
        DSEngineRequest::ModifyGraph(m)
    }
}

#[derive(Debug, Clone)]
/// A request to a specific instantiated Plugin
pub enum PluginRequest {
    ShowGui,
    CloseGui,
    /// Request the plugin to load a preset.
    LoadPreset(PluginPreset),
    GetLatestSaveState,
}
