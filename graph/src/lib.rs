mod audio_thread;
pub(crate) mod channel;
mod main_thread;
pub mod node;
mod settings;

use dropseed_core::RtGCHandle;
use rtrb::RingBuffer;

pub use audio_thread::DsGraphAudioThr;
pub use main_thread::DsGraphMainThr;
pub use settings::DsGraphSettings;

/// Create a new Dropseed graph.
///
/// This will return the main thread counterpart and the audio thread
/// counterpart of the graph. Send the `DsGraphAudioThr` counterpart to
/// your realtime audio thread.
pub fn new_dropseed_graph(
    settings: &DsGraphSettings,
    gc: RtGCHandle,
) -> (DsGraphMainThr, DsGraphAudioThr) {
    let (to_audio_tx, from_main_rx) = RingBuffer::new(settings.channel_size.max(16));

    (DsGraphMainThr::new(to_audio_tx, gc), DsGraphAudioThr::new(from_main_rx))
}
