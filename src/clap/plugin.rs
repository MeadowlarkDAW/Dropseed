use basedrop::Shared;
use log::warn;
use std::error::Error;
use std::path::PathBuf;

use clap_sys::plugin::clap_plugin_descriptor as RawClapPluginDescriptor;
use clap_sys::string_sizes::CLAP_PATH_SIZE;
use clap_sys::version::clap_version as ClapVersion;

use super::c_char_helpers::c_char_ptr_to_maybe_str;
use crate::host::HostInfo;
use crate::plugin::{
    PluginAudioThread, PluginDescriptor, PluginFactory, PluginMainThread, PluginSaveState,
};

pub(crate) struct ClapPluginFactory {}

impl PluginFactory for ClapPluginFactory {
    /// This function is always called first and only once.
    ///
    /// * `plugin_path` - The path to the shared library that was loaded. This will be `None`
    /// for internal plugins.
    ///
    /// This method should be as fast as possible, in order to perform very quick scan of the plugin
    /// descriptors.
    ///
    /// It is forbidden to display graphical user interface in this call.
    /// It is forbidden to perform user inter-action in this call.
    ///
    /// If the initialization depends upon expensive computation, maybe try to do them ahead of time
    /// and cache the result.
    #[allow(unused_attributes)]
    fn entry_init(
        &mut self,
        plugin_path: Option<&PathBuf>,
    ) -> Result<PluginDescriptor, Box<dyn Error>> {
        todo!()
    }

    /// Create a new instance of this plugin.
    ///
    /// A `basedrop` collector handle is provided for realtime-safe garbage collection.
    ///
    /// `[main-thread]`
    fn new(
        &mut self,
        host_info: Shared<HostInfo>,
        coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginMainThread>, Box<dyn Error>> {
        todo!()
    }
}

fn parse_clap_plugin_descriptor(
    raw: *const RawClapPluginDescriptor,
) -> Result<(PluginDescriptor, ClapVersion), String> {
    if raw.is_null() {
        return Err(String::from("clap_plugin_descriptor is null"));
    }

    let raw = unsafe { &*raw };

    let clap_version = raw.clap_version;

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

    Ok((
        PluginDescriptor { id, name, version, vendor, description, url, manual_url, support_url },
        clap_version,
    ))
}
