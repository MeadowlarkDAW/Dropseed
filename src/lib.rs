//mod clap_plugin_host;

mod engine;
mod event;
mod garbage_collector;
mod graph;
mod host;
mod plugin_scanner;

#[cfg(feature = "clap-host")]
mod clap;

pub mod plugin;

pub use engine::RustyDAWEngine;
pub use event::{DAWEngineEvent, PluginScannerEvent};
pub use graph::audio_buffer_pool::AudioPortBuffer;
pub use host::{HostInfo, HostRequest};
pub use plugin::process_info::{ProcInfo, ProcessStatus};
