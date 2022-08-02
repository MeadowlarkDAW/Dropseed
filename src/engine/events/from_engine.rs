use fnv::FnvHashMap;
use smallvec::SmallVec;
use std::path::PathBuf;

use dropseed_plugin_api::{ParamID, PluginInstanceID, PluginSaveState};

use crate::{
    engine::main_thread::{EngineActivatedInfo, ModifyGraphRes},
    engine::plugin_scanner::RescanPluginDirectoriesRes,
    graph::{plugin_host::PluginHandle, ActivatePluginError, ParamModifiedInfo},
};

#[derive(Debug)]
#[non_exhaustive]
pub enum DSEngineEvent {
    /// Sent whenever the engine is deactivated.
    ///
    /// The DSEngineAudioThread sent in a previous EngineActivated event is now
    /// invalidated. Please drop it and wait for a new EngineActivated event to
    /// replace it.
    ///
    /// To keep using the audio graph, you must reactivate the engine with
    /// `DSEngineRequest::ActivateEngine` and repopulate the graph.
    EngineDeactivated(EngineDeactivatedInfo),

    /// This message is sent whenever the engine successfully activates.
    EngineActivated(EngineActivatedInfo),

    /// This message is sent after the user requests the latest save states from
    /// calling `DSEngineRequest::RequestLatestSaveStates` or
    /// `PluginRequest::GetLatestSaveState`.
    ///
    /// This only returns the save states of the plugins which have changed their
    /// state.
    ///
    /// Use the latest save state as a backup in case a plugin crashes or a bug
    /// in the audio graph compiler causes the audio graph to be in an invalid
    /// state, resulting in the audio engine stopping.
    NewSaveStates(Vec<(PluginInstanceID, PluginSaveState)>),

    /// When this message is received, it means that the audio graph has been
    /// cleared.
    ///
    /// Reset your UI as if you are loading up a project for the first time, and
    /// wait for the `AudioGraphModified` event to repopulate the UI.
    AudioGraphCleared,

    /// This message is sent whenever the audio graph has been modified.
    ///
    /// Be sure to update your UI from this new state.
    AudioGraphModified(ModifyGraphRes),

    Plugin(PluginEvent),

    PluginScanner(PluginScannerEvent),
    // TODO: More stuff
}

#[derive(Debug)]
/// Sent whenever the engine is deactivated.
///
/// The DSEngineAudioThread sent in a previous EngineActivated event is now
/// invalidated. Please drop it and wait for a new EngineActivated event to
/// replace it.
///
/// To keep using the audio graph, you must reactivate the engine with
/// `DSEngineRequest::ActivateEngine`, and then restore the audio graph
/// from an existing save state if you wish using
/// `DSEngineRequest::RestoreFromSaveState`.
pub enum EngineDeactivatedInfo {
    /// The engine was deactivated gracefully after recieving a
    /// `DSEngineRequest::DeactivateEngine` request.
    DeactivatedGracefully,
    /// The engine has crashed.
    EngineCrashed { error_msg: String },
}

#[derive(Debug)]
#[non_exhaustive]
pub enum PluginEvent {
    /// Sent whenever a plugin becomes activated after being deactivated or
    /// when the plugin restarts.
    ///
    /// Make sure your UI updates the port configuration on this plugin.
    Activated {
        plugin_id: PluginInstanceID,
        new_handle: PluginHandle,
        new_param_values: FnvHashMap<ParamID, f64>,
    },

    /// Sent whenever a plugin becomes deactivated. When a plugin is deactivated
    /// you cannot access any of its methods until it is reactivated.
    Deactivated {
        plugin_id: PluginInstanceID,
        /// If this is `Ok(())`, then it means the plugin was gracefully
        /// deactivated from user request.
        ///
        /// If this is `Err(e)`, then it means the plugin became deactivated
        /// because it failed to restart.
        status: Result<(), ActivatePluginError>,
    },

    ParamsModified {
        plugin_id: PluginInstanceID,
        modified_params: SmallVec<[ParamModifiedInfo; 4]>,
    },

    /// Sent when the plugin closed its own GUI by its own means. UI should be updated accordingly
    /// so that the user could open the UI again.
    GuiClosed { plugin_id: PluginInstanceID },
}

#[derive(Debug)]
#[non_exhaustive]
pub enum PluginScannerEvent {
    #[cfg(feature = "clap-host")]
    /// A new CLAP plugin scan path was added.
    ClapScanPathAdded(PathBuf),
    #[cfg(feature = "clap-host")]
    /// A CLAP plugin scan path was removed.
    ClapScanPathRemoved(PathBuf),

    /// A request to rescan all plugin directories has finished. Update
    /// the list of available plugins in your UI.
    RescanFinished(RescanPluginDirectoriesRes),
}

impl From<PluginScannerEvent> for DSEngineEvent {
    fn from(e: PluginScannerEvent) -> Self {
        DSEngineEvent::PluginScanner(e)
    }
}
