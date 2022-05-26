use rusty_daw_core::SampleRate;
use std::error::Error;

use crate::{host_request::HostRequest, PluginInstanceID};

pub mod audio_buffer;
pub mod ext;

pub(crate) mod process_info;

mod save_state;

use process_info::{ProcInfo, ProcessStatus};

pub use save_state::PluginSaveState;

use self::process_info::ProcBuffers;

/// The description of a plugin.
#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    /// The unique reverse-domain-name identifier of this plugin.
    ///
    /// eg: "org.rustydaw.spicysynth"
    pub id: String,

    /// The version of this plugin.
    ///
    /// eg: "1.4.4" or "1.1.2_beta"
    pub version: String,

    /// The displayable name of this plugin.
    ///
    /// eg: "Spicy Synth"
    pub name: String,

    /// The vendor of this plugin.
    ///
    /// eg: "RustyDAW"
    pub vendor: String,

    /// A displayable short description of this plugin.
    ///
    /// eg: "Create flaming-hot sounds!"
    pub description: String,

    /* TODO
    /// Arbitrary list of keywords, separated by `;'.
    ///
    /// They can be matched by the host search engine and used to classify the plugin.
    ///
    /// Some pre-defined keywords:
    /// - "instrument", "audio_effect", "note_effect", "analyzer"
    /// - "mono", "stereo", "surround", "ambisonic"
    /// - "distortion", "compressor", "limiter", "transient"
    /// - "equalizer", "filter", "de-esser"
    /// - "delay", "reverb", "chorus", "flanger"
    /// - "tool", "utility", "glitch"
    ///
    /// Some examples:
    /// - "equalizer;analyzer;stereo;mono"
    /// - "compressor;analog;character;mono"
    /// - "reverb;plate;stereo"
    pub features: Option<String>,
    */
    /// The url to the product page of this plugin.
    pub url: String,

    /// The url to the online manual for this plugin.
    pub manual_url: String,

    /// The url to the online support page for this plugin.
    pub support_url: String,
}

/// The methods of an audio plugin which are used to create new instances of the plugin.
pub trait PluginFactory {
    fn description(&self) -> PluginDescriptor;

    /// Create a new instance of this plugin.
    ///
    /// **NOTE**: The plugin is **NOT** allowed to use the host callbacks in this method.
    /// Wait until the `PluginMainThread::init()` method gets called on the plugin to
    /// start using the host callbacks.
    ///
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// `[main-thread]`
    fn new(
        &mut self,
        host: HostRequest,
        plugin_id: PluginInstanceID,
        coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginMainThread>, Box<dyn Error>>;
}

/// The methods of an audio plugin instance which run in the "main" thread.
pub trait PluginMainThread {
    /// This is called after creating a plugin instance and once it's safe for the plugin to
    /// use the host callback methods.
    ///
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// If this returns an error, then the host will discard this plugin instance.
    ///
    /// By default this returns `Ok(())`.
    ///
    /// TODO: preset
    ///
    /// `[main-thread & !active_state]`
    #[allow(unused)]
    fn init(&mut self, _preset: (), coll_handle: &basedrop::Handle) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    /// Activate the plugin, and return the `PluginAudioThread` counterpart.
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
    ) -> Result<Box<dyn PluginAudioThread>, Box<dyn Error>>;

    /// Deactivate the plugin. When this is called it also means that the `PluginAudioThread`
    /// counterpart has/will be dropped.
    ///
    /// `[main-thread & active_state]`
    fn deactivate(&mut self);

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
    fn audio_ports_extension(
        &mut self,
    ) -> Result<ext::audio_ports::PluginAudioPortsExt, Box<dyn Error>> {
        Ok(ext::audio_ports::PluginAudioPortsExt::empty())
    }
}

/// The methods of an audio plugin instance which run in the "audio" thread.
pub trait PluginAudioThread: Send + Sync + 'static {
    /// This will be called when the plugin should start processing after just activing/
    /// waking up from sleep.
    ///
    /// Return an error if the plugin failed to start processing. In this case the host will not
    /// call `process()` and return the plugin to sleep.
    ///
    /// By default this just returns `Ok(())`.
    ///
    /// `[audio-thread & active_state & !processing_state]`
    #[allow(unused)]
    fn start_processing(&mut self) -> Result<(), ()> {
        Ok(())
    }

    /// This will be called when the host puts the plugin to sleep.
    ///
    /// By default this trait method does nothing.
    ///
    /// `[audio-thread & active_state & processing_state]`
    #[allow(unused)]
    fn stop_processing(&mut self) {}

    /// Process audio and events.
    ///
    /// `[audio-thread & active_state & processing_state]`
    fn process(&mut self, proc_info: &ProcInfo, buffers: &mut ProcBuffers) -> ProcessStatus;
}
