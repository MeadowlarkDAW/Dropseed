mod clap_plugin_host;

pub mod audio_buffer;
pub mod engine;
pub mod info;

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
