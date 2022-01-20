mod clap_plugin_host;

pub mod audio_buffer;
pub mod audio_ports;
pub mod c_char_helpers;
pub mod channel_map;
pub mod engine;
pub mod error;
pub mod info;
pub mod process;

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
