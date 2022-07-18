use super::*;

use atomic_refcell::{AtomicRef, AtomicRefMut};
use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{GuiApiType, GuiError};
use clack_host::events::io::{InputEvents, OutputEvents};
use clack_host::instance::processor::PluginAudioProcessor;
use clack_host::instance::{PluginAudioConfiguration, PluginInstance};
use dropseed_core::plugin::buffer::RawAudioChannelBuffers;
use dropseed_core::plugin::ext::audio_ports::{
    AudioPortInfo, MainPortsLayout, PluginAudioPortsExt,
};
use dropseed_core::plugin::ext::params::{ParamID, ParamInfo, ParamInfoFlags};
use dropseed_core::plugin::{
    buffer::EventBuffer, ext, PluginActivatedInfo, PluginAudioThread, PluginMainThread,
    PluginPreset, ProcBuffers, ProcInfo, ProcessStatus,
};
use meadowlark_core_types::time::SampleRate;
use smallvec::SmallVec;
use std::ffi::CString;
use std::io::Cursor;
use std::mem::MaybeUninit;

use super::process::ClapProcess;

impl ClapPluginMainThread {
    pub(crate) fn new(instance: PluginInstance<ClapHost>) -> Result<Self, String> {
        Ok(Self { audio_ports_ext: Self::parse_audio_ports_extension(&instance)?, instance })
    }

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
        let audio_processor = match self.instance.activate(
            |plugin, shared, _| ClapHostAudioProcessor::new(plugin, shared),
            configuration,
        ) {
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

    /*fn param_flush(&mut self, in_events: &EventQueue, out_events: &mut EventQueue) {
        self.instance.main_thread_host_data_mut().param_flush(in_events, out_events)
    }*/

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

    fn deactivate(&mut self) {
        log::trace!("clap plugin instance deactivate {}", self.id());
        self.instance
            .try_deactivate()
            .expect("Called deactivate() before the plugin's AudioProcessor was dropped");
    }

    // --- Parameters ---------------------------------------------------------------------------------

    fn on_main_thread(&mut self) {
        log::trace!("clap plugin instance on_main_thread {}", self.id());

        self.instance.call_on_main_thread_callback();
    }

    fn audio_ports_ext(&mut self) -> Result<PluginAudioPortsExt, String> {
        Ok(self.audio_ports_ext.clone())
    }

    fn num_params(&mut self) -> u32 {
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            params_ext.count(&self.instance)
        } else {
            0
        }
    }

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
                _cookie: info.cookie(),
            })
        } else {
            Err(())
        }
    }

    fn param_value(&self, param_id: ParamID) -> Result<f64, ()> {
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            params_ext.get_value(&self.instance, param_id.0).ok_or(())
        } else {
            Err(())
        }
    }

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

    fn param_text_to_value(&self, param_id: ParamID, display: &str) -> Result<f64, ()> {
        if let Some(params_ext) = self.instance.shared_host_data().params_ext {
            let c_string = CString::new(display).map_err(|_| ())?;

            params_ext.text_to_value(&self.instance, param_id.0, &c_string).ok_or(())
        } else {
            Err(())
        }
    }

    fn supports_gui(&self) -> bool {
        if let (Some(gui), Some(api)) =
            (self.instance.shared_host_data().gui_ext, GuiApiType::default_for_current_platform())
        {
            gui.is_api_supported(&self.instance.main_thread_plugin_data(), api, true)
        } else {
            false
        }
    }

    fn open_gui(&mut self, suggested_title: Option<&str>) -> Result<(), GuiError> {
        let host = self.instance.main_thread_host_data_mut();
        let api_type = GuiApiType::default_for_current_platform().unwrap();

        let gui_ext = host.shared.gui_ext.ok_or(GuiError::CreateError)?;
        let instance = host.instance.as_mut().unwrap(); // TODO: unwrap

        gui_ext.create(instance, api_type, true)?;
        if let Some(title) = suggested_title {
            let title = CString::new(title.to_string()).unwrap(); // TODO: unwrap
            gui_ext.suggest_title(instance, &title);
        }
        gui_ext.show(instance)?;

        host.gui_visible = true;

        Ok(())
    }

    fn close_gui(&mut self) {
        let host = self.instance.main_thread_host_data_mut();
        let gui_ext = host.shared.gui_ext.ok_or(GuiError::CreateError).unwrap();
        let instance = host.instance.as_mut().unwrap(); // TODO: unwrap

        // TODO: unwrap
        gui_ext.hide(instance).unwrap();
        gui_ext.destroy(instance);
    }
}

struct ClapPluginAudioThread {
    audio_processor: PluginAudioProcessor<ClapHost>,
    process: ClapProcess,
}

impl PluginAudioThread for ClapPluginAudioThread {
    fn start_processing(&mut self) -> Result<(), ()> {
        log::trace!(
            "clap plugin instance start_processing {}",
            &*self.audio_processor.shared_host_data().id
        );

        self.audio_processor.start_processing().map_err(|_| ())
    }

    fn stop_processing(&mut self) {
        log::trace!(
            "clap plugin instance stop_processing {}",
            &*self.audio_processor.shared_host_data().id
        );

        self.audio_processor.stop_processing().unwrap() // TODO: handle errors
    }

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
                    match &in_port._raw_channels {
                        RawAudioChannelBuffers::F32(buffers) => {
                            for b in buffers.iter() {
                                input_refs_f32.push(b.borrow());
                            }
                        }
                        RawAudioChannelBuffers::F64(buffers) => {
                            for b in buffers.iter() {
                                input_refs_f64.push(b.borrow());
                            }
                        }
                    }
                }

                for out_port in buffers.audio_out.iter() {
                    match &out_port._raw_channels {
                        RawAudioChannelBuffers::F32(buffers) => {
                            for b in buffers.iter() {
                                output_refs_f32.push(b.borrow_mut());
                            }
                        }
                        RawAudioChannelBuffers::F64(buffers) => {
                            for b in buffers.iter() {
                                output_refs_f64.push(b.borrow_mut());
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

    #[allow(unused)]
    fn param_flush(&mut self, in_events: &EventBuffer, out_events: &mut EventBuffer) {
        self.audio_processor.audio_processor_host_data_mut().param_flush(in_events, out_events)
    }
}
