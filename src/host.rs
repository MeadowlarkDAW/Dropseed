use std::ffi::{CStr, CString};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::thread::ThreadId;

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

/// Used to get info and request actions from the host.
pub struct Host {
    pub(crate) info: Shared<HostInfo>,
    // We are storing this as a slice so we can get a raw pointer to the channel
    // for external plugins.
    pub(crate) plugin_channel: Shared<PluginInstanceChannel>,
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
        self.plugin_channel.restart_requested.store(true, Ordering::Relaxed);
    }

    /// Request the host to activate and start processing the plugin.
    /// This is useful if you have external IO and need to wake up the plugin from "sleep".
    ///
    /// `[thread-safe]`
    pub fn request_process(&self) {
        self.plugin_channel.process_requested.store(true, Ordering::Relaxed);
    }

    /// Request the host to schedule a call to `PluginMainThread::on_main_thread()` on the main thread.
    ///
    /// `[thread-safe]`
    pub fn request_callback(&self) {
        self.plugin_channel.callback_requested.store(true, Ordering::Relaxed);
    }
}

impl Clone for Host {
    fn clone(&self) -> Self {
        Self {
            info: Shared::clone(&self.info),
            plugin_channel: Shared::clone(&self.plugin_channel),
        }
    }
}
