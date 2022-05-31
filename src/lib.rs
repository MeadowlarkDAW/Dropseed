//mod clap_plugin_host;

mod engine;
mod event;
mod fixed_point;
mod graph;
mod host_request;
mod plugin_scanner;
mod thread_id;

#[cfg(feature = "clap-host")]
mod clap;

pub mod plugin;

pub use engine::{EdgeReq, ModifyGraphRequest, ModifyGraphRes, PluginIDReq, RustyDAWEngine};
pub use event::{DAWEngineEvent, PluginScannerEvent};
pub use fixed_point::FixedPoint64;
pub use graph::shared_pool::PluginInstanceID;
pub use graph::{Edge, PluginActivationStatus, PluginEdges, SharedSchedule};
pub use host_request::{HostInfo, HostRequest};
pub use plugin::audio_buffer::{AudioPortBuffer, AudioPortBufferMut};
pub use plugin::ext::audio_ports::{AudioPortInfo, MainPortsLayout, PluginAudioPortsExt};
pub use plugin::process_info::{ProcInfo, ProcessStatus};
pub use plugin_scanner::{ScannedPlugin, ScannedPluginKey};

pub use audio_graph::DefaultPortType as PortType;
