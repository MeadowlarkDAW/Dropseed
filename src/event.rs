use std::path::PathBuf;

use crate::{
    graph::{
        AudioGraphRestoredInfo, AudioGraphSaveState, GraphCompilerError, PluginActivatedInfo,
        PluginInstanceID,
    },
    plugin_scanner::RescanPluginDirectoriesRes,
};

#[derive(Debug)]
pub enum DAWEngineEvent {
    /// When this message is received, it means that the audio graph is in an
    /// invalid state.
    ///
    /// To keep using the audio graph, you must reactivate the engine with
    /// `RustyDAWEngine::activate_engine()`, and then restore the audio graph
    /// from the most recently working save state.
    EngineDeactivatedBecauseGraphIsInvalid(GraphCompilerError),

    /// Called when the user requested to deactivate the audio engine.
    ///
    /// To keep using the audio graph, you must reactivate the engine with
    /// `RustyDAWEngine::activate_engine()`, and then restore the audio graph
    /// from the most recently working save state.
    EngineDeactivated,

    /// When this message is received, it means that the audio graph is starting
    /// the process of restoring from a save state.
    ///
    /// Reset your UI as if you are loading up a project for the first time, and
    /// wait for the `AudioGraphRestoredFromSaveState` event to repopulate your UI.
    ///
    /// If the audio graph is in an invalid state as a result of restoring from
    /// the save state, then the `EngineDeactivatedBecauseGraphIsInvalid` event
    /// will be sent instead.
    AudioGraphCleared,

    /// When this message is received, it means that the audio graph has finished
    /// the process of restoring from a save state.
    ///
    /// Use the given info to populate the UI with the elements in the audio graph,
    /// as well as display any errors that occurred while restoring the save state.
    AudioGraphRestoredFromSaveState(AudioGraphRestoredInfo),

    /// This message is sent after the user requests the latest save state from
    /// calling `RustyDAWEngine::request_latest_save_state()`.
    ///
    /// Use the latest save state as a backup in case a plugin crashes or a bug
    /// in the audio graph compiler causes the audio graph to be in an invalid
    /// state, resulting in the audio engine stopping.
    NewSaveState(AudioGraphSaveState),

    /// The given plugin successfully restarted. Make sure your UI updates the
    /// port configuration on this plugin.
    PluginRestarted(PluginActivatedInfo),
    /// The given plugin failed to restart and is now deactivated.
    PluginFailedToRestart(PluginInstanceID),

    PluginScanner(PluginScannerEvent),
    // TODO: More stuff
}

#[derive(Debug)]
pub enum PluginScannerEvent {
    ScanPathAdded(PathBuf),
    ScanPathRemoved(PathBuf),
    RescanFinished(RescanPluginDirectoriesRes),
}

impl From<PluginScannerEvent> for DAWEngineEvent {
    fn from(e: PluginScannerEvent) -> Self {
        DAWEngineEvent::PluginScanner(e)
    }
}
