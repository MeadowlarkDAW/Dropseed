use basedrop::Shared;
use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfoBuffer, HostAudioPorts, HostAudioPortsImplementation,
    PluginAudioPorts, RescanType,
};
use clack_extensions::log::implementation::HostLog;
use clack_extensions::log::{Log, LogSeverity};
use clack_extensions::params::{
    HostParams, HostParamsImplementation, HostParamsImplementationMainThread,
    ParamClearFlags as ClackParamClearFlags, ParamRescanFlags as ClackParamRescanFlags,
    PluginParams,
};
use clack_extensions::state::PluginState;
use clack_extensions::thread_check::host::ThreadCheckImplementation;
use clack_extensions::thread_check::ThreadCheck;
use clack_host::events::io::{InputEvents, OutputEvents};
use clack_host::extensions::HostExtensions;
use clack_host::host::{Host, HostAudioProcessor, HostMainThread, HostShared};
use meadowlark_core_types::SampleRate;
use std::ffi::CString;
use std::io::Cursor;
use std::mem::MaybeUninit;

use atomic_refcell::{AtomicRef, AtomicRefMut};
use smallvec::SmallVec;

use clack_host::instance::processor::PluginAudioProcessor;
use clack_host::instance::{PluginAudioConfiguration, PluginInstance};
use clack_host::plugin::{PluginAudioProcessorHandle, PluginMainThreadHandle, PluginSharedHandle};

use super::process::ClapProcess;
use crate::plugin::audio_buffer::RawAudioChannelBuffers;
use crate::plugin::ext::params::{ParamClearFlags, ParamRescanFlags};
use crate::plugin::process_info::{ProcBuffers, ProcInfo, ProcessStatus};
use crate::plugin::{ext, PluginActivatedInfo, PluginAudioThread, PluginMainThread, PluginPreset};
use crate::utils::thread_id::SharedThreadIDs;
use crate::{AudioPortInfo, EventBuffer, ParamID};
use crate::{HostRequest, PluginAudioPortsExt, PluginInstanceID};
use crate::{MainPortsLayout, ParamInfo, ParamInfoFlags};

pub(crate) struct ClapPluginMainThread {
    instance: PluginInstance<ClapHost>,
    audio_ports_ext: PluginAudioPortsExt,
}

impl ClapPluginMainThread {
    pub(crate) fn new(instance: PluginInstance<ClapHost>) -> Result<Self, String> {
        Ok(Self { audio_ports_ext: Self::parse_audio_ports_extension(&instance)?, instance })
    }
}

impl ClapPluginMainThread {
    #[inline]
    fn id(&self) -> &str {
        &*self.instance.shared_host_data().id
    }

    fn parse_audio_ports_extension(
        instance: &PluginInstance<ClapHost>,
    ) -> Result<PluginAudioPortsExt, String> {
        let id = &*instance.shared_host_data().id;
        log::trace!("clap plugin instance parse audio ports extension {}", id);

        if instance.is_active() {
            return Err("Cannot get audio ports extension while plugin is active".into());
        }

        let audio_ports = match instance.shared_plugin_data().get_extension::<PluginAudioPorts>() {
            None => return Ok(PluginAudioPortsExt::empty()),
            Some(e) => e,
        };

        let plugin = instance.main_thread_plugin_data();

        let num_in_ports = audio_ports.count(&plugin, true);
        let num_out_ports = audio_ports.count(&plugin, false);

        let mut buffer = AudioPortInfoBuffer::new();

        let mut has_main_in_port = false;
        let mut has_main_out_port = false;

        let inputs: Vec<AudioPortInfo> = (0..num_in_ports).filter_map(|i| {
            let raw_info = match audio_ports.get(&plugin, i, true, &mut buffer) {
                None => {
                    log::warn!("Error when getting CLAP Port Info from plugin instance {}: plugin returned no info for index {}", id, i);
                    return None;
                },
                Some(i) => i
            };

            let port_type = raw_info.port_type.and_then(|t| Some(t.0.to_str().ok()?.to_string()));

            let display_name = match raw_info.name.to_str() {
                Ok(s) => Some(s.to_string()),
                Err(_) => {
                    log::warn!("Failed to get clap_audio_port_info.name from plugin instance {}", id);
                    None
                }
            };

            if raw_info.flags.contains(AudioPortFlags::IS_MAIN) {
                if has_main_in_port {
                    log::warn!("Plugin instance {} already has a main input port (at port index {})", id, i)
                } else {
                    has_main_in_port = true;
                }
            }

            Some(AudioPortInfo {
                stable_id: raw_info.id,
                channels: raw_info.channel_count as u16,
                port_type,
                display_name,
            })
        }).collect();

        let outputs: Vec<AudioPortInfo> = (0..num_out_ports).filter_map(|i| {
            let raw_info = match audio_ports.get(&plugin, i, false, &mut buffer) {
                None => {
                    log::warn!("Error when getting CLAP Port Info from plugin instance {}: plugin returned no info for index {}", id, i);
                    return None;
                },
                Some(i) => i
            };

            let port_type = raw_info.port_type.and_then(|t| Some(t.0.to_str().ok()?.to_string()));

            let display_name = match raw_info.name.to_str() {
                Ok(s) => Some(s.to_string()),
                Err(_) => {
                    log::warn!("Failed to get clap_audio_port_info.name from plugin instance {}", id);
                    None
                }
            };

            if raw_info.flags.contains(AudioPortFlags::IS_MAIN) {
                if has_main_out_port {
                    log::warn!("Plugin instance {} already has a main output port (at port index {})", id, i)
                } else {
                    has_main_out_port = true;
                }
            }

            Some(AudioPortInfo {
                stable_id: raw_info.id,
                channels: raw_info.channel_count as u16,
                port_type,
                display_name,
            })
        }).collect();

        let main_ports_layout = match (has_main_in_port, has_main_out_port) {
            (true, true) => MainPortsLayout::InOut,
            (true, false) => MainPortsLayout::InOnly,
            (false, true) => MainPortsLayout::OutOnly,
            (false, false) => MainPortsLayout::NoMainPorts,
        };

        Ok(PluginAudioPortsExt { inputs, outputs, main_ports_layout })
    }
}

impl PluginMainThread for ClapPluginMainThread {
    /// Activate the plugin, and return the `PluginAudioThread` counterpart.
    ///
    /// In this call the plugin may allocate memory and prepare everything needed for the process
    /// call. The process's sample rate will be constant and process's frame count will included in
    /// the `[min, max]` range, which is bounded by `[1, INT32_MAX]`.
    ///
    /// Once activated the latency and port configuration must remain constant, until deactivation.
    ///
    /// `[main-thread & !active_state]`
    fn activate(
        &mut self,
        sample_rate: SampleRate,
        min_frames: u32,
        max_frames: u32,
        _coll_handle: &basedrop::Handle,
    ) -> Result<PluginActivatedInfo, String> {
        let configuration = PluginAudioConfiguration {
            sample_rate: sample_rate.0,
            frames_count_range: min_frames..=max_frames,
        };

        log::trace!("clap plugin instance activate {}", self.id());
        let audio_processor = match self
            .instance
            .activate(|plugin, shared, _| ClapHostAudioProcessor { plugin, shared }, configuration)
        {
            Ok(p) => p,
            Err(e) => return Err(format!("{}", e)),
        };

        Ok(PluginActivatedInfo {
            audio_thread: Box::new(ClapPluginAudioThread {
                audio_processor: audio_processor.into(),
                process: ClapProcess::new(&self.audio_ports_ext),
            }),
            internal_handle: None,
        })
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
    /// [!active : main-thread]
    /*fn param_flush(&mut self, in_events: &EventQueue, out_events: &mut EventQueue) {
        self.instance.main_thread_host_data_mut().param_flush(in_events, out_events)
    }*/

    /// Collect the save state of this plugin as raw bytes (use serde and bincode).
    ///
    /// If `Ok(None)` is returned, then it means that the plugin does not have a
    /// state it needs to save.
    ///
    /// By default this returns `None`.
    ///
    /// `[main-thread]`
    fn collect_save_state(&mut self) -> Result<Option<Vec<u8>>, String> {
        if let Some(state_ext) = self.instance.shared_host_data().state_ext {
            let mut buffer = Vec::new();

            state_ext.save(self.instance.main_thread_plugin_data(), &mut buffer).map_err(|_| {
                format!(
                    "Plugin with ID {} returned error on call to clap_plugin_state.save()",
                    &*self.id()
                )
            })?;

            Ok(Some(buffer))
        } else {
            Ok(None)
        }
    }

    /// Load the given preset (use serde and bincode).
    ///
    /// By default this does nothing.
    ///
    /// `[main-thread]`
    fn load_state(&mut self, preset: &PluginPreset) -> Result<(), String> {
        if let Some(state_ext) = self.instance.shared_host_data().state_ext {
            let mut reader = Cursor::new(&preset.bytes);

            state_ext.load(self.instance.main_thread_plugin_data(), &mut reader).map_err(|_| {
                format!(
                    "Plugin with ID {} returned error on call to clap_plugin_state.load()",
                    &*self.id()
                )
            })?;

            Ok(())
        } else {
            Err(format!(
                "Could not load state for clap plugin with ID {}: plugin does not implement the \"clap.state\" extension",
                &*self.id()
            ))
        }
    }

    /// Deactivate the plugin. When this is called it also means that the `PluginAudioThread`
    /// counterpart has/will be dropped.
    ///
    /// `[main-thread & active_state]`
    fn deactivate(&mut self) {
        log::trace!("clap plugin instance deactivate {}", self.id());
        // TODO: the Plugin's Audio Processor needs to be deactivated on the main thread
    }

    // --- Parameters ---------------------------------------------------------------------------------

    /// Called by the host on the main thread in response to a previous call to `host.request_callback()`.
    ///
    /// By default this does nothing.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn on_main_thread(&mut self) {
        log::trace!("clap plugin instance on_main_thread {}", self.id());

        self.instance.call_on_main_thread_callback();
    }

    /// An optional extension that describes the configuration of audio ports on this plugin instance.
    ///
    /// This will only be called while the plugin is inactive.
    ///
    /// The default configuration is a main stereo input port and a main stereo output port.
    ///
    /// [main-thread & !active_state]
    fn audio_ports_ext(&mut self) -> Result<PluginAudioPortsExt, String> {
        Ok(self.audio_ports_ext.clone())
    }

    /// Get the total number of parameters in this plugin.
    ///
    /// You may return 0 if this plugins has no parameters.
    ///
    /// By default this returns 0.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn num_params(&mut self) -> u32 {
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            params_ext.count(&self.instance)
        } else {
            0
        }
    }

    /// Get the info of the given parameter.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns an Err(()).
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_info(&mut self, param_index: usize) -> Result<ext::params::ParamInfo, ()> {
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            let mut data = MaybeUninit::uninit();

            let info =
                params_ext.get_info(&self.instance, param_index as u32, &mut data).ok_or(())?;

            Ok(ParamInfo {
                stable_id: ParamID(info.id()),
                flags: ParamInfoFlags::from_bits_truncate(info.flags()),
                // TODO: better handle UTF8 validation
                display_name: core::str::from_utf8(info.name()).map_err(|_| ())?.to_string(),
                module: core::str::from_utf8(info.module()).map_err(|_| ())?.to_string(),
                min_value: info.min_value(),
                max_value: info.max_value(),
                default_value: info.default_value(),
                cookie: info.cookie(),
            })
        } else {
            Err(())
        }
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
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            params_ext.get_value(&self.instance, param_id.0).ok_or(())
        } else {
            Err(())
        }
    }

    /// Formats the display text for the given parameter value.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `Err(())`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_value_to_text(&self, param_id: ParamID, value: f64) -> Result<String, ()> {
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            let mut char_buf = [MaybeUninit::uninit(); 256];

            let bytes = params_ext
                .value_to_text(&self.instance, param_id.0, value, &mut char_buf)
                .ok_or(())?;

            core::str::from_utf8(bytes).map_err(|_| ()).map(|s| s.to_string())
        } else {
            Err(())
        }
    }

    /// Converts the display text to a parameter value.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `Err(())`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_text_to_value(&self, param_id: ParamID, display: &str) -> Result<f64, ()> {
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            let c_string = CString::new(display).map_err(|_| ())?;

            params_ext.text_to_value(&self.instance, param_id.0, &c_string).ok_or(())
        } else {
            Err(())
        }
    }
}

pub(crate) struct ClapPluginAudioThread {
    audio_processor: PluginAudioProcessor<ClapHost>,
    process: ClapProcess,
}

impl PluginAudioThread for ClapPluginAudioThread {
    /// This will be called when the plugin should start processing after just activing/
    /// waking up from sleep.
    ///
    /// Return an error if the plugin failed to start processing. In this case the host will not
    /// call `process()` and return the plugin to sleep.
    ///
    /// By default this just returns `Ok(())`.
    ///
    /// `[audio-thread & active_state & !processing_state]`
    fn start_processing(&mut self) -> Result<(), ()> {
        log::trace!(
            "clap plugin instance start_processing {}",
            &*self.audio_processor.shared_host_data().id
        );

        self.audio_processor.start_processing().map_err(|_| ())
    }

    /// This will be called when the host puts the plugin to sleep.
    ///
    /// By default this trait method does nothing.
    ///
    /// `[audio-thread & active_state & processing_state]`
    fn stop_processing(&mut self) {
        log::trace!(
            "clap plugin instance stop_processing {}",
            &*self.audio_processor.shared_host_data().id
        );

        self.audio_processor.stop_processing().unwrap() // TODO: handle errors
    }

    /// Process audio and events.
    ///
    /// `[audio-thread & active_state & processing_state]`
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        buffers: &mut ProcBuffers,
        in_events: &EventBuffer,
        out_events: &mut EventBuffer,
    ) -> ProcessStatus {
        let (audio_in, mut audio_out) = self.process.update_buffers(buffers);

        let mut in_events = InputEvents::from_buffer(in_events);
        let mut out_events = OutputEvents::from_buffer(out_events);

        let res = {
            //#[cfg(debug_assertions)]
            // In debug mode, borrow all of the atomic ref cells to properly use the
            // safety checks, since external plugins just use the raw pointer to each
            // buffer.
            let (mut input_refs_f32, mut input_refs_f64, mut output_refs_f32, mut output_refs_f64) = {
                let mut input_refs_f32: SmallVec<[AtomicRef<'_, Vec<f32>>; 32]> = SmallVec::new();
                let mut input_refs_f64: SmallVec<[AtomicRef<'_, Vec<f64>>; 32]> = SmallVec::new();
                let mut output_refs_f32: SmallVec<[AtomicRefMut<'_, Vec<f32>>; 32]> =
                    SmallVec::new();
                let mut output_refs_f64: SmallVec<[AtomicRefMut<'_, Vec<f64>>; 32]> =
                    SmallVec::new();

                for in_port in buffers.audio_in.iter() {
                    match &in_port.raw_channels {
                        RawAudioChannelBuffers::F32(buffers) => {
                            for b in buffers.iter() {
                                input_refs_f32.push(b.buffer.data.borrow());
                            }
                        }
                        RawAudioChannelBuffers::F64(buffers) => {
                            for b in buffers.iter() {
                                input_refs_f64.push(b.buffer.data.borrow());
                            }
                        }
                    }
                }

                for out_port in buffers.audio_out.iter() {
                    match &out_port.raw_channels {
                        RawAudioChannelBuffers::F32(buffers) => {
                            for b in buffers.iter() {
                                output_refs_f32.push(b.buffer.data.borrow_mut());
                            }
                        }
                        RawAudioChannelBuffers::F64(buffers) => {
                            for b in buffers.iter() {
                                output_refs_f64.push(b.buffer.data.borrow_mut());
                            }
                        }
                    }
                }

                (input_refs_f32, input_refs_f64, output_refs_f32, output_refs_f64)
            };

            // TODO: handle transport & timer
            let res = self
                .audio_processor
                .as_started_mut()
                .expect("Audio Processor is not started")
                .process(
                    &audio_in,
                    &mut audio_out,
                    &mut in_events,
                    &mut out_events,
                    proc_info.steady_time,
                    Some(proc_info.frames),
                    None,
                );

            //#[cfg(debug_assertions)]
            {
                input_refs_f32.clear();
                input_refs_f64.clear();
                output_refs_f32.clear();
                output_refs_f64.clear();
            }

            res
        };

        use clack_host::process::ProcessStatus::*;
        match res {
            Err(_) => ProcessStatus::Error,
            Ok(Continue) => ProcessStatus::Continue,
            Ok(ContinueIfNotQuiet) => ProcessStatus::ContinueIfNotQuiet,
            Ok(Tail) => ProcessStatus::Tail,
            Ok(Sleep) => ProcessStatus::Sleep,
        }
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
    /// [active && !processing : audio-thread]
    #[allow(unused)]
    fn param_flush(&mut self, in_events: &EventBuffer, out_events: &mut EventBuffer) {
        self.audio_processor.audio_processor_host_data_mut().param_flush(in_events, out_events)
    }
}

pub(crate) struct ClapHost;

impl<'plugin> Host<'plugin> for ClapHost {
    type AudioProcessor = ClapHostAudioProcessor<'plugin>;
    type Shared = ClapHostShared<'plugin>;
    type MainThread = ClapHostMainThread<'plugin>;

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared) {
        builder
            .register::<Log>()
            .register::<ThreadCheck>()
            .register::<HostAudioPorts>()
            .register::<HostParams>();
    }
}

pub(crate) struct ClapHostMainThread<'plugin> {
    pub(crate) shared: &'plugin ClapHostShared<'plugin>,
    pub(crate) instance: Option<PluginMainThreadHandle<'plugin>>,
}

impl<'plugin> ClapHostMainThread<'plugin> {
    #[allow(unused)]
    fn param_flush(&mut self, in_events: &EventBuffer, out_events: &mut EventBuffer) {
        let params_ext = match self.shared.params_ext {
            None => return,
            Some(p) => p,
        };

        let clap_in_events = InputEvents::from_buffer(in_events);
        let mut clap_out_events = OutputEvents::from_buffer(out_events);

        params_ext.flush(self.instance.as_mut().unwrap(), &clap_in_events, &mut clap_out_events);
    }
}

impl<'plugin> HostMainThread<'plugin> for ClapHostMainThread<'plugin> {
    fn instantiated(&mut self, instance: PluginMainThreadHandle<'plugin>) {
        self.instance = Some(instance)
    }
}

pub(crate) struct ClapHostAudioProcessor<'plugin> {
    shared: &'plugin ClapHostShared<'plugin>,
    plugin: PluginAudioProcessorHandle<'plugin>,
}

impl<'plugin> HostAudioProcessor<'plugin> for ClapHostAudioProcessor<'plugin> {}

impl<'plugin> ClapHostAudioProcessor<'plugin> {
    fn param_flush(&mut self, in_events: &EventBuffer, out_events: &mut EventBuffer) {
        let params_ext = match self.shared.params_ext {
            None => return,
            Some(p) => p,
        };

        let clap_in_events = InputEvents::from_buffer(in_events);
        let mut clap_out_events = OutputEvents::from_buffer(out_events);

        params_ext.flush_active(&mut self.plugin, &clap_in_events, &mut clap_out_events);
    }
}

pub(crate) struct ClapHostShared<'plugin> {
    pub id: Shared<String>,

    params_ext: Option<&'plugin PluginParams>,
    state_ext: Option<&'plugin PluginState>,

    host_request: HostRequest,
    plugin_log_name: Shared<String>,
    thread_ids: SharedThreadIDs,
}

impl<'plugin> ClapHostShared<'plugin> {
    pub(crate) fn new(
        id: Shared<String>,
        host_request: HostRequest,
        thread_ids: SharedThreadIDs,
        plugin_id: PluginInstanceID,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        let plugin_log_name = Shared::new(coll_handle, format!("{:?}", &plugin_id));

        Self { id, host_request, params_ext: None, state_ext: None, plugin_log_name, thread_ids }
    }
}

impl<'a> HostShared<'a> for ClapHostShared<'a> {
    fn instantiated(&mut self, instance: PluginSharedHandle<'a>) {
        self.params_ext = instance.get_extension();
        self.state_ext = instance.get_extension();
    }

    fn request_restart(&self) {
        self.host_request.request_restart()
    }

    fn request_process(&self) {
        self.host_request.request_process()
    }

    fn request_callback(&self) {
        self.host_request.request_callback()
    }
}

// TODO: Make sure that the log and print methods don't allocate on the current thread.
// If they do, then we need to come up with a realtime-safe way to print to the terminal.
impl<'a> HostLog for ClapHostShared<'a> {
    fn log(&self, severity: LogSeverity, message: &str) {
        let level = match severity {
            LogSeverity::Debug => log::Level::Debug,
            LogSeverity::Info => log::Level::Info,
            LogSeverity::Warning => log::Level::Warn,
            LogSeverity::Error => log::Level::Error,
            LogSeverity::Fatal => log::Level::Error,
            LogSeverity::HostMisbehaving => log::Level::Error,
            LogSeverity::PluginMisbehaving => log::Level::Error,
        };

        log::log!(level, "{}", self.plugin_log_name.as_str());
        log::log!(level, "{}", message);
    }
}

impl<'a> ThreadCheckImplementation for ClapHostShared<'a> {
    fn is_main_thread(&self) -> bool {
        if let Some(thread_id) = self.thread_ids.external_main_thread_id() {
            std::thread::current().id() == thread_id
        } else {
            log::error!("external_main_thread_id is None");
            false
        }
    }

    fn is_audio_thread(&self) -> bool {
        if let Some(thread_id) = self.thread_ids.external_audio_thread_id() {
            std::thread::current().id() == thread_id
        } else {
            log::error!("external_audio_thread_id is None");
            false
        }
    }
}

impl<'a> HostAudioPortsImplementation for ClapHostMainThread<'a> {
    fn is_rescan_flag_supported(&self, mut flag: RescanType) -> bool {
        if !self.shared.thread_ids.is_external_main_thread() {
            log::warn!("Plugin called clap_host_audio_ports->is_rescan_flag_supported() not in the main thread");
            return false;
        }

        let supported = RescanType::FLAGS
            | RescanType::CHANNEL_COUNT
            | RescanType::PORT_TYPE
            | RescanType::IN_PLACE_PAIR
            | RescanType::LIST;
        // | RescanType::NAMES // TODO: support this

        flag.remove(supported);
        flag.is_empty()
    }

    fn rescan(&mut self, mut flags: RescanType) {
        if !self.shared.thread_ids.is_external_main_thread() {
            log::warn!("Plugin called clap_host_audio_ports->rescan() not in the main thread");
            return;
        }

        if flags.contains(RescanType::NAMES) {
            // TODO: support this
            log::warn!("clap plugin {:?} set CLAP_AUDIO_PORTS_RESCAN_NAMES flag in call to clap_host_audio_ports->rescan()", &*self.shared.plugin_log_name);

            flags.remove(RescanType::NAMES);
        }

        if !flags.is_empty() {
            self.shared.host_request.request_restart();
        }
    }
}

impl<'a> HostParamsImplementation for ClapHostShared<'a> {
    fn request_flush(&self) {
        self.host_request.params.request_flush();
    }
}

impl<'a> HostParamsImplementationMainThread for ClapHostMainThread<'a> {
    fn rescan(&mut self, flags: ClackParamRescanFlags) {
        if !self.shared.thread_ids.is_external_main_thread() {
            log::warn!("Plugin called clap_host_params->rescan() not in the main thread");
            return;
        }

        let flags = ParamRescanFlags::from_bits_truncate(flags.bits());

        self.shared.host_request.params.rescan(flags);
    }

    fn clear(&mut self, param_id: u32, flags: ClackParamClearFlags) {
        if !self.shared.thread_ids.is_external_main_thread() {
            log::warn!("Plugin called clap_host_params->clear() not in the main thread");
            return;
        }

        let flags = ParamClearFlags::from_bits_truncate(flags.bits());

        self.shared.host_request.params.clear(ParamID(param_id), flags);
    }
}
