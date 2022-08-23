use basedrop::Shared;
use clack_extensions::gui::GuiError;
use clack_host::events::io::EventBuffer;
use meadowlark_core_types::time::SampleRate;

use super::transport::TempoMap;
use super::{ext, ParamID, PluginProcessThread};

/// The methods of an audio plugin instance which run in the "main" thread.
pub trait PluginMainThread {
    /// Activate the plugin, and return the `PluginProcessThread` counterpart.
    ///
    /// In this call the plugin may allocate memory and prepare everything needed for the process
    /// call. The process's sample rate will be constant and process's frame count will included in
    /// the `[min, max]` range, which is bounded by `[1, INT32_MAX]`.
    ///
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// Once activated the latency and port configuration must remain constant, until deactivation.
    ///
    /// `[main-thread & !active_state]`
    fn activate(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        coll_handle: &basedrop::Handle,
    ) -> Result<PluginActivatedInfo, String>;

    /// Collect the save state/preset of this plugin as raw bytes (use serde and bincode).
    ///
    /// If `Ok(None)` is returned, then it means that the plugin does not have a
    /// state it needs to save.
    ///
    /// By default this returns `None`.
    ///
    /// `[main-thread]`
    fn collect_save_state(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    /// Load the given save state/preset (use serde and bincode).
    ///
    /// By default this does nothing.
    ///
    /// `[main-thread]`
    #[allow(unused)]
    fn load_save_state(&mut self, state: Vec<u8>) -> Result<(), String> {
        Ok(())
    }

    /// Deactivate the plugin. When this is called it also means that the `PluginProcessThread`
    /// counterpart will already have been dropped.
    ///
    /// `[main-thread & active_state]`
    fn deactivate(&mut self) {}

    /// Called by the host on the main thread in response to a previous call to `host.request_callback()`.
    ///
    /// By default this does nothing.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn on_main_thread(&mut self) {}

    /// An optional extension that describes the configuration of audio ports on this plugin instance.
    ///
    /// This will only be called while the plugin is inactive.
    ///
    /// The default configuration is one with no audio ports.
    ///
    /// [main-thread & !active_state]
    #[allow(unused)]
    fn audio_ports_ext(&mut self) -> Result<ext::audio_ports::PluginAudioPortsExt, String> {
        Ok(ext::audio_ports::EMPTY_AUDIO_PORTS_CONFIG.clone())
    }

    /// An optional extension that describes the configuration of note ports on this plugin instance.
    ///
    /// This will only be called while the plugin is inactive.
    ///
    /// The default configuration is one with no note ports.
    ///
    /// [main-thread & !active_state]
    #[allow(unused)]
    fn note_ports_ext(&mut self) -> Result<ext::note_ports::PluginNotePortsExt, String> {
        Ok(ext::note_ports::EMPTY_NOTE_PORTS_CONFIG.clone())
    }

    // --- Parameters ---------------------------------------------------------------------------------

    /// Get the total number of parameters in this plugin.
    ///
    /// You may return 0 if this plugins has no parameters.
    ///
    /// By default this returns 0.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn num_params(&mut self) -> u32 {
        0
    }

    /// Get the info of the given parameter.
    ///
    /// (Note this is takes the index of the parameter as input (length given by `num_params()`), *NOT* the ID of the parameter)
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns an Err(()).
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_info(&mut self, param_index: usize) -> Result<ext::params::ParamInfo, ()> {
        Err(())
    }

    /// Get the plain value of the parameter.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `Err(())`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_value(&self, param_id: ParamID) -> Result<f64, ()> {
        Err(())
    }

    /// Format the display text for the given parameter value.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `Err(())`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_value_to_text(
        &self,
        param_id: ParamID,
        value: f64,
        text_buffer: &mut String,
    ) -> Result<(), String> {
        Err(String::new())
    }

    /// Convert the text input to a parameter value.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `None`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_text_to_value(&self, param_id: ParamID, text_input: &str) -> Option<f64> {
        None
    }

    /// Called when the tempo map is updated.
    ///
    /// By default this does nothing.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn update_tempo_map(&mut self, new_tempo_map: &Shared<TempoMap>) {}

    /// Whether or not this plugin has an automation out port (seperate from audio and note
    /// out ports).
    ///
    /// Only return `true` for internal plugins which output parameter automation events for
    /// other plugins.
    ///
    /// By default this returns `false`.
    ///
    /// [main-thread]
    fn has_automation_out_port(&self) -> bool {
        false
    }

    /// Flushes a set of parameter changes.
    ///
    /// This will only be called while the plugin is inactive.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// This method will not be called concurrently to clap_plugin->process().
    ///
    /// This method will not be used while the plugin is processing.
    ///
    /// By default this does nothing.
    ///
    /// [active && !processing : process-thread]
    #[allow(unused)]
    fn param_flush(&mut self, in_events: &EventBuffer, out_events: &mut EventBuffer) {}

    // --- GUI ---------------------------------------------------------------------------------

    /// Returns whether or not this plugin instance supports opening a floating GUI window.
    fn has_gui(&self) -> bool {
        false
    }

    fn is_gui_open(&self) -> bool {
        false
    }

    /// Initializes and opens the plugin's GUI
    // TODO: better error type
    fn open_gui(&mut self, _suggested_title: Option<&str>) -> Result<(), GuiError> {
        Err(GuiError::CreateError)
    }

    /// Called when the plugin notified its GUI has been closed.
    ///
    /// `destroyed` is set to true if the GUI has also been destroyed completely, e.g. due to a
    /// lost connection.
    #[allow(unused)]
    fn on_gui_closed(&mut self, destroyed: bool) {}

    /// Closes and destroys the currently active GUI
    fn close_gui(&mut self) {}

    /// The latency in frames this plugin adds.
    /// 
    /// The plugin is only allowed to change its latency when it is deactivated.
    /// 
    /// By default this returns `0` (no latency).
    /// 
    /// [main-thread & !active_state]
    fn latency(&self) -> i64 {
        0
    }
}

pub struct PluginActivatedInfo {
    pub processor: Box<dyn PluginProcessThread>,
    pub internal_handle: Option<Box<dyn std::any::Any + Send + 'static>>,
}
