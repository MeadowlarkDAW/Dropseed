use std::error::Error;

use dropseed_core::RtGCHandle;

pub mod audio_port;
pub mod audio_port_buffer;
mod descriptor;
pub mod features;
mod id;
pub(crate) mod node_host_audio_thread;
pub(crate) mod node_host_main_thread;
mod node_request;
mod process_data;
mod render;

#[cfg(feature = "note-data")]
pub mod note_name;
#[cfg(feature = "note-data")]
pub mod note_port;

pub use descriptor::NodeDescriptor;
pub use id::StableID;
pub use node_request::*;
pub use process_data::*;

use self::{audio_port::AudioPortInfo, render::RenderMode};

#[cfg(feature = "note-data")]
use self::note_name::NoteName;
#[cfg(feature = "note-data")]
use self::note_port::NotePortInfo;

pub trait NodeFactory {
    fn descriptor(&self) -> NodeDescriptor;

    fn new(&mut self) -> Result<Box<dyn NodeMainThr>, Box<dyn Error>>;
}

pub trait NodeMainThr {
    #[allow(unused)]
    fn init(&mut self, requests: NodeRequestMainThr, gc: RtGCHandle) {}

    fn activate(
        &mut self,
        sample_rate: f64,
        min_frames: u32,
        max_frames: u32,
    ) -> Result<Box<dyn NodeAudioThr>, Box<dyn Error>>;

    fn on_main_thread(&mut self) {}

    #[cfg(feature = "external-plugin-guis")]
    fn _supports_gui(&self) -> bool {
        false
    }

    fn supports_timers(&self) -> bool {
        false
    }

    fn audio_in_ports(&self) -> &[AudioPortInfo] {
        &[]
    }

    fn audio_out_ports(&self) -> &[AudioPortInfo] {
        &[]
    }

    /// The latency of this node in samples.
    fn latency(&self) -> u32 {
        0
    }

    #[cfg(feature = "note-data")]
    fn note_in_ports(&self) -> &[NotePortInfo] {
        &[]
    }

    #[cfg(feature = "note-data")]
    fn note_out_ports(&self) -> &[NotePortInfo] {
        &[]
    }

    /// Return `true` if this node uses a custom save state.
    ///
    /// The host will only call `NodeMainThr::save_custom_state()` and
    /// `NodeMainThr::load_custom_state()` if this returns true.
    fn uses_custom_save_state(&self) -> bool {
        false
    }

    /// Save the state as raw bytes and return it.
    ///
    /// If the node failed to save the state, return an error.
    fn save_custom_state(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(Vec::new())
    }

    /// Load the given state.
    ///
    /// If the node failed to load the state, return an error.
    #[allow(unused)]
    fn load_custom_state(&mut self, state: Vec<u8>) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    /// Return true if this node has a hard requirement to process in real-time.
    ///
    /// This is especially useful for nodes acting as a proxy to a hardware device.
    fn has_hard_realtime_requirement(&self) -> bool {
        false
    }

    /// Called when the host changes the rendering mode.
    ///
    /// If the node could not apply the new rendering mode, return an error.
    #[allow(unused)]
    fn set_render_mode(&mut self, mode: RenderMode) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    #[cfg(feature = "note-data")]
    fn note_names(&self) -> Vec<NoteName> {
        Vec::new()
    }
}

pub trait NodeAudioThr: Send + 'static {
    /// Initialize the node. This gives a channel for the node to make requests to the host.
    ///
    /// This is only called once.
    #[allow(unused)]
    fn init(&mut self, requests: NodeRequestAudioThr) {}

    /// Called before processing starts (the plugin is woken up from sleep).
    ///
    /// If the processing can't start for any reason, return an error.
    fn start_processing(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    /// Called before the plugin is sent to sleep (processing stops).
    fn stop_processing(&mut self) {}

    /// The node's process method.
    fn process<'a>(&mut self, proc: &ProcData<'a>) -> ProcessStatus;

    #[cfg(any(feature = "c-bindings", feature = "clap-hosting"))]
    /// The process method called if this node is an internal plugin using the C bindings or
    /// an external CLAP plugin.
    fn _process_ffi<'a>(&mut self, proc: *const clap_sys::process::clap_process) -> ProcessStatus {
        ProcessStatus::Error
    }

    /// Called when the node should clear all its buffers, perform a full reset of the processing
    /// state (filters, oscillators, envelopes, lfo, etc.) and kill all voices.
    fn reset(&mut self) {}
}
