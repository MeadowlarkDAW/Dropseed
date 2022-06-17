//mod clap_plugin_host;

mod engine;
mod graph;

#[cfg(feature = "clap-host")]
mod clap;

pub mod plugin;
pub mod utils;

pub use engine::audio_thread::DSEngineAudioThread;
pub use engine::events::from_engine::{
    DSEngineEvent, EngineDeactivatedInfo, PluginEvent, PluginScannerEvent,
};
pub use engine::events::to_engine::DSEngineRequest;
pub use engine::handle::DSEngineHandle;
pub use engine::main_thread::{
    ActivateEngineSettings, EdgeReq, ModifyGraphRequest, ModifyGraphRes, PluginIDReq,
};
pub use engine::plugin_scanner::{ScannedPlugin, ScannedPluginKey};
pub use graph::shared_pool::PluginInstanceID;
pub use graph::{
    AudioGraphSaveState, Edge, ParamGestureInfo, ParamModifiedInfo, PluginActivationStatus,
    PluginEdges, PluginHandle, PluginParamsExt, PortType,
};
pub use plugin::audio_buffer::{AudioPortBuffer, AudioPortBufferMut};
pub use plugin::events::event_queue::{EventQueue, ProcEvent, ProcEventRef};
pub use plugin::ext::audio_ports::{AudioPortInfo, MainPortsLayout, PluginAudioPortsExt};
pub use plugin::ext::params::{ParamID, ParamInfo, ParamInfoFlags};
pub use plugin::host_request::{HostInfo, HostRequest};
pub use plugin::process_info::{ProcBuffers, ProcInfo, ProcessStatus};
pub use utils::fixed_point::FixedPoint64;
