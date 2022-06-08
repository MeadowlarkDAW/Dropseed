//mod clap_plugin_host;

mod engine;
mod graph;

#[cfg(feature = "clap-host")]
mod clap;

pub mod plugin;
pub mod utils;

pub use engine::audio_thread::DAWEngineAudioThread;
pub use engine::events::from_engine::{
    DAWEngineEvent, EngineDeactivatedInfo, PluginEvent, PluginScannerEvent,
};
pub use engine::events::to_engine::DAWEngineRequest;
pub use engine::handle::DAWEngineHandle;
pub use engine::sandboxed::main_thread::{
    ActivateEngineSettings, EdgeReq, ModifyGraphRequest, ModifyGraphRes, PluginIDReq,
};
pub use engine::sandboxed::plugin_scanner::{ScannedPlugin, ScannedPluginKey};
pub use graph::shared_pool::PluginInstanceID;
pub use graph::{
    AudioGraphSaveState, Edge, ParamGestureInfo, ParamModifiedInfo, PluginActivationStatus,
    PluginEdges, PluginParamsExt,
};
pub use plugin::audio_buffer::{AudioPortBuffer, AudioPortBufferMut};
pub use plugin::events::event_queue::EventQueue;
pub use plugin::ext::audio_ports::{AudioPortInfo, MainPortsLayout, PluginAudioPortsExt};
pub use plugin::ext::params::{ParamID, ParamInfo, ParamInfoFlags};
pub use plugin::host_request::{HostInfo, HostRequest};
pub use plugin::process_info::{ProcInfo, ProcessStatus};
pub use utils::fixed_point::FixedPoint64;

pub use audio_graph::DefaultPortType as PortType;
