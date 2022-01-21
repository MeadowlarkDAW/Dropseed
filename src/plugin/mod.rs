use std::borrow::Cow;
use std::error::Error;

use basedrop::Shared;

use crate::host::{Host, HostInfo};
use crate::process::{ProcAudioPorts, ProcInfo, ProcessStatus};

pub mod ext;

/// The description of a plugin.
pub struct PluginDescriptor<'a> {
    /// The unique reverse-domain-name identifier of this plugin.
    ///
    /// eg: "org.rustydaw.spicysynth"
    pub id: Cow<'a, str>,

    /// The displayable name of this plugin.
    ///
    /// eg: "Spicy Synth"
    pub name: Cow<'a, str>,

    /// The vendor of this plugin.
    ///
    /// eg: "RustyDAW"
    pub vendor: Cow<'a, str>,

    /// The version of this plugin.
    ///
    /// eg: "1.4.4" or "1.1.2_beta"
    pub version: Cow<'a, str>,

    /// A displayable short description of this plugin.
    ///
    /// eg: "Create flaming-hot sounds!"
    pub description: Cow<'a, str>,

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
    pub features: Option<Cow<'a, str>>,

    /// The url to the product page of this plugin.
    ///
    /// Set to `None` if there is no product page.
    pub url: Option<Cow<'a, str>>,

    /// The url to the online manual for this plugin.
    ///
    /// Set to `None` if there is no online manual.
    pub manual_url: Option<Cow<'a, str>>,

    /// The url to the online support page for this plugin.
    ///
    /// Set to `None` if there is no online support page.
    pub support_url: Option<Cow<'a, str>>,
}

/// The methods of an audio plugin which are used to create new instances of the plugin.
pub trait PluginFactory {
    /// Get the description of this plugin.
    ///
    /// This must be fast to execute as this is used while scanning plugins.
    fn description<'a>() -> PluginDescriptor<'a>;

    /// Create a new instance of this plugin.
    ///
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// `[main-thread]`
    fn new(
        host_info: Shared<HostInfo>,
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
    /// By default this does nothing.
    ///
    /// `[main-thread & !active_state]`
    #[allow(unused)]
    fn init(&mut self, host: &Host, coll_handle: &basedrop::Handle) {}

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
        sample_rate: f64,
        min_frames: usize,
        max_frames: usize,
        host: &Host,
        coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginAudioThread>, Box<dyn Error>>;

    /// Deactivate the plugin. When this is called it also means that the `PluginAudioThread`
    /// counterpart has/will be dropped.
    ///
    /// `[main-thread & active_state]`
    fn deactivate(&mut self, host: &Host);

    /// Called by the host on the main thread in response to a previous call to `host.request_callback()`.
    ///
    /// By default this does nothing.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn on_main_thread(&mut self, host: &Host) {}

    /// An optional extension that describes the configuration of audio ports on this plugin instance.
    ///
    /// This will only be called while the plugin is inactive.
    ///
    /// When `None` is returned then the plugin will have a default of one stereo input and one stereo
    /// output port. (Also the host may use the same buffer for these ports ("process replacing").)
    ///
    /// By default this returns `None`.
    ///
    /// [main-thread & !active_state]
    #[allow(unused)]
    fn audio_ports_extension<'a>(
        &'a self,
        host: &Host,
    ) -> Option<ext::audio_ports::PluginAudioPortsExtension<'a>> {
        None
    }
}

/// The methods of an audio plugin instance which run in the "audio" thread.
pub trait PluginAudioThread: Send + 'static {
    /// This will be called each time before a call to `process()` or `process_f64()`.
    ///
    /// Return an error if the plugin failed to start processing. In this case the host will not
    /// call `process()` this process cycle.
    ///
    /// By default this just returns `Ok(())`.
    ///
    /// `[audio-thread & active_state & !processing_state]`
    #[allow(unused)]
    fn start_processing(&mut self, host: &Host) -> Result<(), ()> {
        Ok(())
    }

    /// This will be called each time after a call to `process()` or `process_f64()`.
    ///
    /// By default this does nothing.
    ///
    /// `[audio-thread & active_state & processing_state]`
    #[allow(unused)]
    fn stop_processing(&mut self, host: &Host) {}

    /// Process audio and events (with `f32` audio buffers).
    ///
    /// The host may call this method even if your plugin has set `preferred_sample_size` to `F64`.
    /// In that case it is up to you to decide whether to process in `f32` or convert to/from `f64`
    /// buffers manually.
    ///
    /// `[audio-thread & active_state & processing_state]`
    fn process(
        &mut self,
        proc: &ProcInfo,
        audio: &mut ProcAudioPorts<f32>,
        host: &Host,
    ) -> ProcessStatus;

    /// Process audio and events (with `f64` audio buffers).
    ///
    /// The host will not call this method unless your plugin has set `preferred_sample_size` to `F64`.
    ///
    /// By default this just returns `ProcessStatus::Error`.
    ///
    /// `[audio-thread & active_state & processing_state]`
    #[allow(unused)]
    fn process_f64(
        &mut self,
        proc: &ProcInfo,
        audio: &mut ProcAudioPorts<f64>,
        host: &Host,
    ) -> ProcessStatus {
        ProcessStatus::Error
    }
}
