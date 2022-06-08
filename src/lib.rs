//mod clap_plugin_host;

mod audio_thread;
mod engine;
mod engine_handle;
mod event;
mod fixed_point;
mod graph;
mod host_request;
mod plugin_scanner;
mod thread_id;

#[cfg(feature = "clap-host")]
mod clap;

pub mod plugin;
pub mod reducing_queue;

pub use audio_thread::DAWEngineAudioThread;
pub use engine::{EdgeReq, ModifyGraphRequest, ModifyGraphRes, PluginIDReq};
pub use engine_handle::{ActivateEngineSettings, DAWEngineHandle, DAWEngineRequest};
pub use event::{DAWEngineEvent, EngineDeactivatedInfo, PluginEvent, PluginScannerEvent};
pub use fixed_point::FixedPoint64;
pub use graph::shared_pool::PluginInstanceID;
pub use graph::{
    AudioGraphSaveState, Edge, ParamGestureInfo, ParamModifiedInfo, PluginActivationStatus,
    PluginEdges, PluginParamsExt,
};
pub use host_request::{HostInfo, HostRequest};
pub use plugin::audio_buffer::{AudioPortBuffer, AudioPortBufferMut};
pub use plugin::event_queue::EventQueue;
pub use plugin::events;
pub use plugin::ext::audio_ports::{AudioPortInfo, MainPortsLayout, PluginAudioPortsExt};
pub use plugin::ext::params::{ParamID, ParamInfo, ParamInfoFlags};
pub use plugin::process_info::{ProcInfo, ProcessStatus};
pub use plugin_scanner::{ScannedPlugin, ScannedPluginKey};

pub use audio_graph::DefaultPortType as PortType;
