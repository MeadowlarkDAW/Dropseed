use basedrop::Shared;
use clap_sys::ext::audio_ports::clap_host_audio_ports as RawClapHostAudioPorts;
use clap_sys::ext::log::clap_host_log as RawClapHostLog;
use clap_sys::ext::note_ports::clap_host_note_ports as RawClapHostNotePorts;
use clap_sys::ext::params::clap_host_params as RawClapHostParams;
use clap_sys::ext::thread_check::clap_host_thread_check as RawClapHostThreadCheck;
use clap_sys::host::clap_host as RawClapHost;
use clap_sys::version::CLAP_VERSION;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::c_char_helpers::c_char_ptr_to_maybe_str;

use crate::plugin::ext::audio_ports::AudioPortRescanFlags;
use crate::plugin::ext::note_ports::NotePortRescanFlags;
use crate::plugin::host_request::HostRequest;
use crate::utils::thread_id::SharedThreadIDs;
use crate::ParamID;
use crate::PluginInstanceID;

// TODO: Make sure that the log and print methods don't allocate on the current thread.
// If they do, then we need to come up with a realtime-safe way to print to the terminal.

pub(crate) struct ClapHostRequest {
    // We are storing this as a slice so we can get a raw pointer
    // for external plugins.
    raw: Shared<[RawClapHost; 1]>,
    // We are storing this as a slice so we can get a raw pointer
    // for external plugins.
    host_data: Shared<[HostData; 1]>,
}

impl ClapHostRequest {
    pub(crate) fn new(
        host_request: HostRequest,
        thread_ids: SharedThreadIDs,
        plugin_id: PluginInstanceID,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        let plugin_log_name = Shared::new(coll_handle, format!("{:?}", &plugin_id));

        let host_data = Shared::new(
            coll_handle,
            [HostData {
                plug_did_create: Arc::new(AtomicBool::new(false)),
                plugin_id,
                host_request,
                host_audio_ports: [RawClapHostAudioPorts {
                    is_rescan_flag_supported: is_rescan_audio_ports_flag_supported,
                    rescan: rescan_audio_ports,
                }],
                host_note_ports: [RawClapHostNotePorts {
                    supported_dialects: supported_note_dialects,
                    rescan: rescan_note_ports,
                }],
                host_thread_check: [RawClapHostThreadCheck { is_main_thread, is_audio_thread }],
                host_params: [RawClapHostParams {
                    rescan: params_rescan,
                    clear: params_clear,
                    request_flush: params_request_flush,
                }],
                host_log: [RawClapHostLog { log }],
                plugin_log_name,
                thread_ids,
            }],
        );

        // SAFETY: This is safe because the data lives inside the Host struct,
        // which ensures that the data is alive for as long as there is a
        // reference to it.
        //
        // In addition, this data is wrapped inside basedrop's `Shared` pointer,
        // which ensures that the underlying data doesn't move.
        let raw = Shared::new(
            coll_handle,
            [RawClapHost {
                clap_version: CLAP_VERSION,

                host_data: (*host_data).as_ptr() as *mut c_void,

                name: host_data[0].host_request.info._c_name.as_ptr(),
                vendor: host_data[0].host_request.info._c_vendor.as_ptr(),
                url: host_data[0].host_request.info._c_url.as_ptr(),
                version: host_data[0].host_request.info._c_version.as_ptr(),

                // This is safe because these functions are static.
                get_extension,
                request_restart,
                request_process,
                request_callback,
            }],
        );

        Self { raw, host_data }
    }

    // SAFETY: This is safe because the data lives inside this struct,
    // which ensures that the data is alive for as long as there is a
    // reference to it.
    //
    // In addition, this data is wrapped inside basedrop's `Shared` pointer,
    // which ensures that the underlying data doesn't move.
    pub(crate) fn get_raw(&self) -> *const RawClapHost {
        (*self.raw).as_ptr()
    }

    pub(crate) fn plugin_created(&mut self) {
        self.host_data[0].plug_did_create.store(true, Ordering::Relaxed);
    }
}

impl Clone for ClapHostRequest {
    fn clone(&self) -> Self {
        Self { raw: Shared::clone(&self.raw), host_data: Shared::clone(&self.host_data) }
    }
}

struct HostData {
    plug_did_create: Arc<AtomicBool>,
    plugin_id: PluginInstanceID,
    host_request: HostRequest,
    host_audio_ports: [RawClapHostAudioPorts; 1],
    host_note_ports: [RawClapHostNotePorts; 1],
    host_thread_check: [RawClapHostThreadCheck; 1],
    host_params: [RawClapHostParams; 1],
    host_log: [RawClapHostLog; 1],
    plugin_log_name: Shared<String>,

    thread_ids: SharedThreadIDs,
}

unsafe fn parse_clap_host<'a>(clap_host: *const RawClapHost) -> Result<&'a HostData, ()> {
    if clap_host.is_null() {
        log::warn!("Received a null clap_host_t pointer from plugin");
        return Err(());
    }

    let host = &*clap_host;

    if host.host_data.is_null() {
        log::warn!("Received a null clap_host_t->host_data pointer from plugin");
        return Err(());
    }

    Ok(&*(host.host_data as *const HostData))
}

/// [thread-safe]
unsafe extern "C" fn get_extension(
    clap_host: *const RawClapHost,
    extension_id: *const i8,
) -> *const c_void {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return ptr::null(),
    };

    if extension_id.is_null() {
        log::warn!(
            "Call to `get_extension(host: *const clap_host, extension_id: *const i8) received a null pointer in extension_id from plugin`"
        );
        return ptr::null();
    }

    if !host_data.plug_did_create.load(Ordering::Relaxed) {
        log::warn!(
            "The plugin can't query for extensions during the create method. Wait for the clap_plugin.init() call."
        );
        return ptr::null();
    }

    let extension_id = if let Some(Ok(extension_id)) =
        c_char_ptr_to_maybe_str(extension_id, clap_sys::string_sizes::CLAP_NAME_SIZE)
    {
        extension_id
    } else {
        log::error!(
            "Failed to parse extension id from call to clap_host_audio_ports->get_extension()"
        );
        return ptr::null();
    };

    if extension_id == "clap.thread-check" {
        // Safe because host_data is pinned in place via the `Shared` pointer.
        return (host_data.host_thread_check).as_ptr() as *const c_void;
    }

    if extension_id == "clap.audio-ports" {
        // Safe because host_data is pinned in place via the `Shared` pointer.
        return (host_data.host_audio_ports).as_ptr() as *const c_void;
    }

    if extension_id == "clap.note-ports" {
        // Safe because host_data is pinned in place via the `Shared` pointer.
        return (host_data.host_note_ports).as_ptr() as *const c_void;
    }

    if extension_id == "clap.log" {
        // Safe because host_data is pinned in place via the `Shared` pointer.
        return (host_data.host_log).as_ptr() as *const c_void;
    }

    if extension_id == "clap.params" {
        // Safe because host_data is pinned in place via the `Shared` pointer.
        return (host_data.host_params).as_ptr() as *const c_void;
    }

    ptr::null()
}

/// [main-thread]
unsafe extern "C" fn is_rescan_audio_ports_flag_supported(
    clap_host: *const RawClapHost,
    flag: u32,
) -> bool {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return false,
    };

    if !host_data.thread_ids.is_external_main_thread() {
        log::warn!("Plugin called clap_host_audio_ports->is_rescan_flag_supported() not in the main thread");
        return false;
    }

    let flag = AudioPortRescanFlags::from_bits_truncate(flag);

    host_data.host_request.is_rescan_audio_ports_flag_supported(flag)
}

/// [main-thread]
unsafe extern "C" fn rescan_audio_ports(clap_host: *const RawClapHost, flags: u32) {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    if !host_data.thread_ids.is_external_main_thread() {
        log::warn!("Plugin called clap_host_audio_ports->rescan() not in the main thread");
        return;
    }

    let flags = AudioPortRescanFlags::from_bits_truncate(flags);

    host_data.host_request.rescan_audio_ports(flags);
}

/// [main-thread]
unsafe extern "C" fn supported_note_dialects(clap_host: *const RawClapHost) -> u32 {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return 0,
    };

    if !host_data.thread_ids.is_external_main_thread() {
        log::warn!(
            "Plugin called clap_host_note_ports->supported_dialects() not in the main thread"
        );
        return 0;
    }

    host_data.host_request.supported_note_dialects().bits()
}

/// [main-thread]
unsafe extern "C" fn rescan_note_ports(clap_host: *const RawClapHost, flags: u32) {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    if !host_data.thread_ids.is_external_main_thread() {
        log::warn!("Plugin called clap_host_note_ports->rescan() not in the main thread");
        return;
    }

    let flags = NotePortRescanFlags::from_bits_truncate(flags);

    host_data.host_request.rescan_note_ports(flags);
}

/// [thread-safe]
unsafe extern "C" fn request_restart(clap_host: *const RawClapHost) {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    host_data.host_request.request_restart();
}

/// [thread-safe]
unsafe extern "C" fn request_process(clap_host: *const RawClapHost) {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    host_data.host_request.request_process();
}

/// [thread-safe]
unsafe extern "C" fn request_callback(clap_host: *const RawClapHost) {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    host_data.host_request.request_callback();
}

/// [thread-safe]
unsafe extern "C" fn is_main_thread(clap_host: *const RawClapHost) -> bool {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return false,
    };

    if let Some(thread_id) = host_data.thread_ids.external_main_thread_id() {
        std::thread::current().id() == thread_id
    } else {
        log::error!("external_main_thread_id is None");
        false
    }
}

/// [thread-safe]
unsafe extern "C" fn is_audio_thread(clap_host: *const RawClapHost) -> bool {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return false,
    };

    if let Some(thread_id) = host_data.thread_ids.external_audio_thread_id() {
        std::thread::current().id() == thread_id
    } else {
        log::error!("external_audio_thread_id is None");
        false
    }
}

/// [thread-safe]
unsafe extern "C" fn log(clap_host: *const RawClapHost, severity: i32, msg: *const i8) {
    use clap_sys::ext::log::{
        CLAP_LOG_DEBUG, CLAP_LOG_ERROR, CLAP_LOG_FATAL, CLAP_LOG_HOST_MISBEHAVING, CLAP_LOG_INFO,
        CLAP_LOG_PLUGIN_MISBEHAVING, CLAP_LOG_WARNING,
    };

    // TODO: Flags so the user can choose which plugins to log.

    // TODO: Send messages to the engine thread once we have plugin sandboxing.

    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    if msg.is_null() {
        log::warn!(
            "Call to `log(host: *const clap_host, severity: i32, msg: *const char) received a null pointer for msg from plugin`"
        );
        return;
    }

    // Assume that the user has passed in a null-terminated string.
    //
    // TODO: Safegaurd against non-null-terminated strings?
    let msg = std::ffi::CStr::from_ptr(msg);

    let msg = if let Ok(msg) = msg.to_str() {
        msg
    } else {
        log::warn!(
            "Failed to parse msg in plugin's call to `log(host: *const clap_host, severity: i32, msg: *const char)`"
        );
        return;
    };

    let level = match severity {
        CLAP_LOG_DEBUG => log::Level::Debug,
        CLAP_LOG_INFO => log::Level::Info,
        CLAP_LOG_WARNING => log::Level::Warn,
        CLAP_LOG_ERROR => log::Level::Error,
        CLAP_LOG_FATAL => log::Level::Error,
        CLAP_LOG_HOST_MISBEHAVING => log::Level::Error,
        CLAP_LOG_PLUGIN_MISBEHAVING => log::Level::Error,
        _ => log::Level::Debug,
    };

    log::log!(level, "{}", &*host_data.plugin_log_name);
    log::log!(level, "{}", msg);
}

/// ---  Parameters  -------------------------------------------------------------

/// [main-thread]
unsafe extern "C" fn params_rescan(clap_host: *const RawClapHost, rescan_flags: u32) {
    use crate::plugin::ext::params::ParamRescanFlags;

    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    if !host_data.thread_ids.is_external_main_thread() {
        log::warn!("Plugin called clap_host_params->rescan() not in the main thread");
        return;
    }

    let flags = ParamRescanFlags::from_bits_truncate(rescan_flags);

    host_data.host_request.params.rescan(flags);
}

/// [main-thread]
unsafe extern "C" fn params_clear(clap_host: *const RawClapHost, param_id: u32, clear_flags: u32) {
    use crate::plugin::ext::params::ParamClearFlags;

    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    if !host_data.thread_ids.is_external_main_thread() {
        log::warn!("Plugin called clap_host_params->clear() not in the main thread");
        return;
    }

    let flags = ParamClearFlags::from_bits_truncate(clear_flags);

    host_data.host_request.params.clear(ParamID(param_id), flags);
}

/// [main-thread]
unsafe extern "C" fn params_request_flush(clap_host: *const RawClapHost) {
    let host_data = match parse_clap_host(clap_host) {
        Ok(host_data) => host_data,
        Err(()) => return,
    };

    if host_data.thread_ids.is_external_audio_thread() {
        log::warn!("Plugin called clap_host_params->request_flush() in the audio thread");
        return;
    }

    host_data.host_request.params.request_flush();
}
