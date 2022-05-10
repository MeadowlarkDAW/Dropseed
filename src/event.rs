use std::path::PathBuf;

use crate::{
    engine::EngineActivatedInfo,
    graph::{
        AudioGraphRestoredInfo, AudioGraphSaveState, GraphCompilerError, NewPluginRes,
        PluginActivatedInfo, PluginActivationError, PluginEdgesChangedInfo, PluginInstanceID,
    },
    plugin_scanner::RescanPluginDirectoriesRes,
};

#[derive(Debug)]
pub enum DAWEngineEvent {
    /// Sent whenever the engine is deactivated.
    ///
    /// If the result is `Ok(save_state)`, then it means that the engine
    /// deactivated gracefully via calling `RustyDAWEngine::deactivate_engine()`,
    /// and the latest save state of the audio graph is returned.
    ///
    /// If the result is `Err(e)`, then it means that the engine deactivated
    /// because of a unrecoverable audio graph compiler error.
    ///
    /// To keep using the audio graph, you must reactivate the engine with
    /// `RustyDAWEngine::activate_engine()`, and then restore the audio graph
    /// from an existing save state if you wish using
    /// `RustyDAWEngine::restore_audio_graph_from_save_state()`.
    EngineDeactivated(Result<AudioGraphSaveState, GraphCompilerError>),

    /// This message is sent whenever the engine successfully activates.
    EngineActivated(EngineActivatedInfo),

    /// When this message is received, it means that the audio graph is starting
    /// the process of restoring from a save state.
    ///
    /// Reset your UI as if you are loading up a project for the first time, and
    /// wait for the `PluginInstancesAdded` and `PluginEdgesChanged` events in
    /// order to repopulate the UI.
    ///
    /// If the audio graph is in an invalid state as a result of restoring from
    /// the save state, then the `EngineDeactivatedBecauseGraphIsInvalid` event
    /// will be sent instead.
    AudioGraphCleared,

    /// When this message is received, it means that the audio graph has finished
    /// the process of restoring from a save state.
    AudioGraphRestoredFromSaveState(AudioGraphRestoredInfo),

    /// This message is sent after the user requests the latest save state from
    /// calling `RustyDAWEngine::request_latest_save_state()`.
    ///
    /// Use the latest save state as a backup in case a plugin crashes or a bug
    /// in the audio graph compiler causes the audio graph to be in an invalid
    /// state, resulting in the audio engine stopping.
    NewSaveState(AudioGraphSaveState),

    /// This message is sent after a request to add a new plugin by calling
    /// `RustyDAWEngine::add_new_plugin_instance()`
    ///
    /// Note, if you called `RustyDAWEngine::insert_new_plugin_between_main_ports()`,
    /// then the `RustyDAWEngine::PluginInsertedBetween` result will be returned
    /// instead.
    PluginInstancesAdded(Vec<NewPluginRes>),

    /// This message is sent after a request to remove a plugin by calling
    /// either `RustyDAWEngine::remove_plugin_instances()` or
    /// `RustyDAWEngine::remove_plugin_between()`.
    ///
    /// Note that the host will always send a `PluginEdgesChanged` event
    /// before this event if any of the removed plugins had connected
    /// edges. This `PluginEdgesChanged` event will have all edges that
    /// were connected to any of the removed plugins removed.
    PluginInstancesRemoved(Vec<PluginInstanceID>),

    /// This message is sent whenever the edges of plugins change (including
    /// when adding/removing plugins).
    PluginEdgesChanged(PluginEdgesChangedInfo),

    /// The given plugin successfully restarted. Make sure your UI updates the
    /// port configuration on this plugin.
    PluginRestarted(PluginActivatedInfo),
    /// The given plugin failed to restart and is now deactivated.
    PluginFailedToRestart(PluginInstanceID, PluginActivationError),

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
