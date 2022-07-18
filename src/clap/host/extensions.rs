use crate::clap::host::{ClapHostMainThread, ClapHostShared};
use clack_extensions::audio_ports::{HostAudioPortsImplementation, RescanType};
use clack_extensions::gui::{GuiError, GuiSize, HostGuiImplementation};
use clack_extensions::log::implementation::HostLog;
use clack_extensions::log::LogSeverity;
use clack_extensions::params::{
    HostParamsImplementation, HostParamsImplementationMainThread, ParamClearFlags, ParamRescanFlags,
};
use clack_extensions::thread_check::host::ThreadCheckImplementation;
use dropseed_core::plugin::HostRequestFlags;

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
            // self.shared.host_request.request_restart(); // TODO
        }
    }
}

impl<'a> HostParamsImplementation for ClapHostShared<'a> {
    #[inline]
    fn request_flush(&self) {
        self.host_request.request(HostRequestFlags::FLUSH_PARAMS);
    }
}

impl<'a> HostParamsImplementationMainThread for ClapHostMainThread<'a> {
    fn rescan(&mut self, flags: ParamRescanFlags) {
        if !self.shared.thread_ids.is_external_main_thread() {
            log::warn!("Plugin called clap_host_params->rescan() not in the main thread");
            return;
        }

        let flags = ParamRescanFlags::from_bits_truncate(flags.bits());

        self.rescan_requested =
            Some(self.rescan_requested.unwrap_or(ParamRescanFlags::empty()) | flags);
    }

    fn clear(&mut self, param_id: u32, flags: ParamClearFlags) {
        if !self.shared.thread_ids.is_external_main_thread() {
            log::warn!("Plugin called clap_host_params->clear() not in the main thread");
            return;
        }

        let flags = ParamClearFlags::from_bits_truncate(flags.bits());

        // self.shared.host_request.params.clear(ParamID(param_id), flags); TODO: Vec for each ParamID to clear?
    }
}

impl<'a> HostGuiImplementation for ClapHostShared<'a> {
    fn resize_hints_changed(&self) {
        self.host_request.request(HostRequestFlags::GUI_HINTS_CHANGED);
    }

    fn request_resize(&self, new_size: GuiSize) -> Result<(), GuiError> {
        self.host_request.request_gui_resize(new_size);
        Ok(())
    }

    fn request_show(&self) -> Result<(), GuiError> {
        self.host_request.request(HostRequestFlags::GUI_SHOW);
        Ok(())
    }

    fn request_hide(&self) -> Result<(), GuiError> {
        self.host_request.request(HostRequestFlags::GUI_HIDE);
        Ok(())
    }

    fn closed(&self, was_destroyed: bool) {
        if was_destroyed {
            self.host_request.request(HostRequestFlags::GUI_DESTROYED)
        } else {
            self.host_request.request(HostRequestFlags::GUI_CLOSED)
        }
    }
}
