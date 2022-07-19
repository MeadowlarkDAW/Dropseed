use clack_host::instance::PluginInstance;
use dropseed_core::plugin::ext::audio_ports::PluginAudioPortsExt;

pub(crate) mod factory;

mod host;
use host::*;

mod plugin;

mod process;

pub struct ClapPluginMainThread {
    instance: PluginInstance<ClapHost>,
    audio_ports_ext: PluginAudioPortsExt,
}
