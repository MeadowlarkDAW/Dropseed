//mod clap_plugin_host;

pub mod engine;
pub mod error;
pub mod graph;
pub mod host;
pub mod plugin;
pub mod plugin_scanner;

pub use graph::audio_buffer_pool::AudioPortBuffer;
pub use host::Host;
pub use plugin::process_info::{ProcInfo, ProcessStatus};

#[derive(Debug, Clone, Copy, PartialEq)]
enum EngineState {
    Stopped,
    Running,
    Stopping,
}

pub struct RustyDAWEngine {
    state: EngineState,
}

impl RustyDAWEngine {}
