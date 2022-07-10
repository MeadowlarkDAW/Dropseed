//mod clap_plugin_host;

mod engine;
mod graph;

#[cfg(feature = "clap-host")]
mod clap;

pub mod plugins;
pub mod resource_loader;
pub mod transport;
pub mod utils;

pub use clack_host::events::io::EventBuffer;
pub use clack_host::utils::FixedPoint;

pub use engine::audio_thread::DSEngineAudioThread;
pub use engine::events::from_engine::{
    DSEngineEvent, EngineDeactivatedInfo, PluginEvent, PluginScannerEvent,
};
pub use engine::events::to_engine::DSEngineRequest;
pub use engine::handle::DSEngineHandle;
pub use engine::main_thread::{
    ActivateEngineSettings, EdgeReq, EdgeReqPortID, EngineActivatedInfo, ModifyGraphRequest,
    ModifyGraphRes, PluginIDReq,
};
pub use engine::plugin_scanner::{RescanPluginDirectoriesRes, ScannedPlugin, ScannedPluginKey};
pub use graph::plugin;
pub use graph::plugin::audio_buffer::{AudioPortBuffer, AudioPortBufferMut};
pub use graph::plugin::events::ProcEvent;
pub use graph::plugin::ext::audio_ports::{AudioPortInfo, MainPortsLayout, PluginAudioPortsExt};
pub use graph::plugin::ext::params::{ParamID, ParamInfo, ParamInfoFlags};
pub use graph::plugin::host_request::{HostInfo, HostRequest};
pub use graph::plugin::process_info::{ProcBuffers, ProcInfo, ProcessStatus};
pub use graph::shared_pool::PluginInstanceID;
pub use graph::{
    ActivatePluginError, AudioGraphSaveState, Edge, NewPluginRes, ParamGestureInfo,
    ParamModifiedInfo, PluginActivationStatus, PluginEdges, PluginHandle, PluginParamsExt,
    PortType,
};
pub use resource_loader::ResourceLoader;
pub use transport::TransportInfo;
