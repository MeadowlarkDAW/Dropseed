use basedrop::Shared;
use rusty_daw_core::SampleRate;
use std::error::Error;
use std::ffi::CString;
use std::path::PathBuf;

use clap_sys::entry::clap_plugin_entry as RawClapEntry;
use clap_sys::plugin::clap_plugin as RawClapPlugin;
use clap_sys::plugin::clap_plugin_descriptor as RawClapPluginDescriptor;
use clap_sys::plugin_factory::clap_plugin_factory as RawClapPluginFactory;
use clap_sys::plugin_factory::CLAP_PLUGIN_FACTORY_ID;
use clap_sys::process::clap_process as RawClapProcess;
use clap_sys::process::{
    CLAP_PROCESS_CONTINUE, CLAP_PROCESS_CONTINUE_IF_NOT_QUIET, CLAP_PROCESS_ERROR,
    CLAP_PROCESS_SLEEP, CLAP_PROCESS_TAIL,
};
use clap_sys::string_sizes::CLAP_PATH_SIZE;

use super::c_char_helpers::c_char_ptr_to_maybe_str;
use super::host::ClapHostRequest;
use crate::host::HostRequest;
use crate::plugin::{ext, PluginAudioThread, PluginDescriptor, PluginFactory, PluginMainThread};
use crate::{AudioPortBuffer, ProcInfo, ProcessStatus};

struct SharedClapPluginFactory {
    // We hold on to this to make sure the host callback stays alive for as long as a
    // reference to this struct exists.
    _lib: libloading::Library,

    raw_entry: *const RawClapEntry,
    raw_factory: *const RawClapPluginFactory,
}

// This is safe because we only ever dereference the contained pointers in
// the main thread.
unsafe impl Send for SharedClapPluginFactory {}
// This is safe because we only ever dereference the contained pointers in
// the main thread.
unsafe impl Sync for SharedClapPluginFactory {}

impl Drop for SharedClapPluginFactory {
    fn drop(&mut self) {
        // Safe because the constructor made sure that this is a valid pointer.
        unsafe {
            ((&*self.raw_entry).deinit);
        }
    }
}

pub(crate) struct ClapPluginFactory {
    shared: Shared<SharedClapPluginFactory>,
    descriptor: PluginDescriptor,
    id: Shared<String>,
    c_id: CString,
}

impl ClapPluginFactory {
    pub fn entry_init(
        plugin_path: &PathBuf,
        coll_handle: &basedrop::Handle,
    ) -> Result<Vec<Self>, Box<dyn Error>> {
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

        let shared_factory =
            Shared::new(coll_handle, SharedClapPluginFactory { _lib: lib, raw_entry, raw_factory });

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

        let mut factories: Vec<Self> = Vec::with_capacity(num_plugins as usize);

        for i in 0..num_plugins {
            // Safe because this is the correct format of this function as described in the
            // CLAP spec.
            let raw_descriptor = unsafe { ((&*raw_factory).get_plugin_descriptor)(raw_factory, i) };

            let descriptor = parse_clap_plugin_descriptor(raw_descriptor, plugin_path, i)?;

            let id = Shared::new(coll_handle, descriptor.id.clone());

            let c_id = CString::new(descriptor.id.clone()).unwrap();

            factories.push(Self { shared: Shared::clone(&shared_factory), descriptor, id, c_id });
        }

        Ok(factories)
    }
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
        host_request: &HostRequest,
        coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginMainThread>, Box<dyn Error>> {
        let clap_host_request = ClapHostRequest::new(host_request.clone(), coll_handle);

        let raw_plugin = unsafe {
            ((&*self.shared.raw_factory).create_plugin)(
                self.shared.raw_factory,
                clap_host_request.get_raw(),
                self.c_id.as_ptr(),
            )
        };

        if raw_plugin.is_null() {
            return Err(format!(
                "Plugin with ID {} returned null while calling clap_plugin_factory.create_plugin()",
                &self.descriptor.id
            )
            .into());
        }

        let shared_plugin = Shared::new(
            coll_handle,
            SharedClapPluginInstance {
                raw_plugin,
                id: Shared::clone(&self.id),
                _host_request: clap_host_request,
            },
        );

        Ok(Box::new(ClapPluginMainThread { shared_plugin }))
    }
}

struct SharedClapPluginInstance {
    raw_plugin: *const RawClapPlugin,
    id: Shared<String>,

    // We hold on to this to make sure the host callback stays alive for as long as a
    // reference to this struct exists.
    _host_request: ClapHostRequest,
}

impl Drop for SharedClapPluginInstance {
    fn drop(&mut self) {
        // Safe because the constructor ensures that this is a valid pointer.
        unsafe {
            ((&*self.raw_plugin).destroy)(self.raw_plugin);
        }
    }
}

// This is safe because we are upholding the threading model as defined in the CLAP spec.
unsafe impl Send for SharedClapPluginInstance {}
// This is safe because we are upholding the threading model as defined in the CLAP spec.
unsafe impl Sync for SharedClapPluginInstance {}

struct ClapPluginMainThread {
    shared_plugin: Shared<SharedClapPluginInstance>,
}

impl PluginMainThread for ClapPluginMainThread {
    /// This is called after creating a plugin instance and once it's safe for the plugin to
    /// use the host callback methods.
    ///
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// By default this does nothing.
    ///
    /// `[main-thread & !active_state]`
    #[allow(unused)]
    fn init(
        &mut self,
        _host_request: &HostRequest,
        coll_handle: &basedrop::Handle,
    ) -> Result<(), Box<dyn Error>> {
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
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// Once activated the latency and port configuration must remain constant, until deactivation.
    ///
    /// `[main-thread & !active_state]`
    fn activate(
        &mut self,
        sample_rate: SampleRate,
        min_frames: usize,
        max_frames: usize,
        _host_request: &HostRequest,
        _coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginAudioThread>, Box<dyn Error>> {
        let res = unsafe {
            ((&*self.shared_plugin.raw_plugin).activate)(
                self.shared_plugin.raw_plugin,
                sample_rate.0,
                min_frames as u32,
                max_frames as u32,
            )
        };

        if res {
            Ok(Box::new(ClapPluginAudioThread {
                shared_plugin: Shared::clone(&self.shared_plugin),
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
    fn deactivate(&mut self, _host_request: &HostRequest) {
        unsafe { ((&*self.shared_plugin.raw_plugin).deactivate)(self.shared_plugin.raw_plugin) };
    }

    /// Called by the host on the main thread in response to a previous call to `host.request_callback()`.
    ///
    /// By default this does nothing.
    ///
    /// [main-thread]
    #[allow(unused)]
    fn on_main_thread(&mut self, _host_request: &HostRequest) {
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
    #[allow(unused)]
    fn audio_ports_extension(
        &self,
        host_request: &HostRequest,
    ) -> ext::audio_ports::AudioPortsExtension {
        todo!()
    }
}

struct ClapPluginAudioThread {
    shared_plugin: Shared<SharedClapPluginInstance>,
}

impl ClapPluginAudioThread {
    fn process_clap(&mut self, clap_process: *const RawClapProcess) -> ProcessStatus {
        let res = unsafe {
            ((&*self.shared_plugin.raw_plugin).process)(self.shared_plugin.raw_plugin, clap_process)
        };

        match res {
            CLAP_PROCESS_ERROR => ProcessStatus::Error,
            CLAP_PROCESS_CONTINUE => ProcessStatus::Continue,
            CLAP_PROCESS_CONTINUE_IF_NOT_QUIET => ProcessStatus::ContinueIfNotQuiet,
            CLAP_PROCESS_TAIL => ProcessStatus::Tail,
            CLAP_PROCESS_SLEEP => ProcessStatus::Sleep,
            _ => ProcessStatus::Error,
        }
    }
}

impl PluginAudioThread for ClapPluginAudioThread {
    /// This will be called each time before a call to `process()`.
    ///
    /// Return an error if the plugin failed to start processing. In this case the host will not
    /// call `process()` this process cycle.
    ///
    /// By default this just returns `Ok(())`.
    ///
    /// `[audio-thread & active_state & !processing_state]`
    #[allow(unused)]
    fn start_processing(&mut self, host_request: &HostRequest) -> Result<(), ()> {
        let res = unsafe {
            ((&*self.shared_plugin.raw_plugin).start_processing)(self.shared_plugin.raw_plugin)
        };

        if res {
            Ok(())
        } else {
            Err(())
        }
    }

    /// This will be called each time after a call to `process()`.
    ///
    /// By default this does nothing.
    ///
    /// `[audio-thread & active_state & processing_state]`
    #[allow(unused)]
    fn stop_processing(&mut self, host_request: &HostRequest) {
        unsafe {
            ((&*self.shared_plugin.raw_plugin).stop_processing)(self.shared_plugin.raw_plugin)
        };
    }

    /// This will not be used for CLAP plugins. Instead, the host will call
    /// `ClapPluginAudioThread::process_clap()`.
    fn process(
        &mut self,
        _info: &ProcInfo,
        _audio_in: &[AudioPortBuffer],
        _audio_out: &mut [AudioPortBuffer],
        _host_request: &HostRequest,
    ) -> ProcessStatus {
        ProcessStatus::Error
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

    let parse_optional = |raw_s: *const i8, field: &'static str| -> Option<String> {
        if let Some(s) = c_char_ptr_to_maybe_str(raw_s, CLAP_PATH_SIZE) {
            if let Ok(s) = s {
                let s = s.to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            } else {
                log::warn!("failed to parse {} from clap_plugin_descriptor", field);
                None
            }
        } else {
            None
        }
    };

    let id = parse_mandatory(raw.id, "id")?;
    let name = parse_mandatory(raw.name, "name")?;
    let version = parse_mandatory(raw.version, "version")?;

    let vendor = parse_optional(raw.vendor, "vendor");
    let description = parse_optional(raw.description, "description");
    let url = parse_optional(raw.url, "url");
    let manual_url = parse_optional(raw.manual_url, "manual_url");
    let support_url = parse_optional(raw.support_url, "support_url");

    // TODO: features

    Ok(PluginDescriptor { id, name, version, vendor, description, url, manual_url, support_url })
}
