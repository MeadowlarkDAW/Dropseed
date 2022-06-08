use basedrop::Shared;
use bitflags::bitflags;
use std::ffi::{CStr, CString};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

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

    pub(crate) _c_name: Pin<Box<CStr>>,
    pub(crate) _c_vendor: Pin<Box<CStr>>,
    pub(crate) _c_url: Pin<Box<CStr>>,
    pub(crate) _c_version: Pin<Box<CStr>>,
}

fn to_pin_cstr(str: &str) -> Pin<Box<CStr>> {
    Pin::new(CString::new(str).unwrap_or(CString::new("Error").unwrap()).into_boxed_c_str())
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
        let _c_name: Pin<Box<CStr>> = to_pin_cstr(name.as_str());
        let _c_vendor: Pin<Box<CStr>> =
            to_pin_cstr(vendor.as_ref().map(|s| s.as_str()).unwrap_or(""));
        let _c_url: Pin<Box<CStr>> = to_pin_cstr(vendor.as_ref().map(|s| s.as_str()).unwrap_or(""));
        let _c_version: Pin<Box<CStr>> = to_pin_cstr(&version);

        Self { name, version, vendor, url, _c_name, _c_vendor, _c_url, _c_version }
    }

    /// The version of the `RustyDAW Engine` used by this host.
    pub fn rusty_daw_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}

bitflags! {
    pub(crate) struct RequestFlags: u32 {
        /// Clears all possible references to a parameter
        const RESTART = 1 << 0;

        /// Clears all automations to a parameter
        const PROCESS = 1 << 1;

        /// Clears all modulations to a parameter
        const CALLBACK = 1 << 2;

        const DEACTIVATE = 1 << 3;
    }
}

/// Used to get info and request actions from the host.
pub struct HostRequest {
    pub params: HostParamsExtMainThread,
    pub(crate) info: Shared<HostInfo>,
    request_flags: Arc<AtomicU32>,
}

impl HostRequest {
    pub(crate) fn new(info: Shared<HostInfo>) -> Self {
        Self {
            params: HostParamsExtMainThread::new(),
            info,
            request_flags: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Retrieve info about this host.
    ///
    /// `[thread-safe]`
    pub fn info(&self) -> Shared<HostInfo> {
        Shared::clone(&self.info)
    }

    /// Request the host to deactivate and then reactivate the plugin.
    /// The operation may be delayed by the host.
    ///
    /// `[thread-safe]`
    pub fn request_restart(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self.request_flags.fetch_or(RequestFlags::RESTART.bits(), Ordering::SeqCst);
    }

    /// Request the host to activate and start processing the plugin.
    /// This is useful if you have external IO and need to wake up the plugin from "sleep".
    ///
    /// `[thread-safe]`
    pub fn request_process(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self.request_flags.fetch_or(RequestFlags::PROCESS.bits(), Ordering::SeqCst);
    }

    /// Request the host to schedule a call to `PluginMainThread::on_main_thread()` on the main thread.
    ///
    /// `[thread-safe]`
    pub fn request_callback(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self.request_flags.fetch_or(RequestFlags::CALLBACK.bits(), Ordering::SeqCst);
    }

    /// Request the host to schedule a call to `PluginMainThread::on_main_thread()` on the main thread.
    ///
    /// `[thread-safe]`
    pub(crate) fn request_deactivate(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self.request_flags.fetch_or(RequestFlags::DEACTIVATE.bits(), Ordering::SeqCst);
    }

    pub(crate) fn load_requested(&self) -> RequestFlags {
        // TODO: Are we able to use relaxed ordering here?

        // Safe because this u32 can only be set via a `RequestFlags` value.
        unsafe { RequestFlags::from_bits_unchecked(self.request_flags.load(Ordering::SeqCst)) }
    }

    pub(crate) fn load_requested_and_reset_all(&self) -> RequestFlags {
        // TODO: Are we able to use relaxed ordering here?
        let flags = self.request_flags.fetch_and(0, Ordering::SeqCst);

        // Safe because this u32 can only be set via a `RequestFlags` value.
        unsafe { RequestFlags::from_bits_unchecked(flags) }
    }

    /// Returns true if the previous value had the `RequestFlags::RESTART` flag set.
    pub(crate) fn reset_restart(&self) -> bool {
        // TODO: Are we able to use relaxed ordering here?
        let flags = self.request_flags.fetch_and(!RequestFlags::RESTART.bits(), Ordering::SeqCst);

        // Safe because this u32 can only be set via a `RequestFlags` value.
        unsafe { RequestFlags::from_bits_unchecked(flags).contains(RequestFlags::RESTART) }
    }

    pub(crate) fn reset_process(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self.request_flags.fetch_and(!RequestFlags::PROCESS.bits(), Ordering::SeqCst);
    }

    /// Returns the value of the flags before the callback flag was reset.
    pub(crate) fn load_requests_and_reset_callback(&self) -> RequestFlags {
        // TODO: Are we able to use relaxed ordering here?

        // Safe because this u32 can only be set via a `RequestFlags` value.
        unsafe {
            RequestFlags::from_bits_unchecked(
                self.request_flags.fetch_and(!RequestFlags::CALLBACK.bits(), Ordering::SeqCst),
            )
        }
    }

    pub(crate) fn reset_deactivate(&self) {
        // TODO: Are we able to use relaxed ordering here?
        let _ = self.request_flags.fetch_and(!RequestFlags::DEACTIVATE.bits(), Ordering::SeqCst);
    }
}

impl Clone for HostRequest {
    fn clone(&self) -> Self {
        Self {
            params: self.params.clone(),
            info: Shared::clone(&self.info),
            request_flags: Arc::clone(&self.request_flags),
        }
    }
}
