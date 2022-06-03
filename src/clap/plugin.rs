use basedrop::Shared;
use clap_sys::string_sizes::CLAP_NAME_SIZE;
use rusty_daw_core::SampleRate;
use std::error::Error;
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[cfg(debug_assertions)]
use maybe_atomic_refcell::{MaybeAtomicRef, MaybeAtomicRefMut};
#[cfg(debug_assertions)]
use smallvec::SmallVec;

use clap_sys::entry::clap_plugin_entry as RawClapEntry;
use clap_sys::plugin::clap_plugin as RawClapPlugin;
use clap_sys::plugin::clap_plugin_descriptor as RawClapPluginDescriptor;
use clap_sys::plugin_factory::clap_plugin_factory as RawClapPluginFactory;
use clap_sys::plugin_factory::CLAP_PLUGIN_FACTORY_ID;
use clap_sys::string_sizes::CLAP_PATH_SIZE;
use clap_sys::version::clap_version as ClapVersion;

use super::c_char_helpers::c_char_ptr_to_maybe_str;
use super::events::{ClapInputEvents, ClapOutputEvents};
use super::host_request::ClapHostRequest;
use super::process::ClapProcess;
use crate::clap::c_char_helpers::c_char_buf_to_str;
use crate::host_request::HostRequest;
use crate::plugin::audio_buffer::RawAudioChannelBuffers;
use crate::plugin::process_info::{ProcBuffers, ProcInfo, ProcessStatus};
use crate::plugin::{ext, PluginAudioThread, PluginDescriptor, PluginFactory, PluginMainThread};
use crate::thread_id::SharedThreadIDs;
use crate::MainPortsLayout;
use crate::PluginAudioPortsExt;
use crate::{AudioPortInfo, EventQueue, PluginInstanceID};

use super::params::ClapPluginParams;

struct SharedClapLib {
    // We hold on to this to make sure the host callback stays alive for as long as a
    // reference to this struct exists.
    _lib: libloading::Library,

    raw_entry: *const RawClapEntry,
    raw_factory: *const RawClapPluginFactory,
}

// This is safe because we only ever dereference the contained pointers in
// the main thread.
unsafe impl Send for SharedClapLib {}
// This is safe because we only ever dereference the contained pointers in
// the main thread.
unsafe impl Sync for SharedClapLib {}

impl Drop for SharedClapLib {
    fn drop(&mut self) {
        if !self.raw_entry.is_null() {
            log::trace!("dropping clap library");
            // Safe because the constructor made sure that this is a valid pointer.
            unsafe {
                ((&*self.raw_entry).deinit);
            }
        }
    }
}

pub(crate) fn entry_init(
    plugin_path: &PathBuf,
    thread_ids: SharedThreadIDs,
    coll_handle: &basedrop::Handle,
) -> Result<Vec<ClapPluginFactory>, Box<dyn Error>> {
    log::trace!("clap entry init at path {:?}", plugin_path);

    // "Safe" because we acknowledge the risk of running foreign code in external
    // plugin libraries.
    //
    // TODO: We should use sandboxing to make this even more safe and to
    // gaurd against plugin crashes from bringing down the whole application.
    let lib = unsafe { libloading::Library::new(plugin_path)? };

    // Safe because this is the correct type for this symbol.
    let entry: libloading::Symbol<*const RawClapEntry> = unsafe { lib.get(b"clap_entry\0")? };

    // Safe because we are storing the library in this struct itself, ensuring
    // that the lifetime of this doesn't outlive the lifetime of the library.
    let raw_entry = unsafe { *entry.into_raw() };

    if raw_entry.is_null() {
        return Err(
            format!("Plugin from path {:?} has a null pointer for raw_entry", plugin_path).into()
        );
    }

    // Safe because we checked that this was not null.
    let clap_version = unsafe { (*raw_entry).clap_version };

    if !clap_sys::version::clap_version_is_compatible(clap_version) {
        return Err(format!(
            "Plugin from path {:?} has an incompatible clap version {}.{}.{}. Host version is {}.{}.{}",
            plugin_path, clap_version.major, clap_version.minor, clap_version.revision,
            clap_sys::version::CLAP_VERSION_MAJOR, clap_sys::version::CLAP_VERSION_MINOR, clap_sys::version::CLAP_VERSION_REVISION,
        )
        .into());
    }

    let plugin_path_parent_folder = plugin_path
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or(format!("Plugin path {:?} cannot be in the root path", plugin_path))?;

    let c_path = CString::new(plugin_path_parent_folder.to_string_lossy().to_string())?;

    // Safe because this is the correct format of this function as described in the
    // CLAP spec.
    let init_res = unsafe { ((&*raw_entry).init)(c_path.as_ptr()) };

    if !init_res {
        return Err(format!(
            "Plugin from path {:?} returned false while calling clap_plugin_entry.init()",
            plugin_path
        )
        .into());
    }

    // Safe because this is the correct format of this function as described in the
    // CLAP spec.
    let raw_factory = unsafe { ((&*raw_entry).get_factory)(CLAP_PLUGIN_FACTORY_ID) }
        as *const RawClapPluginFactory;

    if raw_factory.is_null() {
        return Err(format!(
            "Plugin from path {:?} returned null while calling clap_plugin_entry.get_factory()",
            plugin_path
        )
        .into());
    }

    let shared_lib = Shared::new(coll_handle, SharedClapLib { _lib: lib, raw_entry, raw_factory });

    // Safe because this is the correct format of this function as described in the
    // CLAP spec.
    let num_plugins = unsafe { ((&*raw_factory).get_plugin_count)(raw_factory) };

    if num_plugins == 0 {
        return Err(format!(
            "Plugin from path {:?} returned 0 while calling clap_plugin_factory.get_plugin_count()",
            plugin_path
        )
        .into());
    }

    let mut factories: Vec<ClapPluginFactory> = Vec::with_capacity(num_plugins as usize);

    for i in 0..num_plugins {
        // Safe because this is the correct format of this function as described in the
        // CLAP spec.
        let raw_descriptor = unsafe { ((&*raw_factory).get_plugin_descriptor)(raw_factory, i) };

        log::trace!("clap plugin instance parse descriptor {:?}", plugin_path);

        let descriptor = parse_clap_plugin_descriptor(raw_descriptor, plugin_path, i)?;

        let id = Shared::new(coll_handle, descriptor.id.clone());

        let c_id = CString::new(descriptor.id.clone()).unwrap();

        factories.push(ClapPluginFactory {
            shared_lib: Shared::clone(&shared_lib),
            descriptor,
            id,
            c_id,
            clap_version,
            thread_ids: thread_ids.clone(),
        });
    }

    Ok(factories)
}

pub(crate) struct ClapPluginFactory {
    pub clap_version: ClapVersion,

    shared_lib: Shared<SharedClapLib>,
    descriptor: PluginDescriptor,
    id: Shared<String>,
    c_id: CString,

    thread_ids: SharedThreadIDs,
}

impl PluginFactory for ClapPluginFactory {
    fn description(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    /// Create a new instance of this plugin.
    ///
    /// **NOTE**: The plugin is **NOT** allowed to use the host callbacks in this method.
    ///
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// `[main-thread]`
    fn new(
        &mut self,
        host_request: HostRequest,
        plugin_id: PluginInstanceID,
        main_thread_coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginMainThread>, Box<dyn Error>> {
        log::trace!("clap plugin factory new {}", &*self.id);

        let mut clap_host_request = ClapHostRequest::new(
            host_request.clone(),
            self.thread_ids.clone(),
            plugin_id,
            main_thread_coll_handle,
        );

        let raw_plugin = unsafe {
            ((&*self.shared_lib.raw_factory).create_plugin)(
                self.shared_lib.raw_factory,
                clap_host_request.get_raw(),
                self.c_id.as_ptr(),
            )
        };

        clap_host_request.plugin_created();

        if raw_plugin.is_null() {
            return Err(format!(
                "Plugin with ID {} returned null while calling clap_plugin_factory.create_plugin()",
                &self.descriptor.id
            )
            .into());
        }

        let init_res = unsafe { ((&*raw_plugin).init)(raw_plugin) };
        if !init_res {
            unsafe {
                ((&*raw_plugin).destroy)(raw_plugin);
            }

            return Err(format!(
                "Plugin with ID {} returned false while calling clap_plug.init()",
                &self.descriptor.id
            )
            .into());
        }

        let (params_ext, has_params_ext) = ClapPluginParams::new(raw_plugin);

        let in_out_events = if has_params_ext {
            Some((ClapInputEvents::new(), ClapOutputEvents::new()))
        } else {
            None
        };

        let shared_plugin = Shared::new(
            main_thread_coll_handle,
            SharedClapPluginInstance {
                raw_plugin,
                activated: Arc::new(AtomicBool::new(false)),
                id: Shared::clone(&self.id),
                params_ext,
                _host_request: clap_host_request,
                _shared_lib: Shared::clone(&self.shared_lib),
            },
        );

        Ok(Box::new(ClapPluginMainThread { shared_plugin, audio_ports_ext: None, in_out_events }))
    }
}

pub(crate) struct SharedClapPluginInstance {
    pub id: Shared<String>,
    activated: Arc<AtomicBool>,

    raw_plugin: *const RawClapPlugin,

    params_ext: ClapPluginParams,

    // We hold on to these to make sure these stay alive for as long as a
    // reference to this struct exists.
    _host_request: ClapHostRequest,
    _shared_lib: Shared<SharedClapLib>,
}

impl Drop for SharedClapPluginInstance {
    fn drop(&mut self) {
        if !self.raw_plugin.is_null() {
            // Safe because the constructor ensures that this is a valid pointer.
            unsafe {
                if self.activated.load(Ordering::Relaxed) {
                    log::trace!("clap plugin instance deactivate {}", &*self.id);
                    self.activated.store(false, Ordering::Relaxed);
                    ((&*self.raw_plugin).deactivate)(self.raw_plugin);
                }
                if !self.raw_plugin.is_null() {
                    log::trace!("clap plugin instance drop {}", &*self.id);
                    ((&*self.raw_plugin).destroy)(self.raw_plugin);
                }
            }
        }

        self.raw_plugin = std::ptr::null();
    }
}

// This is safe because we are upholding the threading model as defined in the CLAP spec.
unsafe impl Send for SharedClapPluginInstance {}
// This is safe because we are upholding the threading model as defined in the CLAP spec.
unsafe impl Sync for SharedClapPluginInstance {}

pub(crate) struct ClapPluginMainThread {
    shared_plugin: Shared<SharedClapPluginInstance>,
    audio_ports_ext: Option<PluginAudioPortsExt>,

    in_out_events: Option<(ClapInputEvents, ClapOutputEvents)>,
}

impl ClapPluginMainThread {
    fn parse_audio_ports_extension(
        &self,
    ) -> Result<ext::audio_ports::PluginAudioPortsExt, Box<dyn Error>> {
        log::trace!("clap plugin instance parse audio ports extension {}", &*self.shared_plugin.id);

        if self.shared_plugin.activated.load(Ordering::Relaxed) {
            return Err("Cannot get audio ports extension while plugin is active".into());
        }

        use clap_sys::ext::audio_ports::clap_audio_port_info as RawAudioPortInfo;
        use clap_sys::ext::audio_ports::clap_plugin_audio_ports as RawAudioPorts;
        use clap_sys::ext::audio_ports::{CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS};

        let raw_ext = unsafe {
            ((&*self.shared_plugin.raw_plugin).get_extension)(
                self.shared_plugin.raw_plugin,
                CLAP_EXT_AUDIO_PORTS,
            )
        };

        if raw_ext.is_null() {
            return Ok(ext::audio_ports::PluginAudioPortsExt::empty());
        }

        let raw_audio_ports = raw_ext as *const RawAudioPorts;

        let num_in_ports =
            unsafe { ((*raw_audio_ports).count)(self.shared_plugin.raw_plugin, true) };
        let num_out_ports =
            unsafe { ((*raw_audio_ports).count)(self.shared_plugin.raw_plugin, false) };

        let mut raw_in_info: Vec<RawAudioPortInfo> = Vec::with_capacity(num_in_ports as usize);
        let mut raw_out_info: Vec<RawAudioPortInfo> = Vec::with_capacity(num_out_ports as usize);

        for i in 0..num_in_ports {
            // TODO: unitilialized?
            let mut raw_audio_port_info = RawAudioPortInfo {
                id: 0,
                name: [0; CLAP_NAME_SIZE],
                flags: 0,
                channel_count: 0,
                port_type: std::ptr::null(),
                in_place_pair: 0,
            };

            let res = unsafe {
                ((*raw_audio_ports).get)(
                    self.shared_plugin.raw_plugin,
                    i,
                    true,
                    &mut raw_audio_port_info,
                )
            };
            if !res {
                return Err(format!("Failed to get audio port extension from clap plugin instance {}: plugin returned false on call to clap_plugin_audio_ports.get(plugin, {}, true, info)", &*self.shared_plugin.id, i).into());
            }

            if raw_audio_port_info.channel_count == 0 {
                return Err(format!("Failed to get audio port extension from clap plugin instance {}: the input port at index {} has 0 channels", &*self.shared_plugin.id, i).into());
            }

            raw_in_info.push(raw_audio_port_info);
        }

        for i in 0..num_out_ports {
            // TODO: unitilialized?
            let mut raw_audio_port_info = RawAudioPortInfo {
                id: 0,
                name: [0; CLAP_NAME_SIZE],
                flags: 0,
                channel_count: 0,
                port_type: std::ptr::null(),
                in_place_pair: 0,
            };

            let res = unsafe {
                ((*raw_audio_ports).get)(
                    self.shared_plugin.raw_plugin,
                    i,
                    false,
                    &mut raw_audio_port_info,
                )
            };
            if !res {
                return Err(format!("Failed to get audio port extension from clap plugin instance {}: plugin returned false on call to clap_plugin_audio_ports.get(plugin, {}, false, info)", &*self.shared_plugin.id, i).into());
            }

            if raw_audio_port_info.channel_count == 0 {
                return Err(format!("Failed to get audio port extension from clap plugin instance {}: the output port at index {} has 0 channels", &*self.shared_plugin.id, i).into());
            }

            raw_out_info.push(raw_audio_port_info);
        }

        let has_main_in_port = if !raw_in_info.is_empty() {
            raw_in_info[0].flags & CLAP_AUDIO_PORT_IS_MAIN == 1
        } else {
            false
        };
        let has_main_out_port = if !raw_out_info.is_empty() {
            raw_out_info[0].flags & CLAP_AUDIO_PORT_IS_MAIN == 1
        } else {
            false
        };

        let main_ports_layout = if has_main_in_port && has_main_out_port {
            MainPortsLayout::InOut
        } else if has_main_in_port {
            MainPortsLayout::InOnly
        } else if has_main_out_port {
            MainPortsLayout::OutOnly
        } else {
            MainPortsLayout::NoMainPorts
        };

        let inputs: Vec<AudioPortInfo> = raw_in_info.iter().map(|raw_info| {
            let port_type = match c_char_ptr_to_maybe_str(raw_info.port_type, CLAP_NAME_SIZE) {
                None => None,
                Some(Err(_)) => {
                    log::warn!("Failed to get clap_audio_port_info.port_type from plugin instance {}: not null-terminated before {} bytes", &*self.shared_plugin.id, CLAP_NAME_SIZE);
                    None
                }
                Some(Ok(s)) => {
                    if s.len() == 0 {
                        None
                    } else {
                        Some(s.to_string())
                    }
                }
            };

            let display_name = match c_char_buf_to_str(&raw_info.name) {
                Ok(n) => {
                    if n.len() == 0 {
                        None
                    } else {
                        Some(n.to_string())
                    }
                }
                Err(_) => {
                    log::warn!("Failed to get clap_audio_port_info.name from plugin instance {}", &*self.shared_plugin.id);
                    None
                }
            };

            AudioPortInfo {
                stable_id: raw_info.id,
                channels: raw_info.channel_count as usize,
                port_type,
                display_name,
            }
        }).collect();

        let outputs: Vec<AudioPortInfo> = raw_out_info.iter().map(|raw_info| {
            let port_type = match c_char_ptr_to_maybe_str(raw_info.port_type, CLAP_NAME_SIZE) {
                None => None,
                Some(Err(_)) => {
                    log::warn!("Failed to get clap_audio_port_info.port_type from plugin instance {}: not null-terminated before {} bytes", &*self.shared_plugin.id, CLAP_NAME_SIZE);
                    None
                }
                Some(Ok(s)) => {
                    if s.len() == 0 {
                        None
                    } else {
                        Some(s.to_string())
                    }
                }
            };

            let display_name = match c_char_buf_to_str(&raw_info.name) {
                Ok(n) => {
                    if n.len() == 0 {
                        None
                    } else {
                        Some(n.to_string())
                    }
                }
                Err(_) => {
                    log::warn!("Failed to get clap_audio_port_info.name from plugin instance {}", &*self.shared_plugin.id);
                    None
                }
            };

            AudioPortInfo {
                stable_id: raw_info.id,
                channels: raw_info.channel_count as usize,
                port_type,
                display_name,
            }
        }).collect();

        Ok(PluginAudioPortsExt { inputs, outputs, main_ports_layout })
    }
}

impl PluginMainThread for ClapPluginMainThread {
    /// This is called after creating a plugin instance and once it's safe for the plugin to
    /// use the host callback methods.
    ///
    /// `[main-thread & !active_state]`
    #[allow(unused)]
    fn init(&mut self, _preset: (), coll_handle: &basedrop::Handle) -> Result<(), Box<dyn Error>> {
        log::trace!("clap plugin instance init {}", &*self.shared_plugin.id);

        let res =
            unsafe { ((&*self.shared_plugin.raw_plugin).init)(self.shared_plugin.raw_plugin) };

        if res {
            Ok(())
        } else {
            Err(format!(
                "Plugin with ID {} returned false on call to clap_plugin.init()",
                &*self.shared_plugin.id
            )
            .into())
        }
    }

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
    ) -> Result<Box<dyn PluginAudioThread>, Box<dyn Error>> {
        if self.shared_plugin.activated.load(Ordering::Relaxed) {
            return Err("Plugin already activated".into());
        }

        log::trace!("clap plugin instance activate {}", &*self.shared_plugin.id);

        let res = unsafe {
            ((&*self.shared_plugin.raw_plugin).activate)(
                self.shared_plugin.raw_plugin,
                sample_rate.0,
                min_frames as u32,
                max_frames as u32,
            )
        };

        if res {
            self.shared_plugin.activated.store(true, Ordering::Relaxed);
            Ok(Box::new(ClapPluginAudioThread {
                shared_plugin: Shared::clone(&self.shared_plugin),
                // The will always call `audio_ports_extension()` at-least once before
                // activating a plugin.
                process: ClapProcess::new(self.audio_ports_ext.as_ref().unwrap()),
                // Make sure that the plugin will always initialize its list of buffers
                // before the first call to `process()`.
                task_version: std::u64::MAX,
            }))
        } else {
            return Err(format!(
                "Plugin with ID {} returned false on call to clap_plugin.activate()",
                &*self.shared_plugin.id
            )
            .into());
        }
    }

    /// Deactivate the plugin. When this is called it also means that the `PluginAudioThread`
    /// counterpart has/will be dropped.
    ///
    /// `[main-thread & active_state]`
    fn deactivate(&mut self) {
        if self.shared_plugin.activated.load(Ordering::Relaxed) {
            log::trace!("clap plugin instance deactivate {}", &*self.shared_plugin.id);

            self.shared_plugin.activated.store(false, Ordering::Relaxed);
            unsafe {
                ((&*self.shared_plugin.raw_plugin).deactivate)(self.shared_plugin.raw_plugin)
            };
        } else {
            log::error!("Got request to deactivate CLAP plugin that is already inactive");
        }
    }

    /// Called by the host on the main thread in response to a previous call to `host.request_callback()`.
    ///
    /// By default this does nothing.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn on_main_thread(&mut self) {
        log::trace!("clap plugin instance on_main_thread {}", &*self.shared_plugin.id);

        unsafe {
            ((&*self.shared_plugin.raw_plugin).on_main_thread)(self.shared_plugin.raw_plugin)
        };
    }

    /// An optional extension that describes the configuration of audio ports on this plugin instance.
    ///
    /// This will only be called while the plugin is inactive.
    ///
    /// The default configuration is a main stereo input port and a main stereo output port.
    ///
    /// [main-thread & !active_state]
    fn audio_ports_ext(
        &mut self,
    ) -> Result<&ext::audio_ports::PluginAudioPortsExt, Box<dyn Error>> {
        match self.parse_audio_ports_extension() {
            Ok(audio_ports_ext) => {
                self.audio_ports_ext = Some(audio_ports_ext);

                Ok(self.audio_ports_ext.as_ref().unwrap())
            }
            Err(e) => {
                self.audio_ports_ext = None;

                Err(e)
            }
        }
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
        self.shared_plugin.params_ext.num_params()
    }

    /// Get the info of the given parameter.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns an Err(()).
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_info(&mut self, param_id: u32) -> Result<ext::params::ParamInfo, ()> {
        self.shared_plugin.params_ext.param_info(param_id)
    }

    /// Get the plain value of the parameter.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `Err(())`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_value(&self, param_id: u32) -> Result<f64, ()> {
        self.shared_plugin.params_ext.param_value(param_id)
    }

    /// Formats the display text for the given parameter value.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `Err(())`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_value_to_text(&self, param_id: u32, value: f64) -> Result<String, ()> {
        self.shared_plugin.params_ext.param_value_to_text(param_id, value)
    }

    /// Converts the display text to a parameter value.
    ///
    /// This will never be called if `PluginMainThread::num_params()` returned 0.
    ///
    /// By default this returns `Err(())`
    ///
    /// [main-thread]
    #[allow(unused)]
    fn param_text_to_value(&self, param_id: u32, display: &str) -> Result<f64, ()> {
        self.shared_plugin.params_ext.param_text_to_value(param_id, display)
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
        if let Some((clap_in_events, clap_out_events)) = &mut self.in_out_events {
            clap_in_events.sync(in_events);
            clap_out_events.sync(out_events);

            self.shared_plugin.params_ext.param_flush(clap_in_events, clap_out_events);
        }
    }
}

pub(crate) struct ClapPluginAudioThread {
    shared_plugin: Shared<SharedClapPluginInstance>,
    process: ClapProcess,
    task_version: u64,
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
        //log::trace!("clap plugin instance start_processing {}", &*self.shared_plugin.id);

        let res = unsafe {
            ((&*self.shared_plugin.raw_plugin).start_processing)(self.shared_plugin.raw_plugin)
        };
        if res {
            Ok(())
        } else {
            Err(())
        }
    }

    /// This will be called when the host puts the plugin to sleep.
    ///
    /// By default this trait method does nothing.
    ///
    /// `[audio-thread & active_state & processing_state]`
    fn stop_processing(&mut self) {
        //log::trace!("clap plugin instance stop_processing {}", &*self.shared_plugin.id);

        unsafe {
            ((&*self.shared_plugin.raw_plugin).stop_processing)(self.shared_plugin.raw_plugin)
        };
    }

    /// Process audio and events.
    ///
    /// `[audio-thread & active_state & processing_state]`
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        buffers: &mut ProcBuffers,
        in_events: &EventQueue,
        out_events: &mut EventQueue,
    ) -> ProcessStatus {
        use clap_sys::process::{
            CLAP_PROCESS_CONTINUE, CLAP_PROCESS_CONTINUE_IF_NOT_QUIET, CLAP_PROCESS_ERROR,
            CLAP_PROCESS_SLEEP, CLAP_PROCESS_TAIL,
        };

        if buffers.task_version != self.task_version {
            self.task_version = buffers.task_version;

            // Update the list of buffers.
            self.process.update_buffers(buffers);
        }

        self.process.sync_proc_info(proc_info, buffers, in_events, out_events);

        for b in buffers.audio_in.iter_mut() {
            b.sync_constant_mask_from_buffers();
        }

        let res = {
            #[cfg(debug_assertions)]
            // In debug mode, borrow all of the atomic ref cells to properly use the
            // safety checks, since external plugins just use the raw pointer to each
            // buffer.
            let (mut input_refs_f32, mut input_refs_f64, mut output_refs_f32, mut output_refs_f64) = {
                let mut input_refs_f32: SmallVec<[MaybeAtomicRef<'_, Vec<f32>>; 32]> =
                    SmallVec::new();
                let mut input_refs_f64: SmallVec<[MaybeAtomicRef<'_, Vec<f64>>; 32]> =
                    SmallVec::new();
                let mut output_refs_f32: SmallVec<[MaybeAtomicRefMut<'_, Vec<f32>>; 32]> =
                    SmallVec::new();
                let mut output_refs_f64: SmallVec<[MaybeAtomicRefMut<'_, Vec<f64>>; 32]> =
                    SmallVec::new();

                for in_port in buffers.audio_in.iter() {
                    match &in_port.raw_channels {
                        RawAudioChannelBuffers::F32(buffers) => {
                            for b in buffers.iter() {
                                input_refs_f32.push(unsafe { b.buffer.data.borrow() });
                            }
                        }
                        RawAudioChannelBuffers::F64(buffers) => {
                            for b in buffers.iter() {
                                input_refs_f64.push(unsafe { b.buffer.data.borrow() });
                            }
                        }
                    }
                }

                for out_port in buffers.audio_out.iter() {
                    match &out_port.raw_channels {
                        RawAudioChannelBuffers::F32(buffers) => {
                            for b in buffers.iter() {
                                output_refs_f32.push(unsafe { b.buffer.data.borrow_mut() });
                            }
                        }
                        RawAudioChannelBuffers::F64(buffers) => {
                            for b in buffers.iter() {
                                output_refs_f64.push(unsafe { b.buffer.data.borrow_mut() });
                            }
                        }
                    }
                }

                (input_refs_f32, input_refs_f64, output_refs_f32, output_refs_f64)
            };

            let res = unsafe {
                ((&*self.shared_plugin.raw_plugin).process)(
                    self.shared_plugin.raw_plugin,
                    self.process.raw(),
                )
            };

            #[cfg(debug_assertions)]
            input_refs_f32.clear();
            #[cfg(debug_assertions)]
            input_refs_f64.clear();
            #[cfg(debug_assertions)]
            output_refs_f32.clear();
            #[cfg(debug_assertions)]
            output_refs_f64.clear();

            res
        };

        self.process.sync_output_constant_masks(buffers);

        match res {
            CLAP_PROCESS_ERROR => ProcessStatus::Error,
            CLAP_PROCESS_CONTINUE => ProcessStatus::Continue,
            CLAP_PROCESS_CONTINUE_IF_NOT_QUIET => ProcessStatus::ContinueIfNotQuiet,
            CLAP_PROCESS_TAIL => ProcessStatus::Tail,
            CLAP_PROCESS_SLEEP => ProcessStatus::Sleep,
            _ => ProcessStatus::Error,
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
    fn param_flush(&mut self, in_events: &EventQueue, out_events: &mut EventQueue) {
        let (clap_in_events, clap_out_events) = self.process.sync_events(in_events, out_events);

        self.shared_plugin.params_ext.param_flush(clap_in_events, clap_out_events);
    }
}

fn parse_clap_plugin_descriptor(
    raw: *const RawClapPluginDescriptor,
    plugin_path: &PathBuf,
    plugin_index: u32,
) -> Result<PluginDescriptor, String> {
    if raw.is_null() {
        return Err(format!(
            "Plugin from path {:?} return null for its clap_plugin_descriptor at index: {}",
            plugin_path, plugin_index
        ));
    }

    let raw = unsafe { &*raw };

    let parse_mandatory = |raw_s: *const i8, field: &'static str| -> Result<String, String> {
        if let Some(s) = c_char_ptr_to_maybe_str(raw_s, CLAP_PATH_SIZE) {
            if let Ok(s) = s {
                let s = s.to_string();
                if s.is_empty() {
                    Err(format!("clap_plugin_descriptor has no {}", field))
                } else {
                    Ok(s)
                }
            } else {
                Err(format!("failed to parse {} from clap_plugin_descriptor", field))
            }
        } else {
            Err(format!("clap_plugin_descriptor has no {}", field))
        }
    };

    let parse_optional = |raw_s: *const i8, field: &'static str| -> String {
        if let Some(s) = c_char_ptr_to_maybe_str(raw_s, CLAP_PATH_SIZE) {
            if let Ok(s) = s {
                s.to_string()
            } else {
                log::warn!("failed to parse {} from clap_plugin_descriptor", field);
                String::new()
            }
        } else {
            String::new()
        }
    };

    let id = parse_mandatory(raw.id, "id")?;

    let version = parse_optional(raw.version, "version");
    let name = parse_optional(raw.name, "name");
    let vendor = parse_optional(raw.vendor, "vendor");
    let description = parse_optional(raw.description, "description");
    let url = parse_optional(raw.url, "url");
    let manual_url = parse_optional(raw.manual_url, "manual_url");
    let support_url = parse_optional(raw.support_url, "support_url");

    // TODO: features

    Ok(PluginDescriptor { id, name, version, vendor, description, url, manual_url, support_url })
}
