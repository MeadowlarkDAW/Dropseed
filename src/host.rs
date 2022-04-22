use clap_sys::host::clap_host;
use std::ffi::{CStr, CString};
use std::pin::Pin;
use std::sync::atomic::Ordering;

use basedrop::Shared;

use crate::graph::plugin_pool::PluginInstanceChannel;

#[derive(Debug)]
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

    c_name: Pin<Box<CStr>>,
    c_vendor: Pin<Box<CStr>>,
    c_url: Pin<Box<CStr>>,
    c_version: Pin<Box<CStr>>,
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
        let c_name: Pin<Box<CStr>> = to_pin_cstr(name.as_str());
        let c_vendor: Pin<Box<CStr>> =
            to_pin_cstr(vendor.as_ref().map(|s| s.as_str()).unwrap_or(""));
        let c_url: Pin<Box<CStr>> = to_pin_cstr(vendor.as_ref().map(|s| s.as_str()).unwrap_or(""));
        let c_version: Pin<Box<CStr>> = to_pin_cstr(&version);

        Self { name, version, vendor, url, c_name, c_vendor, c_url, c_version }
    }

    /// The version of the `RustyDAW Engine` used by this host.
    pub fn rusty_daw_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    pub(crate) unsafe fn write_to_raw(&self, host: &mut clap_host) {
        host.name = self.c_name.as_ptr();
        host.vendor = self.c_vendor.as_ptr();
        host.url = self.c_url.as_ptr();
        host.version = self.c_version.as_ptr();
    }
}

/// Used to get info and request actions from the host.
pub struct Host {
    pub(crate) info: Shared<HostInfo>,
    pub(crate) current_plugin_channel: Shared<PluginInstanceChannel>,
}

impl Host {
    /// Retrieve info about this host.
    pub fn info(&self) -> Shared<HostInfo> {
        Shared::clone(&self.info)
    }

    /// Request the host to deactivate and then reactivate the plugin.
    /// The operation may be delayed by the host.
    ///
    /// `[thread-safe]`
    pub fn request_restart(&self) {
        self.current_plugin_channel.restart_requested.store(true, Ordering::Relaxed);
    }

    /// Request the host to activate and start processing the plugin.
    /// This is useful if you have external IO and need to wake up the plugin from "sleep".
    ///
    /// `[thread-safe]`
    pub fn request_process(&self) {
        self.current_plugin_channel.process_requested.store(true, Ordering::Relaxed);
    }

    /// Request the host to schedule a call to `PluginMainThread::on_main_thread()` on the main thread.
    ///
    /// `[thread-safe]`
    pub fn request_callback(&self) {
        self.current_plugin_channel.callback_requested.store(true, Ordering::Relaxed);
    }
}
