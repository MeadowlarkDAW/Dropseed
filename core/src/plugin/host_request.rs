use basedrop::Shared;
use bitflags::bitflags;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use clack_extensions::audio_ports::RescanType;
use clack_extensions::note_ports::{NoteDialects, NotePortRescanFlags};
use clack_host::host::HostInfo as ClackHostInfo;

use crate::plugin::ext::params::HostParamsExtMainThread;

#[derive(Debug, Clone)]
pub struct HostInfo {
    /// The name of this host (mandatory).
    ///
    /// eg: "Meadowlark"
    pub name: String,

    /// The version of this host (mandatory).
    ///
    /// eg: "1.4.4", "1.0.2_beta"
    pub version: String,

    /// The vendor of this host.
    ///
    /// eg: "RustyDAW Org"
    pub vendor: Option<String>,

    /// The url to the product page of this host.
    ///
    /// eg: "https://meadowlark.app"
    pub url: Option<String>,

    pub clack_host_info: ClackHostInfo,
}

impl HostInfo {
    /// Create info about this host.
    ///
    /// - `name` - The name of this host (mandatory). eg: "Meadowlark"
    /// - `version` - The version of this host (mandatory). eg: "1.4.4", "1.0.2_beta"
    ///     - A quick way to do this is to set this equal to `String::new(env!("CARGO_PKG_VERSION"))`
    ///     to automatically update this when your crate version changes.
    ///
    /// - `vendor` - The vendor of this host. eg: "RustyDAW Org"
    /// - `url` - The url to the product page of this host. eg: "https://meadowlark.app"
    pub fn new(name: String, version: String, vendor: Option<String>, url: Option<String>) -> Self {
        let clack_host_info = ClackHostInfo::new(
            &name,
            vendor.as_deref().unwrap_or(""),
            url.as_deref().unwrap_or(""),
            &version,
        )
        .unwrap();

        Self { name, version, vendor, url, clack_host_info }
    }

    /// The version of the `RustyDAW Engine` used by this host.
    pub fn rusty_daw_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}

bitflags! {
    pub struct RequestFlags: u32 {
        /// Clears all possible references to a parameter
        const RESTART = 1 << 0;

        /// Clears all automations to a parameter
        const PROCESS = 1 << 1;

        /// Clears all modulations to a parameter
        const CALLBACK = 1 << 2;

        const DEACTIVATE = 1 << 3;

        const RESCAN_AUDIO_PORTS = 1 << 4;

        const RESCAN_NOTE_PORTS = 1 << 5;

        const STATE_DIRTY = 1 << 6;
    }
}

/// Used to get info and request actions from the host.
pub struct HostRequest {
    pub params: HostParamsExtMainThread,
    pub info: Shared<HostInfo>,

    /// Please do not use this! This is only intended to be used by
    /// the dropseed crate.
    pub _request_flags: Arc<AtomicU32>,
}

impl HostRequest {
    pub fn _new(info: Shared<HostInfo>) -> Self {
        Self {
            params: HostParamsExtMainThread::new(),
            info,
            _request_flags: Arc::new(AtomicU32::new(0)),
        }
    }

    // TODO: Move methods starting with `_` into the main dropseed crate.

    /// Request the host to deactivate and then reactivate the plugin.
    /// The operation may be delayed by the host.
    ///
    /// `[thread-safe]`
    pub fn request_restart(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self._request_flags.fetch_or(RequestFlags::RESTART.bits(), Ordering::SeqCst);
    }

    /// Request the host to activate and start processing the plugin.
    /// This is useful if you have external IO and need to wake up the plugin from "sleep".
    ///
    /// `[thread-safe]`
    pub fn request_process(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self._request_flags.fetch_or(RequestFlags::PROCESS.bits(), Ordering::SeqCst);
    }

    /// Request the host to schedule a call to `PluginMainThread::on_main_thread()` on the main thread.
    ///
    /// `[thread-safe]`
    pub fn request_callback(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self._request_flags.fetch_or(RequestFlags::CALLBACK.bits(), Ordering::SeqCst);
    }

    /// Checks if the host allows a plugin to change a given aspect of the audio ports definition.
    ///
    /// [main-thread]
    pub fn is_rescan_audio_ports_flag_supported(&self, _flag: RescanType) -> bool {
        // todo
        false
    }

    /// Checks if the host allows a plugin to change a given aspect of the audio ports definition.
    ///
    /// [main-thread]
    pub fn supported_note_dialects(&self) -> NoteDialects {
        // todo: more
        NoteDialects::CLAP | NoteDialects::MIDI | NoteDialects::MIDI2
    }

    /// Rescan the full list of audio ports according to the flags.
    ///
    /// It is illegal to ask the host to rescan with a flag that is not supported.
    ///
    /// Certain flags require the plugin to be de-activated.
    ///
    /// [main-thread]
    pub fn rescan_audio_ports(&self, _flags: RescanType) {
        // todo
    }

    pub fn rescan_note_ports(&self, _flags: NotePortRescanFlags) {
        // todo
    }

    /// Tell the host that the plugin state has changed and should be saved again.
    ///
    /// If a parameter value changes, then it is implicit that the state is dirty.
    ///
    /// [main-thread]
    pub fn mark_state_dirty(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self._request_flags.fetch_or(RequestFlags::STATE_DIRTY.bits(), Ordering::SeqCst);
    }

    /// Request the host to schedule a call to `PluginMainThread::on_main_thread()` on the main thread.
    ///
    /// `[thread-safe]`
    pub fn _request_deactivate(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self._request_flags.fetch_or(RequestFlags::DEACTIVATE.bits(), Ordering::SeqCst);
    }

    pub fn _load_requested(&self) -> RequestFlags {
        // TODO: Are we able to use relaxed ordering here?

        RequestFlags::from_bits_truncate(self._request_flags.load(Ordering::SeqCst))
    }

    pub fn _load_requested_and_reset_all(&self) -> RequestFlags {
        // TODO: Are we able to use relaxed ordering here?
        let flags = self._request_flags.fetch_and(0, Ordering::SeqCst);

        RequestFlags::from_bits_truncate(flags)
    }

    /// Returns true if the previous value had the `RequestFlags::RESTART` flag set.
    pub fn _reset_restart(&self) -> bool {
        // TODO: Are we able to use relaxed ordering here?
        let flags = self._request_flags.fetch_and(!RequestFlags::RESTART.bits(), Ordering::SeqCst);

        RequestFlags::from_bits_truncate(flags).contains(RequestFlags::RESTART)
    }

    pub fn _reset_process(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self._request_flags.fetch_and(!RequestFlags::PROCESS.bits(), Ordering::SeqCst);
    }

    /// Returns the value of the flags before the callback flag was reset.
    pub fn _load_requests_and_reset_callback(&self) -> RequestFlags {
        // TODO: Are we able to use relaxed ordering here?

        RequestFlags::from_bits_truncate(
            self._request_flags.fetch_and(!RequestFlags::CALLBACK.bits(), Ordering::SeqCst),
        )
    }

    pub fn _reset_deactivate(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self._request_flags.fetch_and(!RequestFlags::DEACTIVATE.bits(), Ordering::SeqCst);
    }

    #[allow(unused)]
    // TODO: Use this.
    pub fn _reset_rescan_audio_ports(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self
            ._request_flags
            .fetch_and(!RequestFlags::RESCAN_AUDIO_PORTS.bits(), Ordering::SeqCst);
    }

    #[allow(unused)]
    // TODO: Use this.
    pub fn _reset_rescan_note_ports(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self
            ._request_flags
            .fetch_and(!RequestFlags::RESCAN_NOTE_PORTS.bits(), Ordering::SeqCst);
    }

    pub fn _state_marked_dirty_and_reset_dirty(&self) -> bool {
        // TODO: Are we able to use relaxed ordering here?

        RequestFlags::from_bits_truncate(
            self._request_flags.fetch_and(!RequestFlags::STATE_DIRTY.bits(), Ordering::SeqCst),
        )
        .contains(RequestFlags::STATE_DIRTY)
    }
}

impl Clone for HostRequest {
    fn clone(&self) -> Self {
        Self {
            params: self.params.clone(),
            info: Shared::clone(&self.info),
            _request_flags: Arc::clone(&self._request_flags),
        }
    }
}