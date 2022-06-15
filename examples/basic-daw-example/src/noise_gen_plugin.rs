use dropseed::plugin::ext::audio_ports::PluginAudioPortsExt;
use dropseed::plugin::ext::params::ParamInfo;
use dropseed::plugin::{PluginAudioThread, PluginDescriptor, PluginFactory, PluginMainThread};
use dropseed::{
    EventQueue, ParamID, ParamInfoFlags, ProcBuffers, ProcEventRef, ProcInfo, ProcessStatus,
};
use meadowlark_core_types::SampleRate;
use std::error::Error;

pub struct NoiseGenPluginFactory {}

impl PluginFactory for NoiseGenPluginFactory {
    fn description(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: "app.meadowlark.noise-generator".into(),
            version: "0.0.1alpha".into(),
            name: "Noise Generator".into(),
            vendor: "Meadowlark".into(),
            description: "Generate noise of different colors".into(),
            url: "https://github.com/MeadowlarkDAW/meadowlark-plugins".into(),
            manual_url: "".into(),
            support_url: "".into(),
        }
    }

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
        _host: dropseed::HostRequest,
        _plugin_id: dropseed::PluginInstanceID,
        _coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginMainThread>, Box<dyn std::error::Error + Send>> {
        let (audio_thread, handle) =
            noise_generator_dsp::NoiseGeneratorDSP::new(Default::default(), SampleRate::default());

        Ok(Box::new(NoiseGenPluginMainThread { handle, pending_audio_thread: Some(audio_thread) }))
    }
}

pub struct NoiseGenPluginMainThread {
    handle: noise_generator_dsp::NoiseGeneratorHandle,
    pending_audio_thread: Option<noise_generator_dsp::NoiseGeneratorDSP>,
}

impl PluginMainThread for NoiseGenPluginMainThread {
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
    fn init(
        &mut self,
        _preset: (),
        coll_handle: &basedrop::Handle,
    ) -> Result<(), Box<dyn Error + Send>> {
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
        _min_frames: u32,
        _max_frames: u32,
        _coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginAudioThread>, Box<dyn Error + Send>> {
        let mut audio_thread = self.pending_audio_thread.take().unwrap();

        audio_thread.set_sample_rate(sample_rate);

        Ok(Box::new(NoiseGenPluginAudioThread { dsp: audio_thread }))
    }

    /// Deactivate the plugin. When this is called it also means that the `PluginAudioThread`
    /// counterpart has/will be dropped.
    ///
    /// `[main-thread & active_state]`
    fn deactivate(&mut self) {
        let preset = self.handle.get_preset();

        let (audio_thread, handle) =
            noise_generator_dsp::NoiseGeneratorDSP::new(preset, SampleRate::default());

        self.handle = handle;
        self.pending_audio_thread = Some(audio_thread);
    }

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
    fn audio_ports_ext(&mut self) -> Result<PluginAudioPortsExt, Box<dyn Error + Send>> {
        Ok(PluginAudioPortsExt::stereo_out())
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
        3
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
    fn param_info(&mut self, param_index: usize) -> Result<ParamInfo, ()> {
        match param_index {
            0 => {
                Ok(ParamInfo::new(
                    ParamID::new(0),                              // stable ID
                    ParamInfoFlags::default_enum(),               // flags
                    "Color".into(),                               // display name
                    "".into(),                                    // module
                    self.handle.color_i32.min() as f64,           // min value
                    self.handle.color_i32.max() as f64,           // max value
                    self.handle.color_i32.default_value() as f64, // default value
                ))
            }
            1 => {
                Ok(ParamInfo::new(
                    ParamID::new(1),                              // stable ID
                    ParamInfoFlags::default_float(),              // flags
                    "Noise Gain".into(),                          // display name
                    "".into(),                                    // module
                    0.0,                                          // min value
                    1.0,                                          // max value
                    self.handle.gain.default_normalized() as f64, // default value
                ))
            }
            2 => {
                Ok(ParamInfo::new(
                    ParamID::new(2), // stable ID
                    ParamInfoFlags::default_enum()
                        | ParamInfoFlags::REQUIRES_PROCESS
                        | ParamInfoFlags::IS_BYPASS, // flags
                    "Bypass".into(), // display name
                    "".into(),       // module
                    0.0,             // min value
                    1.0,             // max value
                    self.handle.bypassed.default_normalized() as f64, // default value
                ))
            }
            _ => Err(()),
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
        match param_id.as_u32() {
            0 => Ok(self.handle.color().as_i32() as f64),
            1 => Ok(self.handle.gain.normalized() as f64),
            2 => Ok(self.handle.bypassed.normalized() as f64),
            _ => Err(()),
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
        match param_id.as_u32() {
            0 => {
                let s = match self.handle.color() {
                    noise_generator_dsp::NoiseColor::White => "White",
                    noise_generator_dsp::NoiseColor::Pink => "Pink",
                    noise_generator_dsp::NoiseColor::Brown => "Brown",
                };

                Ok(s.into())
            }
            1 => Ok(format!("{:.2}dB", self.handle.gain.value())),
            2 => {
                if self.handle.bypassed.value() {
                    Ok("true".into())
                } else {
                    Ok("false".into())
                }
            }
            _ => Err(()),
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
        // todo
        Err(())
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
    #[allow(unused)]
    fn param_flush(&mut self, in_events: &EventQueue, out_events: &mut EventQueue) {
        for event in in_events.iter() {
            if let Ok(event) = event.get() {
                match event {
                    ProcEventRef::ParamValue(e) => match e.param_id().as_u32() {
                        0 => self.handle.color_i32.set_value(e.value().round() as i32),
                        1 => self.handle.gain.set_normalized(e.value() as f32),
                        2 => self.handle.bypassed.set_normalized(e.value() as f32),
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }
}

pub struct NoiseGenPluginAudioThread {
    dsp: noise_generator_dsp::NoiseGeneratorDSP,
}

impl NoiseGenPluginAudioThread {
    fn flush_params(&mut self, in_events: &EventQueue) {
        for event in in_events.iter() {
            if let Ok(event) = event.get() {
                match event {
                    ProcEventRef::ParamValue(e) => match e.param_id().as_u32() {
                        0 => self.dsp.color.set_value(e.value().round() as i32),
                        1 => self.dsp.gain.set_normalized(e.value() as f32),
                        2 => self.dsp.bypassed.set_normalized(e.value() as f32),
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }
}

impl PluginAudioThread for NoiseGenPluginAudioThread {
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
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        buffers: &mut ProcBuffers,
        in_events: &EventQueue,
        _out_events: &mut EventQueue,
    ) -> ProcessStatus {
        self.flush_params(in_events);

        let (mut out_l, mut out_r) = unsafe { buffers.audio_out[0].stereo_f32_unchecked_mut() };

        self.dsp.process_stereo(proc_info.frames, &mut out_l, &mut out_r);

        if self.dsp.can_sleep() {
            ProcessStatus::Sleep
        } else {
            ProcessStatus::Continue
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
    fn param_flush(&mut self, in_events: &EventQueue, _out_events: &mut EventQueue) {
        self.flush_params(in_events);
    }
}
